//! App settings, persisted at `<app data>/settings.json`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Interpreter that has the `camoufox` package installed.
    pub python_path: String,
    /// Explicit path to the `gui/sidecar` directory (auto-resolved when empty).
    pub sidecar_path: String,
    /// Launch this executable instead of the managed kernel (absolute path).
    pub kernel_executable: String,
    /// "zh-CN" | "en"
    pub language: String,
    /// Close every running browser when the GUI exits.
    pub close_browsers_on_exit: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            python_path: "python".into(),
            sidecar_path: String::new(),
            kernel_executable: String::new(),
            language: "zh-CN".into(),
            close_browsers_on_exit: true,
        }
    }
}

impl Settings {
    pub fn load(root: &Path) -> Settings {
        let file = root.join("settings.json");
        match fs::read_to_string(&file) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        let file = root.join("settings.json");
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&file, json).map_err(|e| format!("cannot write {}: {e}", file.display()))
    }

    pub fn sidecar_dir(&self) -> Option<PathBuf> {
        if self.sidecar_path.is_empty() {
            None
        } else {
            Some(PathBuf::from(&self.sidecar_path))
        }
    }
}
