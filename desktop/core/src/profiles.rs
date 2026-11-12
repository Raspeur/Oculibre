//! What the user chose for each phone (rotation, mirror, camera and quality), kept on this
//! computer and applied again when that phone comes back (see `Host`, `Config::remember`).
//!
//! Phones are known by the install id of their `HELLO`: a random UUID, never a hardware id.
//! File: `~/.config/oculibre/phone-settings.json`.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use oculibre_protocol::SetMode;
use serde::{Deserialize, Serialize};

use crate::session::View;

/// The settings of one phone. A field stays `None` until the user changes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub view: Option<View>,
    pub mode: Option<SetMode>,
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

    /// Changes a phone's settings and saves the file.
    pub fn update(&self, phone_id: &str, change: impl FnOnce(&mut Profile)) -> Result<(), String> {
        let mut phones = self.phones.lock().unwrap();
        change(phones.entry(phone_id.to_string()).or_default());
        let text = serde_json::to_string_pretty(&*phones).map_err(|e| e.to_string())?;
        save_atomically(&self.path, text.as_bytes())
    }
}

/// Writes a temporary file, then renames it over the old one: a crash halfway leaves the old file
/// whole, never a half-written one. Readable by this user only, like the other files there.
fn save_atomically(path: &PathBuf, bytes: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let temporary = path.with_extension("json.tmp");
    let write = || -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    };
    write().map_err(|e| format!("cannot save {}: {e}", path.display()))
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
                mode: Some(mode)
            })
        );
        assert_eq!(again.get("phone-b").unwrap().mode, None);
        fs::remove_file(&path).unwrap();
    }
}
