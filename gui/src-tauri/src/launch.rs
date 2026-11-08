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
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Kill a process tree. Windows needs taskkill (the browser spawns children);
/// other targets signal the process directly.
pub fn kill_tree(pid: u32) -> Result<(), String> {
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
}
