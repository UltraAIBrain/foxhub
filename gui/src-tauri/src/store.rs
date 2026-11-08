//! Profile store: one JSON file per profile under `<app data>/profiles/<id>/`.
//! `<app data>/profiles/<id>/userdata/` is the Firefox profile directory — the
//! isolation unit for cookies, storage and extensions.

use std::fs;
use std::path::{Path, PathBuf};

use crate::types::Profile;

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(root: &Path) -> Store {
        Store {
            root: root.to_path_buf(),
        }
    }

    fn profiles_dir(&self) -> PathBuf {
        self.root.join("profiles")
    }

    pub fn profile_dir(&self, id: &str) -> PathBuf {
        self.profiles_dir().join(id)
    }

    pub fn userdata_dir(&self, id: &str) -> PathBuf {
        self.profile_dir(id).join("userdata")
    }

    fn profile_file(&self, id: &str) -> PathBuf {
        self.profile_dir(id).join("profile.json")
    }

    pub fn list(&self) -> Result<Vec<Profile>, String> {
        let dir = self.profiles_dir();
        // A fresh install has no profiles yet; create the dir on first read
        // instead of erroring on the missing path.
        fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let mut out = Vec::new();
        let entries = fs::read_dir(&dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let file = entry.path().join("profile.json");
            if !file.is_file() {
                continue;
            }
            let profile = read_profile(&file)?;
            out.push(profile);
        }
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
        Ok(out)
    }

    pub fn get(&self, id: &str) -> Result<Profile, String> {
        read_profile(&self.profile_file(id))
    }

    /// Validate ids so they can never escape the profiles directory.
    fn valid_id(id: &str) -> Result<(), String> {
        let ok = !id.is_empty()
            && id.len() <= 64
            && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        if ok {
            Ok(())
        } else {
            Err(format!("invalid profile id: {id:?}"))
        }
    }

    pub fn save(&self, profile: &Profile) -> Result<(), String> {
        Self::valid_id(&profile.id)?;
        let dir = self.profile_dir(&profile.id);
        fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let json = serde_json::to_string_pretty(profile).map_err(|e| e.to_string())?;
        let file = self.profile_file(&profile.id);
        let tmp = dir.join(".profile.json.tmp");
        fs::write(&tmp, json).map_err(|e| e.to_string())?;
        fs::rename(&tmp, &file).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        Self::valid_id(id)?;
        let dir = self.profile_dir(id);
        if !dir.exists() {
            return Err(format!("profile {id} does not exist"));
        }
        fs::remove_dir_all(&dir).map_err(|e| format!("cannot delete {}: {e}", dir.display()))
    }

    pub fn groups(&self) -> Result<Vec<String>, String> {
        let mut groups: Vec<String> = self
            .list()?
            .into_iter()
            .filter(|p| !p.group.is_empty())
            .map(|p| p.group)
            .collect();
        groups.sort();
        groups.dedup();
        Ok(groups)
    }
}

fn read_profile(file: &Path) -> Result<Profile, String> {
    let text =
        fs::read_to_string(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    serde_json::from_str::<Profile>(&text)
        .map_err(|e| format!("cannot parse {}: {e}", file.display()))
}

/// A fresh id: time + counter, no external uuid dependency.
pub fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("p{nanos:016x}{n:04x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("camfox-store-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample(id: &str) -> Profile {
        Profile {
            id: id.into(),
            name: format!("name-{id}"),
            group: "g".into(),
            ..Default::default()
        }
    }

    #[test]
    fn save_get_roundtrip() {
        let root = tmp_root("roundtrip");
        let store = Store::new(&root);
        let mut p = sample("abc123");
        p.os = Some("windows".into());
        store.save(&p).unwrap();
        let got = store.get("abc123").unwrap();
        assert_eq!(got.name, "name-abc123");
        assert_eq!(got.os.as_deref(), Some("windows"));
        fs::create_dir_all(store.userdata_dir("abc123")).unwrap();
        assert!(store.userdata_dir("abc123").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn list_sorted_and_delete() {
        let root = tmp_root("list");
        let store = Store::new(&root);
        let mut a = sample("aaa");
        a.created_at = 100;
        let mut b = sample("bbb");
        b.created_at = 200;
        store.save(&a).unwrap();
        store.save(&b).unwrap();
        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, "bbb"); // newest first
        store.delete("aaa").unwrap();
        assert!(store.get("aaa").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn list_on_fresh_store_is_empty() {
        // First run: the store was constructed but no profile was ever saved.
        let root = tmp_root("fresh");
        let store = Store::new(&root);
        assert_eq!(store.list().unwrap(), Vec::<Profile>::new());
        assert!(store.profiles_dir().exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn legacy_profile_without_webrtc_fields_reads_as_auto() {
        // Profiles saved before WebRTC control existed have no webrtcMode.
        let root = tmp_root("legacy");
        let store = Store::new(&root);
        let legacy = r#"{
            "id": "old1", "name": "old", "group": "", "notes": "",
            "createdAt": 1, "lastLaunchedAt": null, "launchCount": 0,
            "fingerprintMode": "fixed", "os": "windows", "screen": null,
            "window": null, "locale": null, "timezone": null, "geoip": false,
            "proxy": {"kind": "none", "host": "", "port": 0},
            "automation": false, "config": null, "prefs": null,
            "launchExecutable": null, "launchArgs": [], "extraEnv": {},
            "summary": null
        }"#;
        let dir = store.profile_dir("old1");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("profile.json"), legacy).unwrap();
        let profile = store.get("old1").unwrap();
        assert_eq!(profile.webrtc_mode, "auto");
        assert_eq!(profile.webrtc_ipv4, "");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn rejects_path_escape_ids() {
        let root = tmp_root("ids");
        let store = Store::new(&root);
        let mut p = sample("../../etc");
        p.id = "../../etc".into();
        assert!(store.save(&p).is_err());
        assert!(store.delete("../x").is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
