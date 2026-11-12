//! What the user chose for each phone (rotation, mirror, camera and quality), kept on this
//! computer and applied again when that phone comes back (see `Host`, `Config::remember`).
//!
//! Phones are known by the install id of their `HELLO`: a random UUID, never a hardware id.
//! File: `~/.config/oculibre/phone-settings.json`.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use oculibre_protocol::SetMode;
use serde::{Deserialize, Serialize};

use crate::session::View;

/// What this computer knows about one phone. The settings stay `None` until the user changes
/// them; `name`, `last_seen` and `last_via` are updated at every visit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub view: Option<View>,
    pub mode: Option<SetMode>,
    /// Its name at the last visit (`HELLO`).
    pub name: Option<String>,
    /// Last visit, UTC ("2026-11-12T10:53Z"; older files: the date alone).
    pub last_seen: Option<String>,
    /// "usb" or "wifi".
    pub last_via: Option<String>,
}

/// Every phone's settings, saved at each change.
pub struct Profiles {
    path: PathBuf,
    phones: Mutex<BTreeMap<String, Profile>>,
}

impl Profiles {
    /// Reads the file; a missing or unreadable file means no settings yet.
    pub fn load(path: PathBuf) -> Self {
        let phones = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self {
            path,
            phones: Mutex::new(phones),
        }
    }

    pub fn get(&self, phone_id: &str) -> Option<Profile> {
        self.phones.lock().unwrap().get(phone_id).cloned()
    }

    /// Every known phone, by install id.
    pub fn all(&self) -> Vec<(String, Profile)> {
        self.phones
            .lock()
            .unwrap()
            .iter()
            .map(|(id, p)| (id.clone(), p.clone()))
            .collect()
    }

    /// Forgets a phone's settings and visits.
    pub fn forget(&self, phone_id: &str) -> Result<(), String> {
        let mut phones = self.phones.lock().unwrap();
        if phones.remove(phone_id).is_none() {
            return Ok(());
        }
        let text = serde_json::to_string_pretty(&*phones).map_err(|e| e.to_string())?;
        crate::store::save_private(&self.path, text.as_bytes())
    }

    /// Changes a phone's settings and saves the file.
    pub fn update(&self, phone_id: &str, change: impl FnOnce(&mut Profile)) -> Result<(), String> {
        let mut phones = self.phones.lock().unwrap();
        change(phones.entry(phone_id.to_string()).or_default());
        let text = serde_json::to_string_pretty(&*phones).map_err(|e| e.to_string())?;
        crate::store::save_private(&self.path, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Rotate;

    #[test]
    fn settings_survive_a_restart() {
        let path = std::env::temp_dir().join(format!(
            "oculibre-test-profiles-{}.json",
            std::process::id()
        ));
        let _ = fs::remove_file(&path);
        let view = View {
            rotate: Rotate::Fixed(0),
            mirror: true,
        };
        let mode = SetMode {
            camera: "1".into(),
            width: 1280,
            height: 720,
            fps: 60,
            path: Some("unlisted".into()),
        };
        {
            let profiles = Profiles::load(path.clone());
            assert_eq!(profiles.get("phone-a"), None);
            profiles.update("phone-a", |p| p.view = Some(view)).unwrap();
            profiles
                .update("phone-a", |p| p.mode = Some(mode.clone()))
                .unwrap();
            profiles
                .update("phone-b", |p| p.view = Some(View::default()))
                .unwrap();
        }
        let again = Profiles::load(path.clone());
        assert_eq!(
            again.get("phone-a"),
            Some(Profile {
                view: Some(view),
                mode: Some(mode),
                ..Profile::default()
            })
        );
        assert_eq!(again.get("phone-b").unwrap().mode, None);
        assert_eq!(again.all().len(), 2);
        again.forget("phone-b").unwrap();
        assert_eq!(Profiles::load(path.clone()).get("phone-b"), None);
        fs::remove_file(&path).unwrap();
    }
}
