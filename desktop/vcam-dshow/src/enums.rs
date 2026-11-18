//! Enumerators the app walks through: our one pin, and the formats we offer.

use std::sync::atomic::{AtomicUsize, Ordering};

use windows::core::implement;
use windows::Win32::Foundation::{E_POINTER, S_FALSE, S_OK};
use windows::Win32::Media::DirectShow::{
    IEnumMediaTypes, IEnumMediaTypes_Impl, IEnumPins, IEnumPins_Impl, IPin,
};
use windows::Win32::Media::MediaFoundation::AM_MEDIA_TYPE;
use windows_core::HRESULT;

use crate::com::guard_hr;
use crate::media::Format;

#[implement(IEnumPins)]
pub struct PinEnum {
    pin: IPin,
    at: AtomicUsize,
}

impl PinEnum {
    pub fn create(pin: IPin) -> IEnumPins {
        Self {
            pin,
            at: AtomicUsize::new(0),
        }
        .into()
    }
}

impl IEnumPins_Impl for PinEnum_Impl {
    fn Next(&self, count: u32, pins: *mut Option<IPin>, fetched: *mut u32) -> HRESULT {
        guard_hr(|| {
            if pins.is_null() {
                return E_POINTER;
            }
            let mut n = 0;
            if count > 0 && self.at.fetch_add(1, Ordering::Relaxed) == 0 {
                // SAFETY: the app gave room for `count` pins.
                unsafe { pins.write(Some(self.pin.clone())) };
                n = 1;
            }
            if !fetched.is_null() {
                // SAFETY: the app's counter.
                unsafe { fetched.write(n) };
            }
            if n == count {
                S_OK
            } else {
                S_FALSE
            }
        })
    }

    fn Skip(&self, count: u32) -> windows_core::Result<()> {
        let before = self.at.fetch_add(count as usize, Ordering::Relaxed);
        if before + count as usize <= 1 {
            Ok(())
        } else {
            Err(S_FALSE.into())
        }
    }

    fn Reset(&self) -> windows_core::Result<()> {
        self.at.store(0, Ordering::Relaxed);
        Ok(())
    }

    fn Clone(&self) -> windows_core::Result<IEnumPins> {
        Ok(PinEnum {
            pin: self.pin.clone(),
            at: AtomicUsize::new(self.at.load(Ordering::Relaxed)),
        }
        .into())
    }
}

#[implement(IEnumMediaTypes)]
pub struct TypeEnum {
    formats: Vec<Format>,
    at: AtomicUsize,
}

impl TypeEnum {
    pub fn create(formats: Vec<Format>) -> IEnumMediaTypes {
        Self {
            formats,
            at: AtomicUsize::new(0),
        }
        .into()
    }
}

impl IEnumMediaTypes_Impl for TypeEnum_Impl {
    fn Next(&self, count: u32, types: *mut *mut AM_MEDIA_TYPE, fetched: *mut u32) -> HRESULT {
        guard_hr(|| {
            if types.is_null() {
                return E_POINTER;
            }
            let mut n = 0u32;
            while n < count {
                let i = self.at.load(Ordering::Relaxed);
                let Some(format) = self.formats.get(i) else {
                    break;
                };
                self.at.store(i + 1, Ordering::Relaxed);
                // SAFETY: the app gave room for `count` pointers; it frees each type.
                unsafe { types.add(n as usize).write(format.new_media_type()) };
                n += 1;
            }
            if !fetched.is_null() {
                // SAFETY: the app's counter.
                unsafe { fetched.write(n) };
            }
            if n == count {
                S_OK
            } else {
                S_FALSE
            }
        })
    }

    fn Skip(&self, count: u32) -> windows_core::Result<()> {
        let at = self.at.load(Ordering::Relaxed) + count as usize;
        self.at.store(at.min(self.formats.len()), Ordering::Relaxed);
        if at <= self.formats.len() {
            Ok(())
        } else {
            Err(S_FALSE.into())
        }
    }

    fn Reset(&self) -> windows_core::Result<()> {
        self.at.store(0, Ordering::Relaxed);
        Ok(())
    }

    fn Clone(&self) -> windows_core::Result<IEnumMediaTypes> {
        Ok(TypeEnum {
            formats: self.formats.clone(),
            at: AtomicUsize::new(self.at.load(Ordering::Relaxed)),
        }
        .into())
    }
}
