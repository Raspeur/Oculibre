//! Small helpers for COM code that runs inside other people's apps.

use std::panic::{catch_unwind, AssertUnwindSafe};

use windows::Win32::Foundation::E_UNEXPECTED;
use windows_core::{HRESULT, PWSTR};

/// Runs `f`, turning a panic into `E_UNEXPECTED`: a Rust panic must never unwind into the app
/// (OBS, Zoom…) that called us, it would crash it.
pub fn guard<T>(f: impl FnOnce() -> windows_core::Result<T>) -> windows_core::Result<T> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| Err(E_UNEXPECTED.into()))
}

/// Same, for methods that return a bare `HRESULT`.
pub fn guard_hr(f: impl FnOnce() -> HRESULT) -> HRESULT {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(E_UNEXPECTED)
}

/// A COM interface moved to another thread. DirectShow's objects are free-threaded (our DLL is
/// registered `ThreadingModel=Both`, the system's allocator and pins too).
pub struct Sendable<T>(pub T);
// SAFETY: see above.
unsafe impl<T> Send for Sendable<T> {}
unsafe impl<T> Sync for Sendable<T> {}

/// "Oculibre" and friends as UTF-16 with a final 0.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Copies `s` into a fixed UTF-16 array (pin and filter names), cut if needed, 0-terminated.
pub fn copy_name(s: &[u16], out: &mut [u16]) {
    let n = s.iter().take_while(|&&c| c != 0).count().min(out.len() - 1);
    out[..n].copy_from_slice(&s[..n]);
    out[n] = 0;
}

/// A string the caller frees with `CoTaskMemFree` (QueryId).
pub fn co_string(s: &str) -> windows_core::Result<PWSTR> {
    let w = wide(s);
    // SAFETY: a block of the right size, filled at once.
    unsafe {
        let p = windows::Win32::System::Com::CoTaskMemAlloc(w.len() * 2) as *mut u16;
        if p.is_null() {
            return Err(windows::Win32::Foundation::E_OUTOFMEMORY.into());
        }
        p.copy_from_nonoverlapping(w.as_ptr(), w.len());
        Ok(PWSTR(p))
    }
}
