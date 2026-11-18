//! Oculibre's DirectShow virtual camera (Windows 10 and 11, #17): a COM DLL that apps load
//! (OBS, Zoom, Discord, Chrome…) when they open the camera "Oculibre". It reads the picture
//! Oculibre writes into shared memory (`oculibre-vcam-shm`) and gives it to the app; without
//! Oculibre it shows a "waiting" picture, so that apps keep the camera.
//!
//! Registered with `regsvr32 oculibre_vcam_dshow.dll` (as administrator), removed with
//! `regsvr32 /u`. A 32-bit build serves 32-bit apps.
//!
//! - `filter`: the filter object (hand-written COM, see why there)
//! - `pin`: the output pin, formats, and the thread that sends pictures
//! - `enums`, `media`, `com`: enumerators, DirectShow formats, helpers
//! - `picture`: resizing and the waiting picture (plain Rust)

pub mod picture;

#[cfg(windows)]
mod com;
#[cfg(windows)]
mod enums;
#[cfg(windows)]
mod filter;
#[cfg(windows)]
mod media;
#[cfg(windows)]
mod pin;
#[cfg(windows)]
mod register;

/// The camera's COM class: `{a962be7b-22f0-4df2-9d19-48b6a42e473e}`.
#[cfg(windows)]
pub const CLSID_OCULIBRE: windows_core::GUID =
    windows_core::GUID::from_u128(0xa962be7b_22f0_4df2_9d19_48b6a42e473e);

#[cfg(windows)]
mod exports {
    use std::ffi::c_void;

    use windows::core::implement;
    use windows::Win32::Foundation::{
        CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, E_POINTER, HINSTANCE, S_FALSE,
    };
    use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
    use windows_core::{IUnknown, Interface, Ref, BOOL, GUID, HRESULT};

    use crate::com::{guard, guard_hr};
    use crate::{filter, register, CLSID_OCULIBRE};

    /// This DLL, for its path in the registry.
    pub static mut MODULE: HINSTANCE = HINSTANCE(std::ptr::null_mut());

    #[no_mangle]
    extern "system" fn DllMain(module: HINSTANCE, reason: u32, _: *mut c_void) -> BOOL {
        const DLL_PROCESS_ATTACH: u32 = 1;
        if reason == DLL_PROCESS_ATTACH {
            // SAFETY: written once, before any other call into the DLL.
            unsafe { MODULE = module };
        }
        true.into()
    }

    #[implement(IClassFactory)]
    struct Factory;

    impl IClassFactory_Impl for Factory_Impl {
        fn CreateInstance(
            &self,
            outer: Ref<IUnknown>,
            iid: *const GUID,
            out: *mut *mut c_void,
        ) -> windows_core::Result<()> {
            guard(|| {
                if out.is_null() || iid.is_null() {
                    return Err(E_POINTER.into());
                }
                if outer.is_some() {
                    return Err(CLASS_E_NOAGGREGATION.into());
                }
                let object = filter::create();
                // SAFETY: the caller's iid and pointer.
                unsafe { object.query(iid, out).ok() }
            })
        }

        fn LockServer(&self, _: BOOL) -> windows_core::Result<()> {
            Ok(())
        }
    }

    #[no_mangle]
    extern "system" fn DllGetClassObject(
        clsid: *const GUID,
        iid: *const GUID,
        out: *mut *mut c_void,
    ) -> HRESULT {
        guard_hr(|| {
            if clsid.is_null() || iid.is_null() || out.is_null() {
                return E_POINTER;
            }
            // SAFETY: the caller's pointers, checked.
            unsafe {
                out.write(std::ptr::null_mut());
                if *clsid != CLSID_OCULIBRE {
                    return CLASS_E_CLASSNOTAVAILABLE;
                }
                let factory: IClassFactory = Factory.into();
                factory.query(iid, out)
            }
        })
    }

    /// Never unloaded: a sending thread may still be finishing, and it costs nothing to stay.
    #[no_mangle]
    extern "system" fn DllCanUnloadNow() -> HRESULT {
        S_FALSE
    }

    #[no_mangle]
    extern "system" fn DllRegisterServer() -> HRESULT {
        guard_hr(|| register::register().map_or_else(|e| e.code(), |_| HRESULT(0)))
    }

    #[no_mangle]
    extern "system" fn DllUnregisterServer() -> HRESULT {
        guard_hr(|| register::unregister().map_or_else(|e| e.code(), |_| HRESULT(0)))
    }
}
