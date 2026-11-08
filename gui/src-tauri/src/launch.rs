//! Launching and tracking browser/automation processes.
//!
//! A launch is a bare subprocess: `camoufox[-bin|.exe] -no-remote -profile
//! <userdata>` with the identity in `CAMOU_CONFIG_1..N` and startup prefs in
//! `CAMOU_PREFS_1..N` — the same channels the Python launcher uses, minus
//! Playwright. Every profile is its own process and profile directory, so
//! concurrent launches isolate naturally.

use std::collections::HashMap;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use serde_json::{json, Map, Value};

use crate::forwarder::ForwarderHandle;
use crate::types::Profile;

/// Environment-variable size the browser side tolerates (see
/// `python/src/camoufox/utils.py::get_env_vars`): 2047 chars per variable on
/// Windows, 32767 elsewhere.
pub fn chunk_limit() -> usize {
    if cfg!(windows) {
        2047
    } else {
        32767
    }
}

/// Chunk `value` into `<prefix>1..N` by Unicode scalar values, exactly like
/// the Python launcher slices its JSON string.
pub fn chunk_var(value: &str, prefix: &str, limit: usize) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut i = 1usize;
    let mut current = String::new();
    let mut len = 0usize;
    for ch in value.chars() {
        if len == limit {
            out.push((format!("{prefix}{i}"), std::mem::take(&mut current)));
            i += 1;
            len = 0;
        }
        current.push(ch);
        len += 1;
    }
    if len > 0 || out.is_empty() {
        out.push((format!("{prefix}{i}"), current));
    }
    out
}

/// The `network.proxy.*` prefs for a launch. Authenticated proxies point the
/// browser at the local forwarder (`local_port`, always SOCKS5 regardless of
/// the upstream kind); unauthenticated ones at the upstream directly. SOCKS5
/// carries remote DNS so hostnames resolve at the proxy exit.
pub fn proxy_prefs(proxy: &crate::types::ProxyConfig, local_port: Option<u16>) -> Map<String, Value> {
    let mut prefs = Map::new();
    let active = matches!(proxy.kind.as_str(), "http" | "socks5")
        && !proxy.host.is_empty()
        && proxy.port > 0;
    if !active {
        return prefs;
    }
    prefs.insert("network.proxy.type".into(), json!(1));
    prefs.insert("network.proxy.share_proxy_settings".into(), json!(true));
    if let Some(port) = local_port {
        // Browser side: SOCKS5 to the forwarder, for either upstream kind.
        prefs.insert("network.proxy.socks".into(), json!("127.0.0.1"));
        prefs.insert("network.proxy.socks_port".into(), json!(port));
        prefs.insert("network.proxy.socks_version".into(), json!(5));
        prefs.insert("network.proxy.socks5_remote_dns".into(), json!(true));
        return prefs;
    }
    match proxy.kind.as_str() {
        "http" => {
            prefs.insert("network.proxy.http".into(), json!(proxy.host));
            prefs.insert("network.proxy.http_port".into(), json!(proxy.port));
            prefs.insert("network.proxy.ssl".into(), json!(proxy.host));
            prefs.insert("network.proxy.ssl_port".into(), json!(proxy.port));
        }
        "socks5" => {
            prefs.insert("network.proxy.socks".into(), json!(proxy.host));
            prefs.insert("network.proxy.socks_port".into(), json!(proxy.port));
            prefs.insert("network.proxy.socks_version".into(), json!(5));
            prefs.insert("network.proxy.socks5_remote_dns".into(), json!(true));
        }
        _ => unreachable!("kind filtered above"),
    }
    prefs
}

/// Prefs written into a fresh profile's user.js once, so a bare first launch
/// does not show Firefox's first-run prompts. Camoufox's camoufox.cfg already
/// covers telemetry and updates; these are the profile-level leftovers.
const FIRST_RUN_USER_JS: &str = r#"// CamFox: suppress first-run prompts.
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("browser.aboutwelcome.enabled", false);
user_pref("datareporting.policy.dataSubmissionPolicyBypassNotification", true);
user_pref("browser.sessionstore.resume_from_crash", false);
"#;

pub fn prepare_userdata(userdata: &Path) -> Result<(), String> {
    std::fs::create_dir_all(userdata)
        .map_err(|e| format!("cannot create {}: {e}", userdata.display()))?;
    let user_js = userdata.join("user.js");
    if !user_js.exists() {
        std::fs::write(&user_js, FIRST_RUN_USER_JS)
            .map_err(|e| format!("cannot write {}: {e}", user_js.display()))?;
    }
    Ok(())
}

/// Size the real browser window to the identity's claimed outer window.
///
/// The spoof rewrites `window.outer*`/`screenX`/`screenY` at the JS level,
/// but a default-sized OS window behind a claimed 1920×1032 is visible to the
/// user and invites doubt. Firefox stores its window geometry in the
/// profile's xulstore.json, so the claimed geometry is written there before
/// every launch — a regenerated identity's new size wins over the window
/// size the profile remembered.
pub fn write_window_xulstore(
    userdata: &Path,
    width: u32,
    height: u32,
    screen_x: i32,
    screen_y: i32,
) -> Result<(), String> {
    let store = serde_json::json!({
        "chrome://browser/content/browser.xhtml": {
            "screenX": screen_x.to_string(),
            "screenY": screen_y.to_string(),
            "width": width.to_string(),
            "height": height.to_string(),
            "sizemode": "normal",
        }
    });
    let path = userdata.join("xulstore.json");
    std::fs::write(&path, serde_json::to_string(&store).map_err(|e| e.to_string())?)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// (width, height, screenX, screenY) from a stored identity, if present.
pub fn identity_window(config: &Value) -> Option<(u32, u32, i32, i32)> {
    let obj = config.as_object()?;
    let num = |k: &str| obj.get(k).and_then(Value::as_f64);
    let w = num("window.outerWidth")?;
    let h = num("window.outerHeight")?;
    let x = num("window.screenX")?;
    let y = num("window.screenY")?;
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    Some((w as u32, h as u32, x as i32, y as i32))
}

/// Whether the pid still has a live process.
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        let output = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .creation_flags(0x0800_0000)
            .output();
        match output {
            Ok(out) => String::from_utf8_lossy(&out.stdout)
                .lines()
                // CSV: "image","pid",... — the pid is the second field; a
                // filtered-out pid prints a localised INFO line instead.
                .any(|l| {
                    l.split(',')
                        .nth(1)
                        .map(|f| f.trim_matches('"').trim() == pid.to_string())
                        .unwrap_or(false)
                }),
            Err(_) => true, // cannot tell; keep the stricter error path
        }
    }
    #[cfg(not(windows))]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(true)
    }
}

/// Kill a process tree; a pid that is already gone is success (a crashed
/// browser must not make the GUI's Close button report an error).
pub fn kill_tree(pid: u32) -> Result<(), String> {
    let result = kill_tree_inner(pid);
    if result.is_err() && !pid_alive(pid) {
        return Ok(());
    }
    result
}

fn kill_tree_inner(pid: u32) -> Result<(), String> {
    #[cfg(windows)]
    {
        let status = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(0x0800_0000)
            .status()
            .map_err(|e| format!("taskkill failed: {e}"))?;
        if !status.success() {
            return Err(format!("taskkill exited with {status}"));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let status = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .status()
            .map_err(|e| format!("kill failed: {e}"))?;
        if !status.success() {
            return Err(format!("kill exited with {status}"));
        }
        Ok(())
    }
}

/// Flatten profile.config + prefs (+ proxy prefs) into the CAMOU_* env pairs.
pub fn camou_env(profile: &Profile, local_port: Option<u16>) -> Result<Vec<(String, String)>, String> {
    let limit = chunk_limit();
    let mut env = Vec::new();
    let config = profile
        .config
        .as_ref()
        .ok_or_else(|| "profile has no fingerprint config (generate it first)".to_string())?;
    let config_str = serde_json::to_string(config).map_err(|e| e.to_string())?;
    env.extend(chunk_var(&config_str, "CAMOU_CONFIG_", limit));

    let mut prefs = profile
        .prefs
        .as_ref()
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for (k, v) in proxy_prefs(&profile.proxy, local_port) {
        prefs.insert(k, v);
    }
    if !prefs.is_empty() {
        let prefs_str = serde_json::to_string(&Value::Object(prefs)).map_err(|e| e.to_string())?;
        env.extend(chunk_var(&prefs_str, "CAMOU_PREFS_", limit));
    }
    Ok(env)
}

/// Parse PowerShell CIM JSON output into (pid, command line) pairs.
pub fn parse_process_list(text: &str) -> Vec<(u32, String)> {
    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let items: Vec<Value> = match value {
        Value::Array(items) => items,
        v @ Value::Object(_) => vec![v],
        _ => return Vec::new(),
    };
    items
        .into_iter()
        .filter_map(|v| {
            let pid = v.get("ProcessId").and_then(Value::as_u64)? as u32;
            let cmdline = v
                .get("CommandLine")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            Some((pid, cmdline))
        })
        .collect()
}

/// The value after `-profile` in a command line, quote-aware.
///
/// The real browser's argv is `... -profile "C:\path with spaces\alpha"`, while
/// a manually started process may pass it unquoted, so both forms are read.
pub fn profile_arg(cmdline: &str) -> Option<String> {
    let idx = cmdline.find("-profile")?;
    let rest = cmdline[idx + "-profile".len()..].trim_start();
    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        return Some(stripped[..end].to_string());
    }
    rest.split_whitespace().next().map(str::to_string)
}

/// Kernel processes whose `-profile` points into our profiles directory, as
/// (pid, profile id). After a GUI restart these browsers are still running
/// with no state to close them by, so they are re-attached at startup.
pub fn find_running_profiles(profiles_root: &Path) -> Result<Vec<(u32, String)>, String> {
    #[cfg(windows)]
    let image = "camoufox.exe";
    #[cfg(not(windows))]
    let image = "camoufox-bin";

    let script = format!(
        "Get-CimInstance Win32_Process -Filter \"Name='{image}'\" | Select-Object ProcessId,CommandLine | ConvertTo-Json -Compress"
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(creation_flags_no_window())
        .output()
        .map_err(|e| format!("process scan failed: {e}"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(find_running_profiles_sync(profiles_root, &text))
}

/// Pure matcher over CIM output so the path logic stays unit-testable.
pub fn find_running_profiles_sync(profiles_root: &Path, cim_text: &str) -> Vec<(u32, String)> {
    let root_norm = profiles_root.to_string_lossy().replace('/', "\\");
    let root_norm = root_norm.trim_end_matches('\\');
    let mut found = Vec::new();
    for (pid, cmdline) in parse_process_list(cim_text) {
        let Some(profile) = profile_arg(&cmdline) else {
            continue;
        };
        // The userdata dir is <profiles_root>\<id>\userdata; take the id.
        let profile_norm = profile.replace('/', "\\");
        let marker = format!("{root_norm}\\");
        if let Some(pos) = profile_norm.find(&marker) {
            let rest = &profile_norm[pos + marker.len()..];
            let id = rest.split('\\').next().unwrap_or("");
            if !id.is_empty() {
                found.push((pid, id.to_string()));
            }
        }
    }
    found
}

fn creation_flags_no_window() -> u32 {
    #[cfg(windows)]
    {
        0x0800_0000 // CREATE_NO_WINDOW
    }
    #[cfg(not(windows))]
    {
        0
    }
}

/// Spawn the browser for one profile. Returns the child.
pub fn spawn_browser(
    executable: &str,
    profile: &Profile,
    userdata: &Path,
    local_port: Option<u16>,
) -> Result<Child, String> {
    if !Path::new(executable).is_file() {
        return Err(format!("browser executable not found: {executable}"));
    }
    let mut command = Command::new(executable);
    for arg in &profile.launch_args {
        command.arg(arg);
    }
    command
        .args(["-no-remote", "-profile"])
        .arg(userdata)
        .stdin(Stdio::null())
        .creation_flags(creation_flags_no_window());
    for (k, v) in camou_env(profile, local_port)? {
        command.env(k, v);
    }
    for (k, v) in &profile.extra_env {
        command.env(k, v);
    }
    command.spawn().map_err(|e| format!("cannot spawn {executable}: {e}"))
}

#[derive(Debug)]
pub enum Running {
    Browser {
        pid: u32,
        forwarder: Option<ForwarderHandle>,
        /// When this launch was registered; the supervisor ignores very young
        /// entries so a slow start is not mistaken for an exit.
        since: std::time::Instant,
    },
    Automation {
        child: Child,
    },
}

impl Running {
    pub fn pid(&self) -> u32 {
        match self {
            Running::Browser { pid, .. } => *pid,
            Running::Automation { child } => child.id(),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Running::Browser { .. } => "browser",
            Running::Automation { .. } => "automation",
        }
    }
}

/// Every live process whose `-profile` points at this profile, launcher and
/// browser alike (Windows starts a launcher process that carries the same
/// argv before the browser process appears).
pub fn profile_processes(profiles_root: &Path, id: &str) -> Vec<u32> {
    find_running_profiles(profiles_root)
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, found)| found == id)
        .map(|(pid, _)| pid)
        .collect()
}

/// Ask a process to close without forcing it: `taskkill` without `/F` posts
/// WM_CLOSE, which lets Firefox flush cookies and session state.
pub fn soft_kill(pid: u32) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T"])
            .creation_flags(0x0800_0000)
            .status();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill").args(["-TERM", &pid.to_string()]).status();
    }
}

/// Post WM_CLOSE to every visible top-level window this pid owns; returns how
/// many were found.
///
/// `taskkill` without `/F` only queues the message and returns at once, and
/// its exit code says nothing about delivery. Posting directly makes the
/// request verifiable and independent of the console locale.
#[cfg(windows)]
pub fn post_close(pid: u32) -> u32 {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
    };

    thread_local! {
        static TARGET: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
        static POSTED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }

    unsafe extern "system" fn callback(hwnd: HWND, _lparam: LPARAM) -> BOOL {
        let target = TARGET.with(|t| t.get());
        let mut window_pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut window_pid) };
        if window_pid == target && unsafe { IsWindowVisible(hwnd) } != 0 {
            if unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) } != 0 {
                POSTED.with(|p| p.set(p.get() + 1));
            }
        }
        TRUE
    }

    TARGET.with(|t| t.set(pid));
    POSTED.with(|p| p.set(0));
    unsafe { EnumWindows(Some(callback), 0) };
    POSTED.with(|p| p.get())
}

#[cfg(not(windows))]
pub fn post_close(_pid: u32) -> u32 {
    0
}

/// Wait until a process carrying this profile exists and return its pid.
///
/// The spawned process may be Windows' launcher, which exits as soon as the
/// browser is up; the browser itself carries the same `-profile`.
pub fn wait_for_browser_pid(profiles_root: &Path, id: &str, timeout: Duration) -> Option<u32> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Ok(found) = find_running_profiles(profiles_root) {
            if let Some((pid, _)) = found.iter().find(|(_, found_id)| found_id == id) {
                return Some(*pid);
            }
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(600));
    }
}

/// Close every process carrying this profile: post WM_CLOSE so Firefox flushes
/// cookies/session, poll the known pids cheaply, then force what is left.
pub fn close_profile_processes(profiles_root: &Path, id: &str) -> Result<(), String> {
    let pids = profile_processes(profiles_root, id);
    if pids.is_empty() {
        return Ok(());
    }
    for pid in &pids {
        post_close(*pid);
        soft_kill(*pid);
    }
    // Firefox takes several seconds to shut down (session and cookie flush);
    // poll the known pids, which costs one tasklist each, instead of
    // re-scanning command lines.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while std::time::Instant::now() < deadline {
        if pids.iter().all(|pid| !pid_alive(*pid)) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    for pid in profile_processes(profiles_root, id) {
        let _ = kill_tree(pid);
    }
    Ok(())
}

pub type RunningMap = Mutex<HashMap<String, Running>>;

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Stop one running entry; browser entries go through the wait thread's
/// regular exit path, automation entries are killed outright.
pub fn stop(pid: u32) -> Result<(), String> {
    kill_tree(pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ProxyConfig, ScreenSpec, Summary};
    use std::fs;

    #[test]
    fn chunk_var_splits_and_rejoins_exactly() {
        let value = "a".repeat(6000);
        let parts = chunk_var(&value, "X_", 2047);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].0, "X_1");
        assert_eq!(parts[2].0, "X_3");
        assert_eq!(parts[0].1.chars().count(), 2047);
        assert_eq!(parts.iter().map(|(_, v)| v.as_str()).collect::<String>(), value);
    }

    #[test]
    fn chunk_var_multibyte_counts_chars_not_bytes() {
        // 1500 CJK chars = 4500 bytes; chunked by chars like the Python launcher.
        let value = "汉".repeat(1500);
        let parts = chunk_var(&value, "C_", 2047);
        assert_eq!(parts.len(), 1); // 1500 chars < 2047
        assert_eq!(parts[0].1, value);
    }

    #[test]
    fn chunk_var_empty_yields_one_empty_var() {
        let parts = chunk_var("", "C_", 2047);
        assert_eq!(parts, vec![("C_1".to_string(), String::new())]);
    }

    #[test]
    fn camou_env_roundtrip_matches_sidecar_config() {
        let mut profile = Profile::default();
        profile.config = Some(json!({"navigator.userAgent": "Mozilla/5.0 测试 UA"}));
        profile.prefs = Some(json!({"intl.locale.requested": "zh-CN"}));
        let env = camou_env(&profile, None).unwrap();
        let config_key = env.iter().find(|(k, _)| k == "CAMOU_CONFIG_1").unwrap();
        assert_eq!(config_key.1, r#"{"navigator.userAgent":"Mozilla/5.0 测试 UA"}"#);
        assert!(env.iter().any(|(k, _)| k == "CAMOU_PREFS_1"));
    }

    #[test]
    fn proxy_prefs_direct_socks() {
        let proxy = ProxyConfig {
            kind: "socks5".into(),
            host: "5.6.7.8".into(),
            port: 1080,
            username: String::new(),
            password: String::new(),
        };
        let prefs = proxy_prefs(&proxy, None);
        assert_eq!(prefs["network.proxy.socks"], json!("5.6.7.8"));
        assert_eq!(prefs["network.proxy.socks_port"], json!(1080));
        assert_eq!(prefs["network.proxy.socks5_remote_dns"], json!(true));
    }

    #[test]
    fn proxy_prefs_forwarder_overrides_host() {
        let proxy = ProxyConfig {
            kind: "http".into(),
            host: "10.0.0.1".into(),
            port: 8080,
            username: "u".into(),
            password: "p".into(),
        };
        let prefs = proxy_prefs(&proxy, Some(45678));
        assert_eq!(prefs["network.proxy.socks"], json!("127.0.0.1"));
        assert_eq!(prefs["network.proxy.socks_port"], json!(45678));
        assert!(!prefs.contains_key("network.proxy.http"));
    }

    #[test]
    fn proxy_prefs_empty_kind_is_noop() {
        let prefs = proxy_prefs(&ProxyConfig::default(), None);
        assert!(prefs.is_empty());
    }

    #[test]
    fn profile_request_shapes_carry_proxy() {
        let mut p = Profile::default();
        p.screen = Some(ScreenSpec { width: 1920, height: 1080 });
        p.summary = Some(Summary::default());
        let req = p.to_prepare_request();
        assert_eq!(req["screen"]["width"], 1920);
        assert_eq!(req["proxy"]["kind"], "none");
    }

    #[test]
    fn xulstore_carries_claimed_geometry() {
        let dir = std::env::temp_dir().join(format!("camfox-xul-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        write_window_xulstore(&dir, 1920, 1032, -8, 0).unwrap();
        let data: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("xulstore.json")).unwrap()).unwrap();
        let win = &data["chrome://browser/content/browser.xhtml"];
        assert_eq!(win["width"], "1920");
        assert_eq!(win["height"], "1032");
        assert_eq!(win["screenX"], "-8");
        assert_eq!(win["sizemode"], "normal");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn identity_window_reads_outer_and_screen_pos() {
        let config = json!({
            "window.outerWidth": 1920.0,
            "window.outerHeight": 1032.0,
            "window.screenX": -8.0,
            "window.screenY": 0.0,
        });
        assert_eq!(identity_window(&config), Some((1920, 1032, -8, 0)));
        assert_eq!(identity_window(&json!({})), None);
    }

    #[test]
    fn parses_cim_list_and_single_object() {
        let list = r#"[{"ProcessId": 42, "CommandLine": "camoufox.exe -profile C:\\d\\profiles\\p1\\userdata"},
                       {"ProcessId": 43, "CommandLine": null}]"#;
        let out = parse_process_list(list);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, 42);
        assert!(out[1].1.is_empty());
        let single = r#"{"ProcessId": 7, "CommandLine": "x"}"#;
        assert_eq!(parse_process_list(single)[0].0, 7);
        assert!(parse_process_list("not json").is_empty());
    }

    #[test]
    fn pid_alive_sees_self_and_not_an_impossible_pid() {
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(0x7FFF_FFF0));
    }

    #[test]
    fn profile_arg_reads_quoted_and_bare_paths() {
        assert_eq!(
            profile_arg(r#""C:\k\camoufox.exe" -no-remote -profile "C:\d\profiles\p1\userdata""#),
            Some(r"C:\d\profiles\p1\userdata".to_string())
        );
        assert_eq!(
            profile_arg(r"C:\k\camoufox.exe -no-remote -profile C:\d\p2\userdata -silent"),
            Some(r"C:\d\p2\userdata".to_string())
        );
        assert_eq!(profile_arg("camoufox.exe -no-remote"), None);
    }

    #[test]
    fn running_profile_scan_matches_profiles_root() {
        let root = std::env::temp_dir().join(format!("camfox-scan-{}", std::process::id()));
        let profiles = root.join("profiles");
        std::fs::create_dir_all(&profiles).unwrap();
        let userdata = profiles.join("p1").join("userdata");
        // Real browser command line: executable and profile path are quoted.
        let cim = format!(
            r#"[{{"ProcessId": 100, "CommandLine": "\"C:\\k\\camoufox.exe\" -no-remote -profile \"{}\""}},
               {{"ProcessId": 101, "CommandLine": "k.exe -profile C:\\Temp\\playwright_oth\\userdata"}},
               {{"ProcessId": 102, "CommandLine": "k.exe -no-remote"}}]"#,
            userdata.to_string_lossy().replace('\\', "\\\\")
        );
        let found = find_running_profiles_sync(&profiles, &cim);
        assert_eq!(found, vec![(100, "p1".to_string())]);
        let _ = fs::remove_dir_all(&root);
    }
}
