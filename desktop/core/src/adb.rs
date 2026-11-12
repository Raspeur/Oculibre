//! USB phones go through `adb` (Android Debug Bridge): `adb reverse` makes the phone's
//! 127.0.0.1:<port> reach this computer over the cable.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Runs `adb reverse tcp:<port> tcp:<port>`. `Err`: why it failed, in a few words.
pub fn reverse(port: u16) -> Result<(), String> {
    let adb = find_adb();
    let spec = format!("tcp:{port}");
    match Command::new(&adb).args(["reverse", &spec, &spec]).output() {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(format!(
            "adb reverse failed ({})",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(e) => Err(format!("cannot run {} ({e})", adb.display())),
    }
}

/// Opens `uri` on the phone plugged in over USB (the app asks the user to confirm). `false`: no
/// phone plugged in, or adb failed.
pub fn open_uri(uri: &str) -> bool {
    let adb = find_adb();
    let plugged = Command::new(&adb)
        .arg("get-state")
        .output()
        .is_ok_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "device");
    // `adb shell` joins its arguments into one shell command: quote the URI (it has `&`).
    plugged
        && !uri.contains('\'')
        && Command::new(&adb)
            .args([
                "shell",
                "am",
                "start",
                "-a",
                "android.intent.action.VIEW",
                "-d",
            ])
            .arg(format!("'{uri}'"))
            .arg("org.oculibre")
            .output()
            .is_ok_and(|o| o.status.success())
}

/// The Android SDK's adb if there is one, else `adb` from the PATH.
fn find_adb() -> PathBuf {
    let sdk_dirs = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .iter()
        .filter_map(|v| std::env::var_os(v).map(PathBuf::from))
        .chain(std::env::var_os("HOME").map(|h| Path::new(&h).join("Android/Sdk")));
    for sdk in sdk_dirs {
        let adb = sdk.join("platform-tools/adb");
        if adb.is_file() {
            return adb;
        }
    }
    PathBuf::from("adb") // rely on PATH
}
