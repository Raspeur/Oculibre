//! `regsvr32`: makes Windows know our COM class (HKEY_CLASSES_ROOT\CLSID\{…}) and lists it among
//! the video capture devices (the category apps enumerate), as "Oculibre".

use windows::core::HSTRING;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, MAX_PATH};
use windows::Win32::Media::DirectShow::{
    IFilterMapper2, MERIT_DO_NOT_USE, REGFILTER2, REGFILTER2_0, REGFILTER2_0_0, REGFILTERPINS,
    REGPINTYPES,
};
use windows::Win32::Media::MediaFoundation::{
    CLSID_FilterMapper2, CLSID_VideoInputDeviceCategory, MEDIATYPE_Video, MEDIASUBTYPE_YUY2,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::Registry::{
    RegCreateKeyW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_CLASSES_ROOT, REG_SZ,
};
use windows_core::{PCWSTR, PWSTR};

use crate::com::wide;
use crate::exports::MODULE;
use crate::CLSID_OCULIBRE;

const NAME: &str = "Oculibre";

fn class_key() -> String {
    format!("CLSID\\{{{:?}}}", CLSID_OCULIBRE)
}

fn set_string(key: HKEY, name: Option<&str>, value: &str) -> windows_core::Result<()> {
    let data = wide(value);
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

fn create_key(parent: HKEY, path: &str) -> windows_core::Result<HKEY> {
    let mut key = HKEY::default();
    // SAFETY: plain registry call.
    unsafe { RegCreateKeyW(parent, &HSTRING::from(path), &mut key).ok()? };
    Ok(key)
}

pub fn register() -> windows_core::Result<()> {
    let mut path = [0u16; MAX_PATH as usize * 2];
    // SAFETY: our module handle (set in DllMain) and a buffer of known size.
    let n = unsafe { GetModuleFileNameW(Some(MODULE.into()), &mut path) } as usize;
    let path = String::from_utf16_lossy(&path[..n]);

    let class = create_key(HKEY_CLASSES_ROOT, &class_key())?;
    set_string(class, None, NAME)?;
    let server = create_key(class, "InprocServer32")?;
    set_string(server, None, &path)?;
    // Apps call us from any thread; our objects lock what they share.
    set_string(server, Some("ThreadingModel"), "Both")?;

    // SAFETY: COM for this call (regsvr32's thread); then plain COM calls with structures that
    // live on our stack for the duration of the call.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let mapper: IFilterMapper2 =
            CoCreateInstance(&CLSID_FilterMapper2, None, CLSCTX_INPROC_SERVER)?;
        let types = [REGPINTYPES {
            clsMajorType: &MEDIATYPE_Video,
            clsMinorType: &MEDIASUBTYPE_YUY2,
        }];
        let mut pin_name = wide("Capture");
        let pins = [REGFILTERPINS {
            strName: PWSTR(pin_name.as_mut_ptr()),
            bRendered: false.into(),
            bOutput: true.into(),
            bZero: false.into(),
            bMany: false.into(),
            clsConnectsToFilter: std::ptr::null(),
            strConnectsToPin: PCWSTR::null(),
            nMediaTypes: types.len() as u32,
            lpMediaType: types.as_ptr(),
        }];
        let filter = REGFILTER2 {
            dwVersion: 1,
            dwMerit: MERIT_DO_NOT_USE.0 as u32,
            Anonymous: REGFILTER2_0 {
                Anonymous1: REGFILTER2_0_0 {
                    cPins: pins.len() as u32,
                    rgPins: pins.as_ptr(),
                },
            },
        };
        let name = HSTRING::from(NAME);
        mapper.RegisterFilter(
            &CLSID_OCULIBRE,
            &name,
            None,
            &CLSID_VideoInputDeviceCategory,
            &name,
            &filter,
        )?;
    }
    Ok(())
}

pub fn unregister() -> windows_core::Result<()> {
    // SAFETY: as in `register`.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        if let Ok(mapper) =
            CoCreateInstance::<_, IFilterMapper2>(&CLSID_FilterMapper2, None, CLSCTX_INPROC_SERVER)
        {
            let _ = mapper.UnregisterFilter(
                &CLSID_VideoInputDeviceCategory,
                &HSTRING::from(NAME),
                &CLSID_OCULIBRE,
            );
        }
        let result = RegDeleteTreeW(HKEY_CLASSES_ROOT, &HSTRING::from(class_key()));
        if result.is_err() && result != ERROR_FILE_NOT_FOUND {
            return result.ok();
        }
    }
    Ok(())
}
