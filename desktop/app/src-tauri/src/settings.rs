//! The settings of this computer, chosen in the Settings screen (the phones' own settings are in
//! `oculibre_core::profiles`). File: `~/.config/oculibre/app-settings.json`.

use std::fs;
use std::path::PathBuf;

use oculibre_core::host;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    /// Accept paired phones over the network (encrypted). Changing it restarts the receiver.
    pub wifi: bool,
    /// Portrait virtual camera (landscape like every webcam otherwise). Restarts the receiver.
    pub portrait: bool,
    /// "auto", "vaapi", "nvdec", "software" or "openh264": applied from the next stream.
    pub decoder: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            wifi: true,
            portrait: false,
            decoder: "auto".into(),
        }
    }
}

impl AppSettings {
    fn file() -> PathBuf {
        host::config_dir().join("app-settings.json")
    }

    /// The saved settings, or the defaults (missing or unreadable file).
    pub fn load() -> Self {
        fs::read_to_string(Self::file())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        oculibre_core::store::save_private(&Self::file(), text.as_bytes())
    }

    /// Whether going from `self` to `other` needs a new receiver (its configuration is fixed).
    pub fn needs_restart(&self, other: &Self) -> bool {
        self.wifi != other.wifi || self.portrait != other.portrait
    }
}
