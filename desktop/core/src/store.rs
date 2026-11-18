//! Files of this computer's settings (`~/.config/oculibre`, `%APPDATA%\Oculibre` on Windows):
//! written atomically, readable by this user only.

use std::fs;
use std::io::Write;
use std::path::Path;

/// Writes a temporary file, then renames it over the old one: a crash halfway leaves the old file
/// whole, never a half-written one. Readable by this user only.
pub fn save_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let temporary = path.with_extension("tmp");
    let write = || -> std::io::Result<()> {
        let mut file = create_private(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    };
    write().map_err(|e| format!("cannot save {}: {e}", path.display()))
}

/// Creates (or empties) a file only this user can read. Unix: mode 600. Windows: the user's
/// profile (`%APPDATA%`) already lets in only this user, the system and administrators, and a
/// new file inherits that.
pub fn create_private(path: &Path) -> std::io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
