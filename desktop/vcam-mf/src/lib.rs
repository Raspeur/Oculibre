//! Oculibre's Media Foundation virtual camera (Windows 11, #17): a media source DLL. Oculibre
//! registers the camera with `MFCreateVirtualCamera` while it runs; Windows' Frame Server service
//! then loads this DLL and asks it for pictures, which it reads from the global shared memory
//! Oculibre writes (`oculibre-vcam-shm`). Seen by every app that uses Media Foundation, including
//! the Windows Camera app. Registered with `regsvr32` (as administrator: the Frame Server reads
//! the class from HKEY_LOCAL_MACHINE).
//!
//! - `activator`: what the Frame Server creates from our CLSID (`IMFActivate`)
//! - `source`, `stream`: the media source and its one live NV12 stream
//! - `attributes`: the attribute stores they all are; `com`: helpers

#[cfg(windows)]
mod activator;
#[cfg(windows)]
mod attributes;
#[cfg(windows)]
mod com;
#[cfg(windows)]
mod source;
#[cfg(windows)]
mod stream;

/// The camera's COM class (the source id given to `MFCreateVirtualCamera`):
/// `{97621bb4-1e6f-4389-b549-29381799e692}`.
#[cfg(windows)]
pub const CLSID_OCULIBRE_MF: windows_core::GUID =
    windows_core::GUID::from_u128(0x97621bb4_1e6f_4389_b549_29381799e692);

/// Why [`system_camera`] did not work.
#[cfg(windows)]
#[derive(Debug)]
pub enum CameraError {
    /// Windows 10: no Media Foundation virtual cameras.
    NotSupported,
    Failed(String),
}

/// Registers (or removes) the camera for every user and for good (lifetime "system", access "all
/// users": as administrator). `MFCreateVirtualCamera` is looked up at run time: linking it would
/// stop the tool from even starting on Windows 10.
#[cfg(windows)]
pub fn system_camera(remove: bool) -> Result<(), CameraError> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows_core::{Interface, GUID, HRESULT};
    type Create = unsafe extern "system" fn(
        MFVirtualCameraType,
        MFVirtualCameraLifetime,
        MFVirtualCameraAccess,
        PCWSTR,
        PCWSTR,
        *const GUID,
        u32,
        *mut *mut core::ffi::c_void,
    ) -> HRESULT;
    let failed = |what: &str, e: windows_core::Error| {
        CameraError::Failed(format!("{what}: {}", e.message()))
    };
    // SAFETY: COM and Media Foundation start-up, then MFCreateVirtualCamera found at run time,
    // called with its documented arguments; the camera object is released at the end.
    unsafe {
        let library = LoadLibraryW(&HSTRING::from("mfsensorgroup.dll"))
            .map_err(|_| CameraError::NotSupported)?;
        let function = GetProcAddress(library, windows::core::s!("MFCreateVirtualCamera"))
            .ok_or(CameraError::NotSupported)?;
        let create: Create = std::mem::transmute(function);
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(|e| failed("Media Foundation", e))?;
        let (name, source) = (
            HSTRING::from("Oculibre"),
            HSTRING::from(format!("{{{:?}}}", CLSID_OCULIBRE_MF)),
        );
        let mut raw = std::ptr::null_mut();
        create(
            MFVirtualCameraType_SoftwareCameraSource,
            MFVirtualCameraLifetime_System,
            MFVirtualCameraAccess_AllUsers,
            PCWSTR(name.as_ptr()),
            PCWSTR(source.as_ptr()),
            std::ptr::null(),
            0,
            &mut raw,
        )
        .ok()
        .map_err(|e| failed("MFCreateVirtualCamera", e))?;
        let camera = IMFVirtualCamera::from_raw(raw);
        let result = if remove {
            camera.Remove().map_err(|e| failed("remove", e))
        } else {
            camera.Start(None).map_err(|e| failed("start", e))
        };
        let _ = MFShutdown();
        result
    }
}

#[cfg(windows)]
mod exports {
    use std::ffi::c_void;

    use windows::core::{implement, HSTRING};
    use windows::Win32::Foundation::{
        CLASS_E_CLASSNOTAVAILABLE, CLASS_E_NOAGGREGATION, ERROR_FILE_NOT_FOUND, E_POINTER,
        HINSTANCE, MAX_PATH, S_FALSE,
    };
    use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};
    use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
    use windows::Win32::System::Registry::{
        RegCreateKeyW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_LOCAL_MACHINE, REG_SZ,
    };
    use windows_core::{IUnknown, Interface, Ref, BOOL, GUID, HRESULT, PCWSTR};

    use crate::com::{guard, guard_hr};
    use crate::{activator, CLSID_OCULIBRE_MF};

    static mut MODULE: HINSTANCE = HINSTANCE(std::ptr::null_mut());

    #[no_mangle]
    extern "system" fn DllMain(module: HINSTANCE, reason: u32, _: *mut c_void) -> BOOL {
        if reason == 1 {
            // SAFETY: written once, at load, before any other call.
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
                let object = activator::create()?;
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
                if *clsid != CLSID_OCULIBRE_MF {
                    return CLASS_E_CLASSNOTAVAILABLE;
                }
                let factory: IClassFactory = Factory.into();
                factory.query(iid, out)
            }
        })
    }

    /// Never unloaded (a worker thread may still be finishing).
    #[no_mangle]
    extern "system" fn DllCanUnloadNow() -> HRESULT {
        S_FALSE
    }

    fn class_key() -> String {
        format!("Software\\Classes\\CLSID\\{{{:?}}}", CLSID_OCULIBRE_MF)
    }

    fn set_string(key: HKEY, name: Option<&str>, value: &str) -> windows_core::Result<()> {
        let data: Vec<u16> = value.encode_utf16().chain(Some(0)).collect();
        let name = name.map(HSTRING::from);
        // SAFETY: a UTF-16 string with its terminator, sized in bytes.
        unsafe {
            RegSetValueExW(
                key,
                name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr())),
                None,
                REG_SZ,
                Some(std::slice::from_raw_parts(
                    data.as_ptr().cast(),
                    data.len() * 2,
                )),
            )
            .ok()
        }
    }

    /// HKEY_LOCAL_MACHINE, not the user's classes: the Frame Server only looks there.
    #[no_mangle]
    extern "system" fn DllRegisterServer() -> HRESULT {
        guard_hr(|| {
            let register = || -> windows_core::Result<()> {
                let mut path = [0u16; MAX_PATH as usize * 2];
                // SAFETY: our module handle and a buffer of known size.
                let n = unsafe { GetModuleFileNameW(Some(MODULE.into()), &mut path) } as usize;
                let path = String::from_utf16_lossy(&path[..n]);
                let mut class = HKEY::default();
                let mut server = HKEY::default();
                // SAFETY: plain registry calls.
                unsafe {
                    RegCreateKeyW(HKEY_LOCAL_MACHINE, &HSTRING::from(class_key()), &mut class)
                        .ok()?;
                    set_string(class, None, "Oculibre (Media Foundation)")?;
                    RegCreateKeyW(class, &HSTRING::from("InprocServer32"), &mut server).ok()?;
                }
                set_string(server, None, &path)?;
                set_string(server, Some("ThreadingModel"), "Both")
            };
            register().map_or_else(|e| e.code(), |_| HRESULT(0))
        })
    }

    #[no_mangle]
    extern "system" fn DllUnregisterServer() -> HRESULT {
        guard_hr(|| {
            // SAFETY: plain registry call.
            let result = unsafe { RegDeleteTreeW(HKEY_LOCAL_MACHINE, &HSTRING::from(class_key())) };
            if result.is_err() && result != ERROR_FILE_NOT_FOUND {
                result.to_hresult()
            } else {
                HRESULT(0)
            }
        })
    }
}
