//! Is the Windows virtual camera there, and setting it up when it is not (#17). Same API as the
//! Linux `vcam_setup`, so the desktop app does not change.
//!
//! The camera is the DirectShow DLL (`vcam-dshow`) next to Oculibre's program. [`status`] reads
//! the registry without any privilege: is our COM class registered, and does its file exist?
//! [`fix`] registers the DLL with `regsvr32`, as administrator: Windows asks the user for
//! permission (UAC) with its own window, like polkit on Linux.

use std::path::{Path, PathBuf};

use serde::Serialize;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, WAIT_OBJECT_0};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CLASSES_ROOT, RRF_RT_REG_SZ};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

/// The camera DLL, next to Oculibre's program.
pub const HELPER: &str = "oculibre_vcam_dshow.dll";
/// Its COM class (`vcam-dshow`'s `CLSID_OCULIBRE`).
const CLASS: &str = "CLSID\\{a962be7b-22f0-4df2-9d19-48b6a42e473e}\\InprocServer32";

/// What the virtual camera looks like now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum VcamStatus {
    /// Registered, its file there: apps list "Oculibre".
    Ready { device: String },
    /// Never on Windows (Linux: another app's loopback camera).
    Other { device: String },
    /// The DLL is there but not registered: [`fix`] registers it.
    NotLoaded,
    /// No DLL: Oculibre must be installed again.
    NotInstalled,
    /// Never on Windows (Linux: Secure Boot refuses the module).
    SecureBoot,
}

/// Why [`fix`] did not work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum FixError {
    /// The permission window was closed, or the user said no.
    Cancelled,
    /// No DLL to register.
    NotInstalled,
    /// Never on Windows.
    SecureBoot,
    /// Never on Windows.
    InUse,
    /// Anything else, with the details.
    Failed(String),
}

/// The file our COM class points to, if it is registered.
fn registered() -> Option<PathBuf> {
    let mut buffer = [0u16; 1024];
    let mut size = (buffer.len() * 2) as u32;
    // SAFETY: a registry read into a buffer of known size.
    unsafe {
        RegGetValueW(
            HKEY_CLASSES_ROOT,
            &HSTRING::from(CLASS),
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .ok()
        .ok()?;
    }
    let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    Some(PathBuf::from(String::from_utf16_lossy(&buffer[..len])))
}

pub fn status() -> VcamStatus {
    match registered() {
        Some(dll) if dll.exists() => VcamStatus::Ready {
            device: "Oculibre".into(),
        },
        _ if helper_path(None).is_some() => VcamStatus::NotLoaded,
        _ => VcamStatus::NotInstalled,
    }
}

/// Registers `dll` with `regsvr32`, as administrator (Windows asks the user).
pub fn fix(dll: &Path) -> Result<(), FixError> {
    if !dll.exists() {
        return Err(FixError::NotInstalled);
    }
    let parameters = HSTRING::from(format!("/s \"{}\"", dll.display()));
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: windows::core::w!("runas"),
        lpFile: windows::core::w!("regsvr32.exe"),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    // SAFETY: a filled SHELLEXECUTEINFOW whose strings outlive the call; the process handle is
    // waited for and closed.
    unsafe {
        if let Err(e) = ShellExecuteExW(&mut info) {
            return Err(if e.code() == ERROR_CANCELLED.to_hresult() {
                FixError::Cancelled
            } else {
                FixError::Failed(e.message())
            });
        }
        let mut code = 1u32;
        if WaitForSingleObject(info.hProcess, INFINITE) == WAIT_OBJECT_0 {
            let _ = GetExitCodeProcess(info.hProcess, &mut code);
        }
        let _ = CloseHandle(info.hProcess);
        if code != 0 {
            return Err(FixError::Failed(format!("regsvr32 ended with {code}")));
        }
    }
    match status() {
        VcamStatus::Ready { .. } => Ok(()),
        _ => Err(FixError::Failed("the camera is still not registered".into())),
    }
}

/// The DLL next to Oculibre's program (installed, or `target\…` while developing), else
/// `fallback` if it exists.
pub fn helper_path(fallback: Option<PathBuf>) -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(HELPER)))
        .filter(|p| p.exists())
        .or_else(|| fallback.filter(|p| p.exists()))
}
