//! Is the virtual camera there, and setting it up when it is not (Linux, #18).
//!
//! [`status`] looks without any privilege (sysfs, `modinfo`). [`fix`] runs the root helper
//! `vcam-setup` (shipped by the package in /usr/lib/oculibre, source
//! desktop/app/src-tauri/linux/vcam-setup.sh) through `pkexec`: the desktop asks for the
//! administrator password with polkit's own window, so the app never sees it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

/// Where the package installs the helper (polkit action org.oculibre.vcam points there).
pub const HELPER: &str = "/usr/lib/oculibre/vcam-setup";

/// What the virtual camera looks like now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum VcamStatus {
    /// The "Oculibre" camera exists: apps see it.
    Ready { device: String },
    /// Another v4l2loopback camera exists (set up by another app): Oculibre writes to it, apps
    /// see it under its own name.
    Other { device: String },
    /// The module is installed for this kernel but not loaded: [`fix`] loads it.
    NotLoaded,
    /// No v4l2loopback module for the running kernel: a package is missing.
    NotInstalled,
}

/// Why [`fix`] did not work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum FixError {
    /// The password window was closed, or the password refused.
    Cancelled,
    /// No v4l2loopback module for this kernel.
    NotInstalled,
    /// Secure Boot refused the module (built on this computer, key not enrolled).
    SecureBoot,
    /// v4l2loopback is loaded with other settings by another app: after the next reboot.
    InUse,
    /// Anything else, with the details.
    Failed(String),
}

/// Looks at the cameras and the kernel's modules.
pub fn status() -> VcamStatus {
    let devices = devices(Path::new("/sys/class/video4linux"));
    if let Some(found) = classify(&devices) {
        return found;
    }
    if module_available() {
        VcamStatus::NotLoaded
    } else {
        VcamStatus::NotInstalled
    }
}

/// A video device as sysfs shows it.
#[derive(Debug, Clone)]
struct Device {
    /// "video10"
    node: String,
    /// Its name ("Oculibre", "Dummy video device (0x0000)", a webcam's name…).
    name: String,
    /// Its kernel driver's name ("v4l2loopback", "uvcvideo"…), if any.
    driver: String,
}

fn devices(class: &Path) -> Vec<Device> {
    let mut list: Vec<Device> = fs::read_dir(class)
        .map(|it| it.filter_map(Result::ok).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .map(|entry| Device {
            node: entry.file_name().to_string_lossy().into_owned(),
            name: fs::read_to_string(entry.path().join("name"))
                .unwrap_or_default()
                .trim()
                .to_string(),
            driver: fs::read_link(entry.path().join("device/driver"))
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_default(),
        })
        .collect();
    list.sort_by(|a, b| a.node.cmp(&b.node));
    list
}

/// Ready or Other when a loopback camera exists (same rule as `v4l2::find_loopback`).
fn classify(devices: &[Device]) -> Option<VcamStatus> {
    let dev = |d: &Device| format!("/dev/{}", d.node);
    if let Some(d) = devices.iter().find(|d| d.name.contains("Oculibre")) {
        return Some(VcamStatus::Ready { device: dev(d) });
    }
    devices
        .iter()
        .find(|d| d.driver.contains("v4l2loopback") || d.name.contains("Dummy video device"))
        .map(|d| VcamStatus::Other { device: dev(d) })
}

/// Whether `modprobe v4l2loopback` would find a module for the running kernel.
fn module_available() -> bool {
    ["modinfo", "/usr/sbin/modinfo", "/sbin/modinfo"]
        .iter()
        .any(|program| {
            Command::new(program)
                .args(["-F", "filename", "v4l2loopback"])
                .output()
                .map(|out| out.status.success() && !out.stdout.is_empty())
                .unwrap_or(false)
        })
}

/// Sets the camera up as root, through pkexec (the desktop asks for the password). `helper`:
/// [`HELPER`] once installed; while developing, the script in the source tree.
pub fn fix(helper: &Path) -> Result<(), FixError> {
    let out = Command::new("pkexec")
        .arg(helper)
        .output()
        .map_err(|e| FixError::Failed(format!("cannot run pkexec: {e}")))?;
    let details = String::from_utf8_lossy(&out.stderr).trim().to_string();
    outcome(out.status.code(), details)
}

/// The helper's exit code (see vcam-setup.sh), or pkexec's own: 126 when the password window
/// was closed, 127 when the password was refused or polkit failed.
fn outcome(code: Option<i32>, details: String) -> Result<(), FixError> {
    match code {
        Some(0) => Ok(()),
        Some(2) => Err(FixError::NotInstalled),
        Some(3) => Err(FixError::SecureBoot),
        Some(4) => Err(FixError::InUse),
        Some(126) => Err(FixError::Cancelled),
        Some(127) if details.is_empty() || details.contains("Not authorized") => {
            Err(FixError::Cancelled)
        }
        _ => Err(FixError::Failed(if details.is_empty() {
            format!("vcam-setup ended with {code:?}")
        } else {
            details
        })),
    }
}

/// The helper to run: the installed one, else (development) `fallback` if it exists.
pub fn helper_path(fallback: Option<PathBuf>) -> Option<PathBuf> {
    let installed = PathBuf::from(HELPER);
    if installed.exists() {
        return Some(installed);
    }
    fallback.filter(|p| p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(node: &str, name: &str, driver: &str) -> Device {
        Device {
            node: node.into(),
            name: name.into(),
            driver: driver.into(),
        }
    }

    #[test]
    fn oculibre_camera_first() {
        let list = [
            device("video0", "Microsoft LifeCam HD-3000", "uvcvideo"),
            device("video9", "Dummy video device (0x0000)", "v4l2loopback"),
            device("video10", "Oculibre", "v4l2loopback"),
        ];
        assert_eq!(
            classify(&list),
            Some(VcamStatus::Ready {
                device: "/dev/video10".into()
            })
        );
    }

    #[test]
    fn another_loopback_camera() {
        let list = [
            device("video0", "Microsoft LifeCam HD-3000", "uvcvideo"),
            device("video2", "OBS Virtual Camera", "v4l2loopback"),
        ];
        assert_eq!(
            classify(&list),
            Some(VcamStatus::Other {
                device: "/dev/video2".into()
            })
        );
    }

    #[test]
    fn real_webcams_only() {
        let list = [device("video0", "Microsoft LifeCam HD-3000", "uvcvideo")];
        assert_eq!(classify(&list), None);
    }

    #[test]
    fn helper_exit_codes() {
        assert_eq!(outcome(Some(0), String::new()), Ok(()));
        assert_eq!(outcome(Some(2), String::new()), Err(FixError::NotInstalled));
        assert_eq!(
            outcome(Some(3), "Key was rejected by service".into()),
            Err(FixError::SecureBoot)
        );
        assert_eq!(outcome(Some(4), String::new()), Err(FixError::InUse));
        assert_eq!(outcome(Some(126), String::new()), Err(FixError::Cancelled));
        assert_eq!(
            outcome(
                Some(127),
                "Error executing command as another user: Not authorized".into()
            ),
            Err(FixError::Cancelled)
        );
        assert!(matches!(outcome(Some(1), "boom".into()), Err(FixError::Failed(d)) if d == "boom"));
    }
}
