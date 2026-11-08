//! Tauri commands: every operation the frontend can invoke.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::forwarder;
use crate::launch::{self, Running, RunningMap};
use crate::settings::Settings;
use crate::sidecar::{resolve_sidecar, Sidecar, SidecarProgram};
use crate::store::{new_id, Store};
use crate::types::{PrepareResult, Profile, ProxyCheck, ProxyConfig, RunningInfo};

pub struct AppState {
    pub store: Store,
    pub settings: std::sync::Mutex<Settings>,
    pub running: RunningMap,
    /// Profiles currently between "launch requested" and "process spawned";
    /// blocks a second launch of the same profile during the slow draw.
    pub launching: std::sync::Mutex<std::collections::HashSet<String>>,
    pub data_dir: std::path::PathBuf,
}

impl AppState {
    pub fn new(data_dir: std::path::PathBuf) -> AppState {
        let settings = Settings::load(&data_dir);
        AppState {
            store: Store::new(&data_dir),
            settings: std::sync::Mutex::new(settings),
            running: std::sync::Mutex::new(HashMap::new()),
            launching: std::sync::Mutex::new(std::collections::HashSet::new()),
            data_dir,
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    /// A sidecar client built from the current settings. `resource_dir` is
    /// only available with an AppHandle, so commands pass it in.
    pub fn sidecar(&self, app: &AppHandle) -> Result<Sidecar, String> {
        let settings = self.settings();
        let resource_dir = app.path().resource_dir().ok();
        let mut program = resolve_sidecar(settings.sidecar_dir().as_deref(), resource_dir.as_deref())
            .ok_or_else(|| {
                "sidecar not found — in dev builds point Settings at gui/sidecar".to_string()
            })?;
        if let SidecarProgram::Source { python, .. } = &mut program {
            // The frozen build ignores the interpreter; the source build needs
            // one with the camoufox package installed.
            if python.is_empty() {
                *python = settings.python_path.clone();
            }
        }
        Ok(Sidecar { program })
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.running.lock().unwrap().contains_key(id)
    }

    pub fn emit_status(app: &AppHandle, id: &str, status: &str, extra: Value) {
        let _ = app.emit(
            "profile-status",
            json!({ "id": id, "status": status }).merge(extra),
        );
    }
}

trait MergeValue {
    fn merge(self, other: Value) -> Value;
}

impl MergeValue for Value {
    fn merge(self, other: Value) -> Value {
        match (self, other) {
            (Value::Object(mut a), Value::Object(b)) => {
                a.extend(b);
                Value::Object(a)
            }
            (a, _) => a,
        }
    }
}

// ---------------------------------------------------------------- profiles

#[tauri::command]
pub fn list_profiles(state: State<'_, AppState>) -> Result<Vec<Profile>, String> {
    state.store.list()
}

#[tauri::command]
pub fn list_groups(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    state.store.groups()
}

#[tauri::command]
pub fn save_profile(mut profile: Profile, state: State<'_, AppState>) -> Result<Profile, String> {
    if profile.name.trim().is_empty() {
        return Err("profile name is required".into());
    }
    profile.name = profile.name.trim().to_string();
    if profile.id.is_empty() {
        profile.id = new_id();
        profile.created_at = launch::now();
    } else {
        // Keep generated fields the caller did not send (the editor sends the
        // whole object, so this only guards against stale UI state).
        let existing = state.store.get(&profile.id)?;
        profile.config = profile.config.or(existing.config);
        profile.prefs = profile.prefs.or(existing.prefs);
        profile.launch_executable = profile
            .launch_executable
            .or(existing.launch_executable);
        if profile.summary.is_none() {
            profile.summary = existing.summary;
        }
    }
    state.store.save(&profile)?;
    Ok(profile)
}

#[tauri::command]
pub fn delete_profile(id: String, state: State<'_, AppState>) -> Result<(), String> {
    if state.is_running(&id) {
        return Err("profile is running — close it first".into());
    }
    state.store.delete(&id)
}

#[tauri::command]
pub fn duplicate_profile(id: String, state: State<'_, AppState>) -> Result<Profile, String> {
    let mut copy = state.store.get(&id)?;
    copy.id = new_id();
    copy.name = format!("{} copy", copy.name);
    copy.created_at = launch::now();
    copy.last_launched_at = None;
    copy.launch_count = 0;
    // A clone of a fingerprint is a second browser claiming one machine; drop
    // the identity so the caller regenerates it (prepare_profile).
    copy.config = None;
    copy.summary = None;
    state.store.save(&copy)?;
    Ok(copy)
}

/// Draw the identity for one profile through the sidecar and store the result.
#[tauri::command]
pub async fn prepare_profile(
    id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<PrepareResult, String> {
    prepare_profile_inner(id, &state, &app).await
}

pub async fn prepare_profile_inner(
    id: String,
    state: &AppState,
    app: &AppHandle,
) -> Result<PrepareResult, String> {
    let mut profile = state.store.get(&id)?;
    let sidecar = state.sidecar(app)?;
    let req = profile.to_prepare_request();
    let response = tauri::async_runtime::spawn_blocking(move || {
        sidecar.call("prepare", &req, Duration::from_secs(300))
    })
    .await
    .map_err(|e| e.to_string())??;

    profile.config = response.get("config").cloned();
    profile.prefs = response.get("prefs").cloned();
    profile.launch_executable = response
        .get("executable_path")
        .and_then(Value::as_str)
        .map(str::to_string);
    profile.launch_args = response
        .get("args")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    profile.extra_env = response
        .get("extra_env")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();
    profile.summary = response
        .get("summary")
        .cloned()
        .and_then(|s| serde_json::from_value(s).ok());
    let warnings = response
        .get("warnings")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    state.store.save(&profile)?;
    Ok(PrepareResult { profile, warnings })
}

#[tauri::command]
pub async fn launch_profile(
    id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    if state.is_running(&id) {
        return Err("profile is already running".into());
    }
    // One draw at a time per profile: a double-clicked open button must not
    // run two fingerprint generations and spawn two browsers.
    if !state.launching.lock().unwrap().insert(id.clone()) {
        return Err("profile is already launching".into());
    }
    AppState::emit_status(&app, &id, "launching", json!({}));
    let result = launch_profile_inner(&id, &state, &app).await;
    if result.is_err() {
        state.launching.lock().unwrap().remove(&id);
        AppState::emit_status(&app, &id, "failed", json!({ "error": result.clone().err() }));
    }
    result
}

async fn launch_profile_inner(
    id: &str,
    state: &State<'_, AppState>,
    app: &AppHandle,
) -> Result<(), String> {
    let mut profile = state.store.get(id)?;

    // Random mode (or a profile without a stored identity) draws fresh here.
    if profile.config.is_none() || profile.fingerprint_mode == "random" {
        profile = prepare_profile_inner(id.to_string(), state, app).await?.profile;
    }

    let executable = {
        let settings = state.settings();
        if !settings.kernel_executable.is_empty() {
            settings.kernel_executable.clone()
        } else {
            profile
                .launch_executable
                .clone()
                .filter(|p| !p.is_empty())
                .ok_or_else(|| "no kernel executable — download one in Kernel Management".to_string())?
        }
    };

    // Authenticated proxies go through the local forwarder; the browser only
    // ever sees 127.0.0.1 without credentials.
    let forwarder_handle = if profile.proxy.needs_forwarder() {
        Some(forwarder::start(profile.proxy.clone()).await?)
    } else {
        None
    };
    let local_port = forwarder_handle.as_ref().map(|f| f.port);

    let userdata = state.store.userdata_dir(id);
    launch::prepare_userdata(&userdata)?;

    // The OS window shows the identity's claimed geometry (see
    // write_window_xulstore); without a stored config there is nothing to
    // match, so Firefox keeps its remembered size.
    if let Some(config) = &profile.config {
        if let Some((w, h, x, y)) = launch::identity_window(config) {
            launch::write_window_xulstore(&userdata, w, h, x, y)?;
        }
    }

    let mut child = launch::spawn_browser(&executable, &profile, &userdata, local_port)?;
    let pid = child.id();

    profile.last_launched_at = Some(launch::now());
    profile.launch_count += 1;
    state.store.save(&profile)?;

    state.launching.lock().unwrap().remove(id);
    state.running.lock().unwrap().insert(
        id.to_string(),
        Running::Browser {
            pid,
            forwarder: forwarder_handle,
        },
    );
    AppState::emit_status(app, id, "running", json!({ "pid": pid }));

    // Exit watcher: clean up the forwarder and report the outcome.
    let app_handle = app.clone();
    let watch_id = id.to_string();
    std::thread::spawn(move || {
        let _ = child.wait();
        if let Some(running) = app_handle
            .state::<AppState>()
            .running
            .lock()
            .unwrap()
            .remove(&watch_id)
        {
            if let Running::Browser { forwarder, .. } = running {
                if let Some(f) = forwarder {
                    f.stop();
                }
            }
        }
        AppState::emit_status(&app_handle, &watch_id, "exited", json!({}));
    });
    Ok(())
}

#[tauri::command]
pub fn close_profile(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let running = state.running.lock().unwrap();
    let entry = running
        .get(&id)
        .ok_or_else(|| "profile is not running".to_string())?;
    launch::stop(entry.pid())
}

#[tauri::command]
pub fn running_profiles(state: State<'_, AppState>) -> Vec<RunningInfo> {
    state
        .running
        .lock()
        .unwrap()
        .iter()
        .map(|(id, r)| RunningInfo {
            id: id.clone(),
            kind: r.kind().to_string(),
            pid: r.pid(),
            ws_endpoint: None,
        })
        .collect()
}

// --------------------------------------------------------------- automation

/// Serve one profile over a Playwright websocket endpoint. The sidecar blocks
/// until closed; the reader thread picks the endpoint out of its output.
#[tauri::command]
pub fn open_automation(
    id: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), String> {
    if state.is_running(&id) {
        return Err("profile is already running".into());
    }
    let profile = state.store.get(&id)?;
    if profile.config.is_none() {
        return Err("profile has no fingerprint config — generate it first".into());
    }
    let sidecar = state.sidecar(&app)?;
    let req = profile.to_automation_request();
    let mut child = sidecar.spawn_long("automate", &req)?;
    // The request is already written and stdin closed by `spawn_long`.
    let pid = child.id();
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "automation stdout unavailable".to_string())?;

    state.running.lock().unwrap().insert(
        id.clone(),
        Running::Automation { child },
    );
    AppState::emit_status(&app, &id, "automation-connecting", json!({ "pid": pid }));

    let app_handle = app.clone();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if let Some(start) = line.find("ws://") {
                let rest = &line[start..];
                let end = rest
                    .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
                    .unwrap_or(rest.len());
                let endpoint = rest[..end].to_string();
                AppState::emit_status(
                    &app_handle,
                    &id,
                    "automation-ready",
                    json!({ "endpoint": endpoint }),
                );
            } else if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if v.get("ok").and_then(Value::as_bool) == Some(false) {
                    let error = v
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("automation failed")
                        .to_string();
                    AppState::emit_status(
                        &app_handle,
                        &id,
                        "failed",
                        json!({ "error": error }),
                    );
                    app_handle.state::<AppState>().running.lock().unwrap().remove(&id);
                    return;
                }
            }
        }
        // Process ended.
        if app_handle.state::<AppState>().running.lock().unwrap().remove(&id).is_some() {
            AppState::emit_status(&app_handle, &id, "exited", json!({}));
        }
    });
    Ok(())
}

#[tauri::command]
pub fn close_automation(id: String, state: State<'_, AppState>) -> Result<(), String> {
    close_profile(id, state)
}

// -------------------------------------------------------------- proxy check

#[tauri::command]
pub async fn check_proxy(
    proxy: ProxyConfig,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<ProxyCheck, String> {
    let sidecar = state.sidecar(&app)?;
    let req = json!({ "proxy": proxy });
    let response = tauri::async_runtime::spawn_blocking(move || {
        sidecar.call("check-proxy", &req, Duration::from_secs(90))
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(ProxyCheck {
        ip: response.get("ip").and_then(Value::as_str).map(str::to_string),
        timezone: response.get("timezone").and_then(Value::as_str).map(str::to_string),
        locale: response.get("locale").and_then(Value::as_str).map(str::to_string),
        latitude: response.get("latitude").and_then(Value::as_f64),
        longitude: response.get("longitude").and_then(Value::as_f64),
        geoip_error: response.get("geoip_error").and_then(Value::as_str).map(str::to_string),
        error: None,
    })
}

// ------------------------------------------------------------------ kernel

#[tauri::command]
pub async fn kernel_info(state: State<'_, AppState>, app: AppHandle) -> Result<Value, String> {
    let sidecar = state.sidecar(&app)?;
    let response = tauri::async_runtime::spawn_blocking(move || {
        sidecar.call("kernel-info", &json!({}), Duration::from_secs(60))
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(response)
}

/// Download a kernel, streaming the CLI output as `kernel-log` events.
#[tauri::command]
pub async fn kernel_fetch(
    specifier: Option<String>,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Value, String> {
    let sidecar = state.sidecar(&app)?;
    let req = json!({ "specifier": specifier });
    let (tx, rx) = std::sync::mpsc::channel::<Result<Value, String>>();
    std::thread::spawn(move || {
        let result = run_streaming(&sidecar, "kernel-fetch", &req, &app, "kernel-log");
        let _ = tx.send(result);
    });
    rx.recv_timeout(Duration::from_secs(3600))
        .map_err(|_| "kernel fetch timed out".to_string())?
}

/// Run a streaming sidecar command: `{"event":"log",...}` lines go out as
/// Tauri events, the final `{"ok":...}` line is the result.
fn run_streaming(
    sidecar: &Sidecar,
    cmd: &str,
    req: &Value,
    app: &AppHandle,
    event_name: &str,
) -> Result<Value, String> {
    let mut child = sidecar.spawn_long(cmd, req)?;
    let stdout = child.stdout.take().ok_or_else(|| "no stdout".to_string())?;
    use std::io::BufRead;
    let reader = std::io::BufReader::new(stdout);
    let mut logs: Vec<String> = Vec::new();
    for line in reader.lines() {
        let Ok(line) = line else { break };
        match serde_json::from_str::<Value>(&line) {
            Ok(v) if v.get("event").and_then(Value::as_str) == Some("log") => {
                if let Some(text) = v.get("line").and_then(Value::as_str) {
                    let _ = app.emit(event_name, json!({ "line": text }));
                    logs.push(text.to_string());
                }
            }
            Ok(v) if v.get("ok").is_some() => {
                let _ = child.wait();
                return if v.get("ok").and_then(Value::as_bool) == Some(true) {
                    Ok(v)
                } else {
                    Err(v
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("command failed")
                        .to_string())
                };
            }
            _ => logs.push(line),
        }
    }
    let _ = child.wait();
    Err(format!(
        "sidecar ended without a response\n{}",
        logs.iter().rev().take(5).rev().cloned().collect::<Vec<_>>().join("\n")
    ))
}

#[tauri::command]
pub async fn kernel_set(
    specifier: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Value, String> {
    let sidecar = state.sidecar(&app)?;
    let req = json!({ "specifier": specifier });
    let response = tauri::async_runtime::spawn_blocking(move || {
        sidecar.call("kernel-set", &req, Duration::from_secs(120))
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(response)
}

#[tauri::command]
pub async fn kernel_remove(
    specifier: String,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<Value, String> {
    let sidecar = state.sidecar(&app)?;
    let req = json!({ "specifier": specifier });
    let response = tauri::async_runtime::spawn_blocking(move || {
        sidecar.call("kernel-remove", &req, Duration::from_secs(120))
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(response)
}

// ------------------------------------------------------- batch import/export

#[tauri::command]
pub fn export_profiles(
    ids: Option<Vec<String>>,
    include_secrets: bool,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let all = state.store.list()?;
    let selected: Vec<Profile> = match ids {
        Some(ids) => all
            .into_iter()
            .filter(|p| ids.contains(&p.id))
            .collect(),
        None => all,
    };
    let mut profiles = selected;
    for p in &mut profiles {
        if !include_secrets && !p.proxy.password.is_empty() {
            p.proxy.password = String::new();
        }
    }
    serde_json::to_string_pretty(&json!({ "version": 1, "profiles": profiles }))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_profiles(
    content: String,
    regenerate: bool,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<usize, String> {
    let doc: Value =
        serde_json::from_str(&content).map_err(|e| format!("invalid import file: {e}"))?;
    let items = doc
        .get("profiles")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| "import file has no profiles array".to_string())?;
    let existing_names: Vec<String> = state
        .store
        .list()?
        .into_iter()
        .map(|p| p.name)
        .collect();

    let mut imported = 0;
    for item in items {
        let mut profile: Profile = serde_json::from_value(item.clone())
            .map_err(|e| format!("invalid profile entry: {e}"))?;
        profile.id = new_id();
        profile.created_at = launch::now();
        profile.last_launched_at = None;
        profile.launch_count = 0;
        let mut name = profile.name.clone();
        let mut n = 2;
        while existing_names.contains(&name) {
            name = format!("{} ({n})", profile.name);
            n += 1;
        }
        profile.name = name;
        if regenerate {
            profile.config = None;
            profile.summary = None;
        }
        state.store.save(&profile)?;
        if regenerate {
            prepare_profile_inner(profile.id.clone(), &state, &app).await?;
        }
        imported += 1;
    }
    Ok(imported)
}

// ------------------------------------------------------------------ cookies

fn guard_stopped(state: &AppState, id: &str) -> Result<(), String> {
    if state.is_running(id) {
        return Err("profile is running — close it first".into());
    }
    Ok(())
}

#[tauri::command]
pub fn export_cookies(id: String, format: String, state: State<'_, AppState>) -> Result<String, String> {
    guard_stopped(&state, &id)?;
    let userdata = state.store.userdata_dir(&id);
    match format.as_str() {
        crate::cookies::FORMAT_JSON => {
            let v = crate::cookies::export_json(&userdata)?;
            serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
        }
        crate::cookies::FORMAT_NETSCAPE => crate::cookies::export_netscape(&userdata),
        other => Err(format!("unknown cookie format: {other}")),
    }
}

#[tauri::command]
pub fn import_cookies(
    id: String,
    format: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    guard_stopped(&state, &id)?;
    let userdata = state.store.userdata_dir(&id);
    match format.as_str() {
        crate::cookies::FORMAT_JSON => {
            let cookies: Vec<Value> =
                serde_json::from_str(&content).map_err(|e| format!("invalid cookie JSON: {e}"))?;
            crate::cookies::import_json(&userdata, &cookies)
        }
        crate::cookies::FORMAT_NETSCAPE => crate::cookies::import_netscape(&userdata, &content),
        other => Err(format!("unknown cookie format: {other}")),
    }
}

// ------------------------------------------------- recovery and diagnostics

/// Re-attach browsers the GUI lost track of (restart, crash): any kernel
/// process whose `-profile` points at one of our userdata dirs is put back
/// into the running map so Close works on it again. Note the per-profile
/// proxy forwarder is process-local and cannot be recovered; an
/// authenticated-proxy profile must be relaunched for traffic to flow.
pub fn restore_running_browsers(app: &AppHandle) -> usize {
    let state = app.state::<AppState>();
    let profiles_root = state.store.profiles_dir();
    let found = match launch::find_running_profiles(&profiles_root) {
        Ok(f) => f,
        Err(_) => return 0, // scan is best-effort
    };
    let mut restored = 0;
    for (pid, id) in found {
        if state.store.get(&id).is_err() || state.is_running(&id) {
            continue;
        }
        state
            .running
            .lock()
            .unwrap()
            .insert(id.clone(), Running::Browser { pid, forwarder: None });
        AppState::emit_status(app, &id, "running", json!({ "pid": pid, "recovered": true }));
        restored += 1;
    }
    restored
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    /// "frozen" (bundled sidecar exe) | "source" (python -m camfox_sidecar).
    pub sidecar_mode: String,
    pub sidecar_dir: Option<String>,
    pub python: Option<String>,
    pub kernel_version: Option<String>,
    pub kernel_path: Option<String>,
    pub error: Option<String>,
}

/// Check the whole launch chain (sidecar resolution, interpreter, managed
/// kernel) so a broken setup is diagnosable from Settings instead of failing
/// obscurely on first launch.
#[tauri::command]
pub async fn diagnose(state: State<'_, AppState>, app: AppHandle) -> Result<Diagnostics, String> {
    let settings = state.settings();
    let resource_dir = app.path().resource_dir().ok();
    let (mode, dir, python) = match resolve_sidecar(
        settings.sidecar_dir().as_deref(),
        resource_dir.as_deref(),
    ) {
        Some(SidecarProgram::Frozen { exe }) => (
            "frozen".to_string(),
            Some(exe.parent().map(|p| p.display().to_string()).unwrap_or_default()),
            None,
        ),
        Some(SidecarProgram::Source { dir, .. }) => (
            "source".to_string(),
            Some(dir.display().to_string()),
            Some(settings.python_path.clone()),
        ),
        None => {
            return Ok(Diagnostics {
                sidecar_mode: "not found".into(),
                sidecar_dir: None,
                python: Some(settings.python_path.clone()),
                kernel_version: None,
                kernel_path: None,
                error: Some(
                    "sidecar directory not found — set it in Settings (dev builds: gui/sidecar)"
                        .into(),
                ),
            })
        }
    };
    let sidecar = state.sidecar(&app)?;
    let response = tauri::async_runtime::spawn_blocking(move || {
        sidecar.call("kernel-info", &json!({}), Duration::from_secs(60))
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(Diagnostics {
        sidecar_mode: mode,
        sidecar_dir: dir,
        python,
        kernel_version: response.get("version").and_then(Value::as_str).map(str::to_string),
        kernel_path: response.get("activePath").and_then(Value::as_str).map(str::to_string),
        error: None,
    })
}

// ----------------------------------------------------------------- settings

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn save_settings(settings: Settings, state: State<'_, AppState>) -> Result<(), String> {
    settings.save(&state.data_dir)?;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

#[tauri::command]
pub fn data_dir(state: State<'_, AppState>) -> String {
    state.data_dir.display().to_string()
}

/// Small file IO for the frontend's save/open dialogs.
#[tauri::command]
pub fn write_text_file(path: String, content: String) -> Result<(), String> {
    let path = Path::new(&path);
    std::fs::write(path, content).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

#[tauri::command]
pub fn read_text_file(path: String) -> Result<String, String> {
    let path = Path::new(&path);
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}
