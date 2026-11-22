//! The computer's power state, and asking Windows not to slow Oculibre down.
//!
//! Oculibre runs in the background most of the time (window hidden, tray icon), and that is
//! exactly what power saving throttles first: Windows 11 puts background processes on its
//! efficient cores at a low frequency (EcoQoS). The video then falls behind, seconds behind with
//! the old pipeline. Video and call apps opt out the same way.

/// Asks the system to run Oculibre at full speed even in the background (Windows: no EcoQoS, no
/// coarse timers). Nothing to do elsewhere. Call once at start.
pub fn keep_full_speed() {
    #[cfg(windows)]
    windows_power::keep_full_speed();
}

/// The computer runs on its battery (a laptop unplugged). `false` when unknown or without battery.
pub fn on_battery() -> bool {
    #[cfg(windows)]
    return windows_power::on_battery();
    #[cfg(target_os = "linux")]
    return linux_on_battery(std::path::Path::new("/sys/class/power_supply"));
    #[allow(unreachable_code)]
    false
}

/// A battery that discharges, among the power supplies the kernel lists (`type` Battery,
/// `status` Discharging). A desktop has none; a plugged laptop says Charging, Full or Not charging.
#[cfg(any(target_os = "linux", test))]
fn linux_on_battery(supplies: &std::path::Path) -> bool {
    let Ok(entries) = std::fs::read_dir(supplies) else {
        return false;
    };
    let read = |dir: &std::path::Path, name: &str| {
        std::fs::read_to_string(dir.join(name))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    entries.flatten().any(|e| {
        let dir = e.path();
        read(&dir, "type") == "Battery" && read(&dir, "status") == "Discharging"
    })
}

#[cfg(windows)]
mod windows_power {
    use std::ffi::c_void;

    use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
    use windows::Win32::System::Threading::{
        GetCurrentProcess, ProcessPowerThrottling, SetProcessInformation,
        PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION, PROCESS_POWER_THROTTLING_STATE,
    };

    pub fn keep_full_speed() {
        // ControlMask: what we decide ourselves; StateMask 0: throttling off for those.
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED
                | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
            StateMask: 0,
        };
        // SAFETY: a valid structure of the size given, for our own process.
        let _ = unsafe {
            SetProcessInformation(
                GetCurrentProcess(),
                ProcessPowerThrottling,
                &state as *const _ as *const c_void,
                std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
        };
    }

    pub fn on_battery() -> bool {
        let mut status = SYSTEM_POWER_STATUS::default();
        // SAFETY: fills the structure we own. ACLineStatus: 0 offline, 1 online, 255 unknown.
        unsafe { GetSystemPowerStatus(&mut status) }.is_ok() && status.ACLineStatus == 0
    }
}

#[cfg(test)]
mod tests {
    use super::linux_on_battery;

    #[test]
    fn only_a_discharging_battery_counts() {
        let dir = std::env::temp_dir().join(format!("oculibre-power-{}", std::process::id()));
        let supply = |name: &str, kind: &str, status: Option<&str>| {
            let d = dir.join(name);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("type"), format!("{kind}\n")).unwrap();
            if let Some(s) = status {
                std::fs::write(d.join("status"), format!("{s}\n")).unwrap();
            }
        };
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!linux_on_battery(&dir), "no such folder: a desktop");
        supply("AC", "Mains", None);
        supply("BAT0", "Battery", Some("Charging"));
        assert!(!linux_on_battery(&dir), "plugged in");
        supply("BAT0", "Battery", Some("Discharging"));
        assert!(linux_on_battery(&dir), "unplugged");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
