//! Helpers for COM code that runs inside a Windows service (the Frame Server).

use std::panic::{catch_unwind, AssertUnwindSafe};

use windows::Win32::Foundation::{ERROR_SET_NOT_FOUND, E_UNEXPECTED};
use windows_core::HRESULT;

/// Runs `f`, turning a panic into `E_UNEXPECTED`: a panic must never unwind into the service.
pub fn guard<T>(f: impl FnOnce() -> windows_core::Result<T>) -> windows_core::Result<T> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| Err(E_UNEXPECTED.into()))
}

pub fn guard_hr(f: impl FnOnce() -> HRESULT) -> HRESULT {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(E_UNEXPECTED)
}

/// A COM interface moved to another thread (Media Foundation objects are free-threaded).
pub struct Sendable<T>(pub T);
// SAFETY: see above.
unsafe impl<T> Send for Sendable<T> {}
unsafe impl<T> Sync for Sendable<T> {}

/// What our `IKsControl` answers: no camera controls (as Windows' own samples do).
pub fn ks_not_found() -> windows_core::Error {
    ERROR_SET_NOT_FOUND.to_hresult().into()
}
