//! Files of this computer's settings (`~/.config/oculibre`): written atomically, readable by this
//! user only.

use std::fs;
use std::io::Write;
use std::path::Path;

/// Writes a temporary file, then renames it over the old one: a crash halfway leaves the old file
/// whole, never a half-written one. Readable by this user only.
pub fn save_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let temporary = path.with_extension("tmp");
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
