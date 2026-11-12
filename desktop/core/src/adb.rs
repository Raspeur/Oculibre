//! USB phones go through `adb` (Android Debug Bridge): `adb reverse` makes the phone's
//! 127.0.0.1:<port> reach this computer over the cable.
//!
//! Android forgets that redirection when the cable is unplugged (or the adb server restarts), so
//! [`watch`] follows the phones plugged in (`adb track-devices`) and redoes it for each new one.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// `adb -s <serial> reverse tcp:<port> tcp:<port>`: one phone, when several are plugged in.
pub fn reverse_device(serial: &str, port: u16) -> Result<(), String> {
    let adb = find_adb();
    let spec = format!("tcp:{port}");
    match Command::new(&adb)
        .args(["-s", serial, "reverse", &spec, &spec])
        .output()
    {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(format!(
            "adb reverse failed ({})",
            String::from_utf8_lossy(&out.stderr).trim()
        )),
        Err(e) => Err(format!("cannot run {} ({e})", adb.display())),
    }
}

/// Follows the phones plugged in; stops when dropped.
pub struct Watch {
    stop: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(child) = self.child.lock().unwrap().as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Redoes `adb reverse` for every phone that appears, ready for USB debugging ("device" state;
/// "unauthorized" waits until the user accepts this computer on the phone). `report` gets each
/// result; it gets one error and the watch ends if adb cannot run at all.
pub fn watch(port: u16, report: impl Fn(Result<(), String>) + Send + 'static) -> Watch {
    let stop = Arc::new(AtomicBool::new(false));
    let child: Arc<Mutex<Option<Child>>> = Arc::default();
    let (stopped, slot) = (stop.clone(), child.clone());
    let _ = thread::Builder::new()
        .name("oculibre-adb".into())
        .spawn(move || {
            let adb = find_adb();
            let mut ready: HashSet<String> = HashSet::new();
            while !stopped.load(Ordering::Relaxed) {
                let mut command = Command::new(&adb);
                command
                    .arg("track-devices")
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null());
                die_with_parent(&mut command);
                let spawned = command.spawn();
                let mut tracker = match spawned {
                    Ok(c) => c,
                    Err(e) => {
                        report(Err(format!("cannot run {} ({e})", adb.display())));
                        return;
                    }
                };
                let Some(mut out) = tracker.stdout.take() else {
                    return;
                };
                *slot.lock().unwrap() = Some(tracker);
                // Each message: 4 hex digits (length), then "serial\tstate\n" lines.
                loop {
                    let mut len = [0u8; 4];
                    if out.read_exact(&mut len).is_err() {
                        break; // the adb server went away (or we were dropped)
                    }
                    let Some(n) = std::str::from_utf8(&len)
                        .ok()
                        .and_then(|h| usize::from_str_radix(h, 16).ok())
                    else {
                        break;
                    };
                    let mut list = vec![0u8; n];
                    if out.read_exact(&mut list).is_err() {
                        break;
                    }
                    let now = ready_devices(&String::from_utf8_lossy(&list));
                    for serial in now.iter().filter(|s| !ready.contains(*s)) {
                        report(reverse_device(serial, port));
                    }
                    ready = now; // an unplugged phone is redone when it comes back
                }
                if let Some(mut c) = slot.lock().unwrap().take() {
                    let _ = c.kill();
                    let _ = c.wait();
                }
                // The adb server restarted, or is starting: try again in a moment.
                thread::sleep(Duration::from_secs(2));
                ready.clear();
            }
        });
    Watch { stop, child }
}

/// Linux: the kernel stops the child when this thread ends, also when the program is killed
/// (then `Drop` does not run): no `adb track-devices` left behind.
fn die_with_parent(command: &mut Command) {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: between fork and exec, only an async-signal-safe system call.
        unsafe {
            command.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                Ok(())
            });
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = command;
}

/// The serials ready for USB debugging in an `adb track-devices` list.
fn ready_devices(list: &str) -> HashSet<String> {
    list.lines()
        .filter_map(|line| {
            let (serial, state) = line.split_once('\t')?;
            (state.trim() == "device").then(|| serial.to_string())
        })
        .collect()
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

#[cfg(test)]
mod tests {
    #[test]
    fn only_authorized_phones_are_ready() {
        let list = "5a9dee96\tdevice\nR5CW\tunauthorized\nemulator-5554\toffline\nX1\tdevice\n";
        let ready = super::ready_devices(list);
        assert_eq!(ready.len(), 2);
        assert!(ready.contains("5a9dee96") && ready.contains("X1"));
        assert!(super::ready_devices("").is_empty());
    }
}
