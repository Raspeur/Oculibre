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
    /// Closing the window keeps Oculibre in the system tray (the virtual camera stays).
    pub tray: bool,
    /// "fr", "en", or "" for the system's language: the window (src/i18n.ts) and the tray menu.
    pub language: String,
    /// USB without developer mode (#21, Linux): switch every Android phone plugged in, not only
    /// those the user chose once. Off by default: a phone plugged in to charge is left alone.
    pub usb_any_phone: bool,
}

/// The languages of the app (the window has the same in src/i18n.ts).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Fr,
}

impl Lang {
    /// The settings' choice; "" = the system's (French if it is French, else English).
    pub fn resolve(choice: &str) -> Self {
        let tag = if choice.is_empty() {
            system_language()
        } else {
            choice.to_string()
        };
        if tag.to_ascii_lowercase().starts_with("fr") {
            Lang::Fr
        } else {
            Lang::En
        }
    }
}

/// The system's language as gettext finds it: LANGUAGE (a list, "fr:en"), then LC_ALL,
/// LC_MESSAGES, LANG ("fr_FR.UTF-8"). Empty when none is set.
fn system_language() -> String {
    ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(|value| value.split(':').next().unwrap_or("").to_string())
        .find(|value| !value.is_empty() && value != "C" && value != "POSIX")
        .unwrap_or_default()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            wifi: true,
            portrait: false,
            decoder: "auto".into(),
            tray: true,
            language: String::new(),
            usb_any_phone: false,
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

    /// The language to use.
    pub fn lang(&self) -> Lang {
        Lang::resolve(&self.language)
    }

    /// Whether going from `self` to `other` needs a new receiver (its configuration is fixed).
    pub fn needs_restart(&self, other: &Self) -> bool {
        self.wifi != other.wifi || self.portrait != other.portrait
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_choice() {
        assert_eq!(Lang::resolve("fr"), Lang::Fr);
        assert_eq!(Lang::resolve("en"), Lang::En);
        assert_eq!(Lang::resolve("fr_CA"), Lang::Fr);
        assert_eq!(Lang::resolve("de"), Lang::En);
    }

    #[test]
    fn older_settings_files_still_load() {
        // Files written before the language existed: the missing field takes its default.
        let old = r#"{"wifi":false,"portrait":false,"decoder":"auto","tray":true}"#;
        let settings: AppSettings = serde_json::from_str(old).unwrap();
        assert!(!settings.wifi);
        assert_eq!(settings.language, "");
    }
}
