//! Shared data types. The JSON is camelCase on the wire (Tauri convention);
//! `Profile` mirrors `profiles/<id>/profile.json` on disk.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScreenSpec {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyConfig {
    /// "none" | "http" | "socks5"
    pub kind: String,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            kind: "none".into(),
            host: String::new(),
            port: 0,
            username: String::new(),
            password: String::new(),
        }
    }
}

impl ProxyConfig {
    /// Credentials present? An authenticated proxy is served to the browser
    /// through the local forwarder, which injects them upstream.
    pub fn needs_forwarder(&self) -> bool {
        matches!(self.kind.as_str(), "http" | "socks5")
            && !self.host.is_empty()
            && self.port > 0
            && (!self.username.is_empty() || !self.password.is_empty())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    #[serde(default)]
    pub identity_id: Option<String>,
    #[serde(default)]
    pub ua: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub oscpu: Option<String>,
    #[serde(default)]
    pub screen: Option<Vec<u32>>,
    #[serde(default)]
    pub avail_screen: Option<Vec<u32>>,
    #[serde(default)]
    pub window: Option<Vec<u32>>,
    #[serde(default)]
    pub inner_window: Option<Vec<u32>>,
    #[serde(default)]
    pub color_depth: Option<u32>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub webrtc_ipv4: Option<String>,
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub hardware_concurrency: Option<u32>,
    #[serde(default)]
    pub device_pixel_ratio: Option<f64>,
    #[serde(default)]
    pub gpu_vendor: Option<String>,
    #[serde(default)]
    pub gpu_renderer: Option<String>,
    #[serde(default)]
    pub fonts_count: Option<u32>,
    #[serde(default)]
    pub voices_count: Option<u32>,
    #[serde(default)]
    pub media_devices: Option<Vec<u32>>,
}

fn default_webrtc_mode() -> String {
    "auto".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub group: String,
    pub notes: String,
    pub created_at: i64,
    pub last_launched_at: Option<i64>,
    pub launch_count: u64,
    /// "fixed" keeps the stored identity across launches; "random" regenerates
    /// it on every launch through the sidecar.
    pub fingerprint_mode: String,
    pub os: Option<String>,
    pub screen: Option<ScreenSpec>,
    pub window: Option<ScreenSpec>,
    pub locale: Option<String>,
    pub timezone: Option<String>,
    pub geoip: bool,
    /// "auto" | "block" | "custom"; profiles saved before the field existed
    /// deserialize as "auto".
    #[serde(default = "default_webrtc_mode")]
    pub webrtc_mode: String,
    #[serde(default)]
    pub webrtc_ipv4: String,
    pub proxy: ProxyConfig,
    pub automation: bool,
    /// The identity the browser reads via CAMOU_CONFIG_* (sidecar output).
    pub config: Option<Value>,
    /// Firefox prefs applied at startup via CAMOU_PREFS_* (sidecar output).
    pub prefs: Option<Value>,
    pub launch_executable: Option<String>,
    pub launch_args: Vec<String>,
    pub extra_env: std::collections::BTreeMap<String, String>,
    pub summary: Option<Summary>,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            group: String::new(),
            notes: String::new(),
            created_at: 0,
            last_launched_at: None,
            launch_count: 0,
            fingerprint_mode: "fixed".into(),
            os: None,
            screen: None,
            window: None,
            locale: None,
            timezone: None,
            geoip: false,
            webrtc_mode: "auto".into(),
            webrtc_ipv4: String::new(),
            proxy: ProxyConfig::default(),
            automation: false,
            config: None,
            prefs: None,
            launch_executable: None,
            launch_args: Vec::new(),
            extra_env: Default::default(),
            summary: None,
        }
    }
}

impl Profile {
    /// Fields the sidecar needs to draw the identity.
    pub fn to_prepare_request(&self) -> Value {
        serde_json::json!({
            "os": self.os,
            "screen": self.screen,
            "window": self.window,
            "locale": self.locale,
            "timezone": self.timezone,
            "geoip": self.geoip,
            "webrtc": {
                "mode": self.webrtc_mode,
                "ipv4": self.webrtc_ipv4,
            },
            "proxy": {
                "kind": self.proxy.kind,
                "host": self.proxy.host,
                "port": self.proxy.port,
                "username": self.proxy.username,
                "password": self.proxy.password,
            },
        })
    }

    pub fn to_automation_request(&self) -> Value {
        serde_json::json!({
            "config": self.config,
            "prefs": self.prefs,
            "os": self.os,
            "locale": self.locale,
            "headless": false,
            "proxy": {
                "kind": self.proxy.kind,
                "host": self.proxy.host,
                "port": self.proxy.port,
                "username": self.proxy.username,
                "password": self.proxy.password,
            },
        })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrepareResult {
    pub profile: Profile,
    /// LeakWarnings/FallbackWarnings from the launcher, shown to the user.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningInfo {
    pub id: String,
    pub kind: String,
    pub pid: u32,
    pub ws_endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyCheck {
    pub ip: Option<String>,
    pub timezone: Option<String>,
    pub locale: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub geoip_error: Option<String>,
    pub error: Option<String>,
}
