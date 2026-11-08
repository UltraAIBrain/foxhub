//! Client for the Python sidecar (`gui/sidecar/camfox_sidecar`).
//!
//! One subprocess per call: request JSON on stdin, the response is the last
//! stdout line that parses as a JSON object with an "ok" key — everything else
//! (camoufox's own progress output, warnings) is treated as log text.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

pub struct Sidecar {
    pub python: String,
    pub dir: PathBuf,
}

/// Where `gui/sidecar` lives, tried in order: explicit setting, env override,
/// beside the executable (packaged layout), Tauri resource dir, the dev-tree
/// location relative to this crate.
pub fn resolve_sidecar_dir(explicit: Option<&Path>, resource_dir: Option<&Path>) -> Option<PathBuf> {
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
        candidates.push(res.join("sidecar"));
    }
    // Compile-time dev-tree location: `pnpm tauri dev` runs the binary without
    // CARGO_MANIFEST_DIR in the environment.
    candidates.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("sidecar"));
    candidates
        .into_iter()
        .find(|dir| dir.join("camfox_sidecar").join("__main__.py").is_file())
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
        let mut command = Command::new(&self.python);
        command
            .args(["-m", "camfox_sidecar", cmd])
            .current_dir(&self.dir)
            .env("PYTHONPATH", &self.dir)
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
        // In this repo the dev tree location always exists.
        let found = resolve_sidecar_dir(None, None);
        assert!(found.is_some());
        assert!(found.unwrap().join("camfox_sidecar").is_dir());
    }
}
