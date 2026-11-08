//! Client for the Python sidecar (`gui/sidecar/camfox_sidecar`).
//!
//! One subprocess per call: request JSON on stdin, the response is the last
//! stdout line that parses as a JSON object with an "ok" key — everything else
//! (camoufox's own progress output, warnings) is treated as log text.
//!
//! The sidecar runs either from source (`python -m camfox_sidecar`, dev) or
//! as a PyInstaller-frozen `camfox-sidecar.exe` bundled as a resource
//! (release), which is what makes the packaged app work without a system
//! Python.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

/// What to spawn for sidecar calls.
#[derive(Debug, Clone)]
pub enum SidecarProgram {
    /// Source layout: `<dir>/camfox_sidecar/__main__.py`, run with an
    /// interpreter that has the `camoufox` package installed.
    Source { python: String, dir: PathBuf },
    /// Frozen PyInstaller onedir: `<exe>` plus its `_internal` payload.
    Frozen { exe: PathBuf },
}

pub const FROZEN_EXE_NAME: &str = "camfox-sidecar.exe";

pub struct Sidecar {
    pub program: SidecarProgram,
}

/// Locate the sidecar, preferring a frozen build. Directories probed in
/// order: explicit setting, env override, beside the executable, the Tauri
/// resource dir (with the `sidecar/` mapping from tauri.conf.json), and the
/// compile-time dev-tree location.
pub fn resolve_sidecar(explicit: Option<&Path>, resource_dir: Option<&Path>) -> Option<SidecarProgram> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = explicit {
        candidates.push(p.to_path_buf());
    }
    if let Ok(env_dir) = std::env::var("CAMFOX_SIDECAR_DIR") {
        if !env_dir.is_empty() {
            candidates.push(PathBuf::from(env_dir));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join("sidecar"));
        }
    }
    if let Some(res) = resource_dir {
        // Mapped in tauri.conf.json; cover both dir-copy interpretations.
        candidates.push(res.join("sidecar"));
        candidates.push(res.join("sidecar").join("camfox-sidecar"));
    }
    // Compile-time dev-tree location: `pnpm tauri dev` runs the binary without
    // CARGO_MANIFEST_DIR in the environment.
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("sidecar"));

    for dir in candidates {
        let frozen = dir.join(FROZEN_EXE_NAME);
        if frozen.is_file() {
            return Some(SidecarProgram::Frozen { exe: frozen });
        }
        if dir.join("camfox_sidecar").join("__main__.py").is_file() {
            return Some(SidecarProgram::Source {
                python: String::new(), // filled in by the caller
                dir,
            });
        }
    }
    None
}

pub fn parse_output(text: &str) -> (Option<Value>, Vec<String>) {
    let mut logs: Vec<String> = Vec::new();
    let mut response = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(line) {
            Ok(v) if v.is_object() && v.get("ok").is_some() => response = Some(v),
            Ok(_) | Err(_) => logs.push(line.to_string()),
        }
    }
    (response, logs)
}

impl Sidecar {
    pub fn call(&self, cmd: &str, req: &Value, timeout: Duration) -> Result<Value, String> {
        let child = self
            .spawn(cmd, req)
            .map_err(|e| format!("cannot start sidecar ({cmd}): {e}"))?;
        let output = wait_with_timeout(child, timeout)?;
        let (response, logs) = parse_output(&output);
        finish(response, logs, cmd)
    }

    /// Spawn a long-running command (automation) and return the child. The
    /// caller reads stdout through `ChildStdout`.
    pub fn spawn_long(
        &self,
        cmd: &str,
        req: &Value,
    ) -> Result<std::process::Child, String> {
        self.spawn(cmd, req).map_err(|e| format!("cannot start sidecar ({cmd}): {e}"))
    }

    fn spawn(&self, cmd: &str, req: &Value) -> Result<std::process::Child, String> {
        let mut command = match &self.program {
            SidecarProgram::Frozen { exe } => {
                let mut c = Command::new(exe);
                c.current_dir(exe.parent().unwrap_or(Path::new(".")));
                c
            }
            SidecarProgram::Source { python, dir } => {
                let mut c = Command::new(python);
                c.args(["-m", "camfox_sidecar"])
                    .current_dir(dir)
                    .env("PYTHONPATH", dir);
                c
            }
        };
        command
            .arg(cmd)
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUTF8", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        use std::io::Write;
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "sidecar stdin unavailable".to_string())?;
        stdin
            .write_all(req.to_string().as_bytes())
            .map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())?;
        // Drop stdin so the sidecar's read() returns instead of waiting for EOF
        // forever if it reads more than we sent.
        child.stdin.take();
        Ok(child)
    }
}

fn wait_with_timeout(
    mut child: std::process::Child,
    timeout: Duration,
) -> Result<String, String> {
    use std::io::Read;

    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "sidecar stdout unavailable".to_string())?;
    // Read on a thread so a hung sidecar cannot block us past the timeout.
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut output = String::new();
        let mut buf = [0u8; 8192];
        loop {
            match stdout.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => output.push_str(&String::from_utf8_lossy(&buf[..n])),
            }
        }
        let _ = tx.send(output);
    });

    match rx.recv_timeout(timeout) {
        Ok(output) => {
            let _ = child.wait();
            Ok(output)
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(format!("sidecar timed out after {:?}", timeout))
        }
    }
}

fn finish(response: Option<Value>, logs: Vec<String>, cmd: &str) -> Result<Value, String> {
    match response {
        Some(v) if v.get("ok").and_then(Value::as_bool) == Some(true) => Ok(v),
        Some(v) => {
            let error = v
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown sidecar error")
                .to_string();
            Err(format_sidecar_error(cmd, &error, &logs))
        }
        None => Err(format_sidecar_error(cmd, "sidecar produced no response", &logs)),
    }
}

fn format_sidecar_error(cmd: &str, error: &str, logs: &[String]) -> String {
    if logs.is_empty() {
        format!("[{cmd}] {error}")
    } else {
        let tail: Vec<&str> = logs.iter().rev().take(5).rev().map(String::as_str).collect();
        format!("[{cmd}] {error}\n{}", tail.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_last_ok_line() {
        let text = "rich progress\n{\"event\":\"log\"}\n{\"ok\":true,\"value\":1}";
        let (response, logs) = parse_output(text);
        assert_eq!(response.unwrap(), json!({"ok": true, "value": 1}));
        // Event lines carry no "ok" flag, so in one-shot output they are logs.
        assert_eq!(logs, vec!["rich progress", "{\"event\":\"log\"}"]);
    }

    #[test]
    fn failure_line_is_a_response() {
        let text = "{\"ok\":false,\"error\":\"boom\"}";
        let (response, _logs) = parse_output(text);
        assert!(response.is_some());
        assert!(finish(response, vec![], "prepare").is_err());
    }

    #[test]
    fn no_response_reports_logs() {
        let err = finish(None, vec!["traceback line".into()], "prepare");
        assert!(err.unwrap_err().contains("traceback line"));
    }

    #[test]
    fn resolve_dev_layout() {
        // In this repo the dev tree location always exists (source layout).
        let found = resolve_sidecar(None, None);
        assert!(matches!(found, Some(SidecarProgram::Source { .. })));
    }

    #[test]
    fn frozen_exe_wins_when_present() {
        let root = std::env::temp_dir().join(format!("camfox-sidecar-test-{}", std::process::id()));
        let pkg = root.join("camfox_sidecar");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(root.join(FROZEN_EXE_NAME), b"mz").unwrap();
        let found = resolve_sidecar(Some(&root), None);
        match found {
            Some(SidecarProgram::Frozen { exe }) => {
                assert_eq!(exe, root.join(FROZEN_EXE_NAME))
            }
            other => panic!("expected frozen, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
