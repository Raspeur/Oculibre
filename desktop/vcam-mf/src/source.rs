//! The media source the Frame Server runs (through the activator): one live video stream. It
//! makes the global shared memory Oculibre writes into (`Shared::create_global`).

use std::sync::{Arc, Mutex};

use oculibre_vcam_shm::Shared;
use windows::core::implement;
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, E_POINTER, S_OK};
use windows::Win32::Media::KernelStreaming::{IKsControl, IKsControl_Impl, KSIDENTIFIER};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Variant::VT_I8;
use windows_core::{ComObject, IUnknown, IUnknownImpl, Interface, Ref, GUID};

use crate::attributes::delegate_attributes;
use crate::com::{guard, ks_not_found};
use crate::stream::Stream;

#[implement(
    IMFMediaSourceEx,
    IMFGetService,
    IKsControl,
    IMFSampleAllocatorControl,
    IMFAttributes
)]
pub struct Source {
    attributes: IMFAttributes,
    queue: IMFMediaEventQueue,
    descriptor: IMFPresentationDescriptor,
    stream: ComObject<Stream>,
    /// Kept for the source's life: the global block exists while an app uses the camera.
    _shared: Arc<Option<Shared>>,
    shut: Mutex<bool>,
}

delegate_attributes!(Source_Impl, attributes);

impl Source {
    /// A source whose attributes start as a copy of the activator's.
    pub fn create(from: &IMFAttributes) -> windows_core::Result<IMFMediaSourceEx> {
        // SAFETY: Media Foundation constructors and setters.
        unsafe {
            let attributes = {
                let mut a = None;
                MFCreateAttributes(&mut a, 8)?;
                a.ok_or(windows_core::Error::from(E_POINTER))?
            };
            from.CopyAllItems(&attributes)?;
            let shared = Arc::new(Shared::create_global().ok());
            let stream = ComObject::new(Stream::new(shared.clone())?);
            let descriptor =
                MFCreatePresentationDescriptor(Some(&[Some(stream.descriptor().clone())]))?;
            descriptor.SelectStream(0)?;
            let source = ComObject::new(Source {
                attributes,
                queue: MFCreateEventQueue()?,
                descriptor,
                stream,
                _shared: shared,
                shut: Mutex::new(false),
            });
            let ex: IMFMediaSourceEx = source.to_interface();
            source.stream.set_source(Some(ex.cast::<IMFMediaSource>()?));
            Ok(source.to_interface())
        }
    }

    fn check(&self) -> windows_core::Result<()> {
        if *self.shut.lock().unwrap() {
            Err(MF_E_SHUTDOWN.into())
        } else {
            Ok(())
        }
    }
}

/// `VT_I8` PROPVARIANT (the start and stop times of the events).
fn time_variant(t: i64) -> PROPVARIANT {
    let mut v = PROPVARIANT::default();
    // SAFETY: setting the type and its matching union field.
    unsafe {
        let inner = &mut v.Anonymous.Anonymous;
        inner.vt = VT_I8;
        inner.Anonymous.hVal = t;
    }
    v
}

impl IMFMediaEventGenerator_Impl for Source_Impl {
    fn GetEvent(
        &self,
        flags: MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS,
    ) -> windows_core::Result<IMFMediaEvent> {
        // SAFETY: our queue.
        unsafe { self.queue.GetEvent(flags.0) }
    }
    fn BeginGetEvent(
        &self,
        callback: Ref<IMFAsyncCallback>,
        state: Ref<IUnknown>,
    ) -> windows_core::Result<()> {
        // SAFETY: our queue.
        unsafe { self.queue.BeginGetEvent(callback.as_ref(), state.as_ref()) }
    }
    fn EndGetEvent(&self, result: Ref<IMFAsyncResult>) -> windows_core::Result<IMFMediaEvent> {
        // SAFETY: our queue.
        unsafe { self.queue.EndGetEvent(result.as_ref()) }
    }
    fn QueueEvent(
        &self,
        met: u32,
        extended: *const GUID,
        status: windows_core::HRESULT,
        value: *const PROPVARIANT,
    ) -> windows_core::Result<()> {
        // SAFETY: our queue.
        unsafe { self.queue.QueueEventParamVar(met, extended, status, value) }
    }
}

impl IMFMediaSource_Impl for Source_Impl {
    fn GetCharacteristics(&self) -> windows_core::Result<u32> {
        self.check()?;
        Ok(MFMEDIASOURCE_IS_LIVE.0 as u32)
    }

    fn CreatePresentationDescriptor(&self) -> windows_core::Result<IMFPresentationDescriptor> {
        self.check()?;
        // SAFETY: our descriptor.
        unsafe { self.descriptor.Clone() }
    }

    /// Order the Frame Server expects: MENewStream (source), MEStreamStarted (stream),
    /// MESourceStarted (source, with the start time).
    fn Start(
        &self,
        pd: Ref<IMFPresentationDescriptor>,
        time_format: *const GUID,
        start: *const PROPVARIANT,
    ) -> windows_core::Result<()> {
        guard(|| {
            self.check()?;
            // SAFETY: the caller's pointers, checked.
            unsafe {
                if start.is_null() || time_format.as_ref().is_some_and(|g| *g != GUID::zeroed()) {
                    return Err(E_INVALIDARG.into());
                }
                let pd = pd.ok()?;
                let mut selected = windows_core::BOOL::default();
                let mut sd = None;
                pd.GetStreamDescriptorByIndex(0, &mut selected, &mut sd)?;
                let sd = sd.ok_or(windows_core::Error::from(E_FAIL))?;
                // The media type the Frame Server chose goes to our stream's descriptor.
                if let Ok(t) = sd
                    .GetMediaTypeHandler()
                    .and_then(|h| h.GetCurrentMediaType())
                {
                    let _ = self
                        .stream
                        .descriptor()
                        .GetMediaTypeHandler()?
                        .SetCurrentMediaType(&t);
                }
                if selected.as_bool() {
                    self.descriptor.SelectStream(0)?;
                    let stream: IUnknown = self.stream.to_interface();
                    self.queue.QueueEventParamUnk(
                        MENewStream.0 as u32,
                        &GUID::zeroed(),
                        S_OK,
                        &stream,
                    )?;
                    self.stream.start()?;
                } else {
                    self.descriptor.DeselectStream(0)?;
                    self.stream.stop()?;
                }
                let now = time_variant(MFGetSystemTime());
                self.queue
                    .QueueEventParamVar(MESourceStarted.0 as u32, &GUID::zeroed(), S_OK, &now)
            }
        })
    }

    fn Stop(&self) -> windows_core::Result<()> {
        guard(|| {
            self.check()?;
            self.stream.stop()?;
            // SAFETY: our descriptor and queue.
            unsafe {
                self.descriptor.DeselectStream(0)?;
                let now = time_variant(MFGetSystemTime());
                self.queue
                    .QueueEventParamVar(MESourceStopped.0 as u32, &GUID::zeroed(), S_OK, &now)
            }
        })
    }

    fn Pause(&self) -> windows_core::Result<()> {
        Err(MF_E_INVALID_STATE_TRANSITION.into())
    }

    fn Shutdown(&self) -> windows_core::Result<()> {
        guard(|| {
            let mut shut = self.shut.lock().unwrap();
            if *shut {
                return Ok(());
            }
            *shut = true;
            self.stream.shutdown();
            // SAFETY: our queue.
            unsafe { self.queue.Shutdown() }
        })
    }
}

impl IMFMediaSourceEx_Impl for Source_Impl {
    fn GetSourceAttributes(&self) -> windows_core::Result<IMFAttributes> {
        self.check()?;
        Ok(self.to_interface())
    }

    fn GetStreamAttributes(&self, id: u32) -> windows_core::Result<IMFAttributes> {
        self.check()?;
        if id != 0 {
            return Err(E_FAIL.into());
        }
        Ok(self.stream.to_interface())
    }

    /// No Direct3D: our pictures are made in memory (the allocator then gives memory samples).
    fn SetD3DManager(&self, _: Ref<IUnknown>) -> windows_core::Result<()> {
        Ok(())
    }
}

impl IMFGetService_Impl for Source_Impl {
    fn GetService(
        &self,
        _: *const GUID,
        _: *const GUID,
        out: *mut *mut core::ffi::c_void,
    ) -> windows_core::Result<()> {
        if !out.is_null() {
            // SAFETY: the caller's pointer.
            unsafe { out.write(std::ptr::null_mut()) };
        }
        Err(MF_E_UNSUPPORTED_SERVICE.into())
    }
}

impl IMFSampleAllocatorControl_Impl for Source_Impl {
    fn SetDefaultAllocator(
        &self,
        stream: u32,
        allocator: Ref<IUnknown>,
    ) -> windows_core::Result<()> {
        if stream != 0 {
            return Err(E_INVALIDARG.into());
        }
        let allocator = allocator
            .as_ref()
            .and_then(|a| a.cast::<IMFVideoSampleAllocator>().ok());
        self.stream.set_allocator(allocator);
        Ok(())
    }

    fn GetAllocatorUsage(
        &self,
        stream: u32,
        input: *mut u32,
        usage: *mut MFSampleAllocatorUsage,
    ) -> windows_core::Result<()> {
        if stream != 0 {
            return Err(E_INVALIDARG.into());
        }
        if input.is_null() || usage.is_null() {
            return Err(E_POINTER.into());
        }
        // SAFETY: the caller's pointers.
        unsafe {
            input.write(0);
            usage.write(MFSampleAllocatorUsage_UsesProvidedAllocator);
        }
        Ok(())
    }
}

impl IKsControl_Impl for Source_Impl {
    fn KsProperty(
        &self,
        _: *const KSIDENTIFIER,
        _: u32,
        _: *mut core::ffi::c_void,
        _: u32,
        _: *mut u32,
    ) -> windows_core::Result<()> {
        Err(ks_not_found())
    }
    fn KsMethod(
        &self,
        _: *const KSIDENTIFIER,
        _: u32,
        _: *mut core::ffi::c_void,
        _: u32,
        _: *mut u32,
    ) -> windows_core::Result<()> {
        Err(ks_not_found())
    }
    fn KsEvent(
        &self,
        _: *const KSIDENTIFIER,
        _: u32,
        _: *mut core::ffi::c_void,
        _: u32,
        _: *mut u32,
    ) -> windows_core::Result<()> {
        Err(ks_not_found())
    }
}
