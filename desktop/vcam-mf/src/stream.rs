//! The camera's one video stream: the Frame Server asks for pictures (`RequestSample`), a worker
//! thread answers each request with the latest picture from Oculibre, NV12, as soon as a new one
//! comes, or the last one again after a frame interval (the waiting picture without Oculibre).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use oculibre_vcam_shm::{picture, Shared};
use windows::core::implement;
use windows::Win32::Foundation::{E_POINTER, S_OK};
use windows::Win32::Media::KernelStreaming::{IKsControl, IKsControl_Impl, KSIDENTIFIER};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows_core::{IUnknown, Interface, Ref, GUID};

use crate::attributes::delegate_attributes;
use crate::com::{guard, ks_not_found, Sendable};

/// After this long without a new picture from Oculibre, show the "waiting" picture.
const STALE: Duration = Duration::from_secs(2);

/// What the stream sends: NV12 at this size and pace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    pub width: usize,
    pub height: usize,
    pub fps: u32,
}

impl Format {
    /// The formats offered, best first (same sizes as the DirectShow camera).
    pub const ALL: [Format; 5] = [
        Format {
            width: 1920,
            height: 1080,
            fps: 30,
        },
        Format {
            width: 1920,
            height: 1080,
            fps: 60,
        },
        Format {
            width: 1280,
            height: 720,
            fps: 30,
        },
        Format {
            width: 1280,
            height: 720,
            fps: 60,
        },
        Format {
            width: 640,
            height: 360,
            fps: 30,
        },
    ];

    pub fn media_type(&self) -> windows_core::Result<IMFMediaType> {
        // SAFETY: plain attribute setters on a new media type.
        unsafe {
            let t = MFCreateMediaType()?;
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
            t.SetUINT64(
                &MF_MT_FRAME_SIZE,
                ((self.width as u64) << 32) | self.height as u64,
            )?;
            t.SetUINT64(&MF_MT_FRAME_RATE, ((self.fps as u64) << 32) | 1)?;
            t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1)?;
            t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            t.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
            t.SetUINT32(&MF_MT_DEFAULT_STRIDE, self.width as u32)?;
            t.SetUINT32(
                &MF_MT_AVG_BITRATE,
                (self.width * self.height * 3 / 2 * 8) as u32 * self.fps,
            )?;
            Ok(t)
        }
    }

    /// Reads a media type back (the one the Frame Server chose).
    pub fn of(t: &IMFMediaType) -> Option<Format> {
        // SAFETY: plain attribute getters.
        unsafe {
            let size = t.GetUINT64(&MF_MT_FRAME_SIZE).ok()?;
            let rate = t.GetUINT64(&MF_MT_FRAME_RATE).ok()?;
            let (num, den) = ((rate >> 32) as u32, (rate & 0xffff_ffff) as u32);
            Some(Format {
                width: (size >> 32) as usize,
                height: (size & 0xffff_ffff) as usize,
                fps: num.checked_div(den).map_or(30, |fps| fps.max(1)),
            })
        }
    }

    fn interval(&self) -> Duration {
        Duration::from_nanos(1_000_000_000 / u64::from(self.fps))
    }
}

struct Worker {
    requests: Sender<Option<Sendable<IUnknown>>>,
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

struct Inner {
    state: MF_STREAM_STATE,
    allocator: Option<IMFVideoSampleAllocator>,
    worker: Option<Worker>,
    source: Option<IMFMediaSource>,
    shut: bool,
}

#[implement(IMFMediaStream2, IKsControl, IMFAttributes)]
pub struct Stream {
    attributes: IMFAttributes,
    queue: IMFMediaEventQueue,
    descriptor: IMFStreamDescriptor,
    /// The picture Oculibre writes (made by the source; `None`: it could not be made).
    shared: Arc<Option<Shared>>,
    inner: Mutex<Inner>,
}

delegate_attributes!(Stream_Impl, attributes);

impl Stream {
    pub fn new(shared: Arc<Option<Shared>>) -> windows_core::Result<Self> {
        // SAFETY: Media Foundation constructors and setters.
        unsafe {
            let attributes = {
                let mut a = None;
                MFCreateAttributes(&mut a, 8)?;
                a.ok_or(windows_core::Error::from(E_POINTER))?
            };
            // What the Frame Server looks for: a shared color capture stream, number 0.
            attributes.SetGUID(
                &MF_DEVICESTREAM_STREAM_CATEGORY,
                &windows::Win32::Media::KernelStreaming::PINNAME_VIDEO_CAPTURE,
            )?;
            attributes.SetUINT32(&MF_DEVICESTREAM_STREAM_ID, 0)?;
            attributes.SetUINT32(&MF_DEVICESTREAM_FRAMESERVER_SHARED, 1)?;
            attributes.SetUINT32(
                &MF_DEVICESTREAM_ATTRIBUTE_FRAMESOURCE_TYPES,
                MFFrameSourceTypes_Color.0 as u32,
            )?;
            let types: Vec<Option<IMFMediaType>> = Format::ALL
                .iter()
                .map(|f| f.media_type().map(Some))
                .collect::<windows_core::Result<_>>()?;
            let descriptor = MFCreateStreamDescriptor(0, &types)?;
            descriptor
                .GetMediaTypeHandler()?
                .SetCurrentMediaType(types[0].as_ref().unwrap())?;
            Ok(Self {
                attributes,
                queue: MFCreateEventQueue()?,
                descriptor,
                shared,
                inner: Mutex::new(Inner {
                    state: MF_STREAM_STATE_STOPPED,
                    allocator: None,
                    worker: None,
                    source: None,
                    shut: false,
                }),
            })
        }
    }

    pub fn descriptor(&self) -> &IMFStreamDescriptor {
        &self.descriptor
    }

    pub fn set_source(&self, source: Option<IMFMediaSource>) {
        self.inner.lock().unwrap().source = source;
    }

    pub fn set_allocator(&self, allocator: Option<IMFVideoSampleAllocator>) {
        self.inner.lock().unwrap().allocator = allocator;
    }

    /// Starts sending with the stream's current media type.
    pub fn start(&self) -> windows_core::Result<()> {
        self.stop_worker();
        // SAFETY: COM calls on our descriptor, queue and allocator.
        unsafe {
            let current = self
                .descriptor
                .GetMediaTypeHandler()?
                .GetCurrentMediaType()?;
            let format = Format::of(&current).unwrap_or(Format::ALL[0]);
            let allocator = self.inner.lock().unwrap().allocator.clone();
            if let Some(a) = &allocator {
                a.InitializeSampleAllocator(10, &current)?;
            }
            let (requests, rx) = mpsc::channel();
            let stop = Arc::new(AtomicBool::new(false));
            let thread = {
                let stop = stop.clone();
                let queue = Sendable(self.queue.clone());
                let allocator = Sendable(allocator);
                let shared = self.shared.clone();
                std::thread::Builder::new()
                    .name("oculibre-mf-camera".into())
                    .spawn(move || {
                        let (queue, allocator) = (queue, allocator);
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            work(&rx, &queue.0, allocator.0.as_ref(), format, &shared, &stop)
                        }));
                    })
                    .map_err(|_| windows_core::Error::from(windows::Win32::Foundation::E_FAIL))?
            };
            let mut inner = self.inner.lock().unwrap();
            inner.worker = Some(Worker {
                requests,
                stop,
                thread,
            });
            inner.state = MF_STREAM_STATE_RUNNING;
            self.queue.QueueEventParamVar(
                MEStreamStarted.0 as u32,
                &GUID::zeroed(),
                S_OK,
                std::ptr::null(),
            )?;
        }
        Ok(())
    }

    pub fn stop(&self) -> windows_core::Result<()> {
        self.stop_worker();
        let allocator = {
            let mut inner = self.inner.lock().unwrap();
            inner.state = MF_STREAM_STATE_STOPPED;
            inner.allocator.clone()
        };
        // SAFETY: COM calls on our allocator and queue.
        unsafe {
            if let Some(a) = allocator {
                let _ = a.UninitializeSampleAllocator();
            }
            self.queue.QueueEventParamVar(
                MEStreamStopped.0 as u32,
                &GUID::zeroed(),
                S_OK,
                std::ptr::null(),
            )
        }
    }

    pub fn shutdown(&self) {
        self.stop_worker();
        let mut inner = self.inner.lock().unwrap();
        inner.shut = true;
        inner.source = None;
        if let Some(a) = inner.allocator.take() {
            // SAFETY: COM call on our allocator.
            let _ = unsafe { a.UninitializeSampleAllocator() };
        }
        // SAFETY: COM call on our queue.
        let _ = unsafe { self.queue.Shutdown() };
    }

    fn stop_worker(&self) {
        let worker = self.inner.lock().unwrap().worker.take();
        if let Some(w) = worker {
            w.stop.store(true, Ordering::Release);
            drop(w.requests); // wakes the thread
            let _ = w.thread.join();
        }
    }
}

/// Answers sample requests until stopped.
fn work(
    requests: &Receiver<Option<Sendable<IUnknown>>>,
    queue: &IMFMediaEventQueue,
    allocator: Option<&IMFVideoSampleAllocator>,
    format: Format,
    shared: &Option<Shared>,
    stop: &AtomicBool,
) {
    // SAFETY: COM for this thread.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let interval = format.interval();
    let started = Instant::now();
    let mut seen_frames = u64::MAX;
    let mut last_new = Instant::now() - STALE * 2;
    let mut last_sent = Instant::now() - interval;
    let mut last = oculibre_vcam_shm::LastPicture::default();
    let mut fitted = vec![0u8; format.width * format.height * 2];
    while !stop.load(Ordering::Acquire) {
        let token = match requests.recv_timeout(Duration::from_millis(100)) {
            Ok(token) => token,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        // Pace: a new picture from Oculibre as soon as ~an interval passed; otherwise the last
        // one again after an interval (steady rate for the app).
        let new = loop {
            if stop.load(Ordering::Acquire) {
                return;
            }
            let frames = shared.as_ref().map(|s| s.frames().frames());
            let new = frames.is_some_and(|f| f != seen_frames);
            let since = last_sent.elapsed();
            let phone = last_new.elapsed() < STALE;
            let repeat_after = if phone { interval * 3 / 2 } else { interval };
            if new && since >= interval * 3 / 4 || since >= repeat_after {
                if new {
                    seen_frames = frames.unwrap_or(seen_frames);
                    last_new = Instant::now();
                }
                break new;
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        let due = if new || last_sent.elapsed() >= interval * 2 {
            Instant::now()
        } else {
            last_sent + interval
        };

        // The picture, YUY2 at the stream's size, then NV12 into the sample.
        if last_new.elapsed() >= STALE {
            last.clear();
        }
        let got = shared
            .as_ref()
            .filter(|_| last_new.elapsed() < STALE)
            .and_then(|s| last.update(s.frames()));
        match got {
            Some((frame, w, h)) => {
                picture::fit(frame, w, h, &mut fitted, format.width, format.height)
            }
            None => picture::waiting(
                &mut fitted,
                format.width,
                format.height,
                started.elapsed().as_millis() as u64,
            ),
        }
        if let Ok(sample) = make_sample(allocator, format, &fitted) {
            // SAFETY: COM calls on the sample and our queue.
            unsafe {
                let _ = sample.SetSampleTime(MFGetSystemTime());
                let _ = sample.SetSampleDuration(10_000_000 / i64::from(format.fps));
                if let Some(t) = &token {
                    let _ = sample.SetUnknown(&MFSampleExtension_Token, &t.0);
                }
                let _ = queue.QueueEventParamUnk(
                    MEMediaSample.0 as u32,
                    &GUID::zeroed(),
                    S_OK,
                    &sample,
                );
            }
        }
        last_sent = due;
    }
}

/// A sample from the Frame Server's allocator (or our own), filled with `yuy2` as NV12.
fn make_sample(
    allocator: Option<&IMFVideoSampleAllocator>,
    format: Format,
    yuy2: &[u8],
) -> windows_core::Result<IMFSample> {
    let (w, h) = (format.width, format.height);
    // SAFETY: COM calls; the buffer is written only while locked, within its size.
    unsafe {
        let sample = match allocator {
            Some(a) => a.AllocateSample()?,
            None => {
                let sample = MFCreateSample()?;
                sample.AddBuffer(&MFCreate2DMediaBuffer(
                    w as u32,
                    h as u32,
                    MFVideoFormat_NV12.data1,
                    false,
                )?)?;
                sample
            }
        };
        let buffer = sample.GetBufferByIndex(0)?;
        if let Ok(two_d) = buffer.cast::<IMF2DBuffer>() {
            let mut scan0 = std::ptr::null_mut();
            let mut pitch = 0i32;
            two_d.Lock2D(&mut scan0, &mut pitch)?;
            let pitch = pitch.unsigned_abs() as usize;
            let out = std::slice::from_raw_parts_mut(scan0, pitch * h * 3 / 2);
            picture::yuy2_to_nv12(yuy2, w, h, out, pitch);
            let _ = two_d.Unlock2D();
        } else {
            let mut data = std::ptr::null_mut();
            let mut max = 0u32;
            buffer.Lock(&mut data, Some(&mut max), None)?;
            if (max as usize) >= w * h * 3 / 2 {
                picture::yuy2_to_nv12(
                    yuy2,
                    w,
                    h,
                    std::slice::from_raw_parts_mut(data, w * h * 3 / 2),
                    w,
                );
            }
            let _ = buffer.Unlock();
            buffer.SetCurrentLength((w * h * 3 / 2) as u32)?;
        }
        Ok(sample)
    }
}

impl IMFMediaEventGenerator_Impl for Stream_Impl {
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

impl IMFMediaStream_Impl for Stream_Impl {
    fn GetMediaSource(&self) -> windows_core::Result<IMFMediaSource> {
        let inner = self.inner.lock().unwrap();
        if inner.shut {
            return Err(MF_E_SHUTDOWN.into());
        }
        inner.source.clone().ok_or(MF_E_SHUTDOWN.into())
    }

    fn GetStreamDescriptor(&self) -> windows_core::Result<IMFStreamDescriptor> {
        Ok(self.descriptor.clone())
    }

    fn RequestSample(&self, token: Ref<IUnknown>) -> windows_core::Result<()> {
        guard(|| {
            let inner = self.inner.lock().unwrap();
            if inner.shut {
                return Err(MF_E_SHUTDOWN.into());
            }
            match &inner.worker {
                Some(w) if inner.state == MF_STREAM_STATE_RUNNING => {
                    let _ = w.requests.send(token.as_ref().cloned().map(Sendable));
                    Ok(())
                }
                _ => Err(MF_E_INVALIDREQUEST.into()),
            }
        })
    }
}

impl IMFMediaStream2_Impl for Stream_Impl {
    fn SetStreamState(&self, value: MF_STREAM_STATE) -> windows_core::Result<()> {
        guard(|| {
            let state = self.inner.lock().unwrap().state;
            match value {
                v if v == state => Ok(()),
                MF_STREAM_STATE_RUNNING => self.start(),
                MF_STREAM_STATE_STOPPED => self.stop(),
                MF_STREAM_STATE_PAUSED if state == MF_STREAM_STATE_RUNNING => {
                    self.inner.lock().unwrap().state = MF_STREAM_STATE_PAUSED;
                    Ok(())
                }
                _ => Err(MF_E_INVALID_STATE_TRANSITION.into()),
            }
        })
    }

    fn GetStreamState(&self) -> windows_core::Result<MF_STREAM_STATE> {
        Ok(self.inner.lock().unwrap().state)
    }
}

impl IKsControl_Impl for Stream_Impl {
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
