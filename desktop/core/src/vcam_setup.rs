//! Is the virtual camera there, and setting it up when it is not (Linux, #18).
//!
//! [`status`] looks without any privilege (sysfs, `modinfo`, the EFI variables of Secure Boot). [`fix`] runs the root helper
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
    /// The module was built and signed on this computer (DKMS), Secure Boot is on and that key is
    /// not enrolled yet: loading it would be refused, the user has to enrol the key.
    SecureBoot,
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
    match module_signer() {
        None => VcamStatus::NotInstalled,
        Some(signer) if secure_boot_refuses(&signer) => VcamStatus::SecureBoot,
        Some(_) => VcamStatus::NotLoaded,
    }
}

/// EFI variables (efivarfs, readable by every user): 4 bytes of attributes, then the value.
const EFIVARS: &str = "/sys/firmware/efi/efivars";
const SECURE_BOOT_VAR: &str = "SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c";
/// The keys enrolled through shim (Machine Owner Keys), as the running system sees them.
const MOK_LIST_VAR: &str = "MokListRT-605dab50-e046-4300-abb6-3dd810dd8b23";
/// Where DKMS keeps the certificate it signs modules with (DER): Debian, then Ubuntu.
const DKMS_KEYS: [&str; 2] = ["/var/lib/dkms/mok.pub", "/var/lib/shim-signed/mok/MOK.der"];

/// Whether Secure Boot would refuse a module signed by `signer` (`modinfo -F signer`): signed
/// by this computer's DKMS key, and that key not enrolled. Signed by the distribution (Ubuntu's
/// own modules): never.
fn secure_boot_refuses(signer: &str) -> bool {
    let read = |name: &str| fs::read(Path::new(EFIVARS).join(name)).ok();
    if !secure_boot_on(read(SECURE_BOOT_VAR).as_deref()) {
        return false;
    }
    let enrolled = read(MOK_LIST_VAR).unwrap_or_default();
    DKMS_KEYS
        .iter()
        .filter_map(|path| fs::read(path).ok())
        .any(|cert| signed_by(signer, &cert) && !contains(&enrolled, &cert))
}

fn secure_boot_on(var: Option<&[u8]>) -> bool {
    var.and_then(|v| v.get(4)) == Some(&1)
}

/// Whether the module's signer is this certificate: sign-file names the signer by the
/// certificate's common name, which a self-signed DER certificate holds as plain bytes.
fn signed_by(signer: &str, cert: &[u8]) -> bool {
    !signer.is_empty() && contains(cert, signer.as_bytes())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
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

/// The signer of the module `modprobe v4l2loopback` would load ("" when unsigned), `None`
/// when there is no module for the running kernel. `modinfo` is in /usr/sbin, which is not in
/// a user's PATH on Debian.
fn module_signer() -> Option<String> {
    ["modinfo", "/usr/sbin/modinfo", "/sbin/modinfo"]
        .iter()
        .find_map(|program| {
            let out = Command::new(program).arg("v4l2loopback").output().ok()?;
            out.status.success().then_some(())?;
            fields(&String::from_utf8_lossy(&out.stdout))
        })
}

/// `filename` must be there (the module exists); `signer` is missing on an unsigned module.
fn fields(modinfo: &str) -> Option<String> {
    let field = |name: &str| {
        modinfo.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key == name).then(|| value.trim().to_string())
        })
    };
    field("filename").filter(|f| !f.is_empty())?;
    Some(field("signer").unwrap_or_default())
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
    fn modinfo_fields() {
        let signed =
            "filename:       /lib/modules/7.0.0-43-generic/ubuntu/dkms/v4l2loopback.ko.zst\n\
                      license:        GPL\n\
                      signer:         Canonical Ltd. Kernel Module Signing\n\
                      sig_key:        E9:DF:13:0F\n";
        assert_eq!(
            fields(signed).as_deref(),
            Some("Canonical Ltd. Kernel Module Signing")
        );
        assert_eq!(
            fields("filename: /lib/modules/x/v4l2loopback.ko\n").as_deref(),
            Some("")
        );
        assert_eq!(fields(""), None);
    }

    #[test]
    fn secure_boot_variable() {
        assert!(secure_boot_on(Some(&[6, 0, 0, 0, 1])));
        assert!(!secure_boot_on(Some(&[6, 0, 0, 0, 0])));
        assert!(!secure_boot_on(None)); // BIOS, or no EFI variables
    }

    #[test]
    fn module_signed_by_the_local_key() {
        // A self-signed certificate holds its common name as plain bytes.
        let cert = b"\x30\x82...\x06\x03\x55\x04\x03\x0c\x17DKMS module signing key...";
        assert!(signed_by("DKMS module signing key", cert));
        assert!(!signed_by("Canonical Ltd. Kernel Module Signing", cert));
        assert!(!signed_by("", cert)); // unsigned module
        let enrolled = [b"list:".as_slice(), cert, b":end"].concat();
        assert!(contains(&enrolled, cert));
        assert!(!contains(b"list:other keys:end", cert));
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
