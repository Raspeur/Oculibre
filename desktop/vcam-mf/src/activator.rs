//! What the Frame Server creates from our CLSID: an `IMFActivate` (with attributes of its own),
//! which makes the media source when asked.

use std::sync::Mutex;

use windows::core::implement;
use windows::Win32::Foundation::E_POINTER;
use windows::Win32::Media::MediaFoundation::*;
use windows_core::{IUnknown, Interface, GUID};

use crate::attributes::delegate_attributes;
use crate::com::guard;
use crate::source::Source;
use crate::CLSID_OCULIBRE_MF;

#[implement(IMFActivate, IMFAttributes)]
pub struct Activator {
    attributes: IMFAttributes,
    source: Mutex<Option<IMFMediaSourceEx>>,
}

delegate_attributes!(Activator_Impl, attributes);

pub fn create() -> windows_core::Result<IUnknown> {
    // SAFETY: Media Foundation constructors and setters.
    unsafe {
        let attributes = {
            let mut a = None;
            MFCreateAttributes(&mut a, 4)?;
            a.ok_or(windows_core::Error::from(E_POINTER))?
        };
        // As Windows' own samples do: our class, and "provide associated camera sources".
        attributes.SetUINT32(&MF_VIRTUALCAMERA_PROVIDE_ASSOCIATED_CAMERA_SOURCES, 1)?;
        attributes.SetGUID(&MFT_TRANSFORM_CLSID_Attribute, &CLSID_OCULIBRE_MF)?;
        Ok(Activator {
            attributes,
            source: Mutex::new(None),
        }
        .into())
    }
}

impl IMFActivate_Impl for Activator_Impl {
    fn ActivateObject(
        &self,
        iid: *const GUID,
        out: *mut *mut core::ffi::c_void,
    ) -> windows_core::Result<()> {
        guard(|| {
            if iid.is_null() || out.is_null() {
                return Err(E_POINTER.into());
            }
            let mut source = self.source.lock().unwrap();
            if source.is_none() {
                *source = Some(Source::create(&self.attributes)?);
            }
            // SAFETY: the caller's iid and pointer.
            unsafe { source.as_ref().unwrap().query(iid, out).ok() }
        })
    }

    fn ShutdownObject(&self) -> windows_core::Result<()> {
        Ok(())
    }

    fn DetachObject(&self) -> windows_core::Result<()> {
        *self.source.lock().unwrap() = None;
        Ok(())
    }
}
