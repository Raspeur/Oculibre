//! The camera's output pin: how the app connects to us, which format it gets
//! (`IAMStreamConfig`), that it is a capture pin (`IKsPropertySet`), and the thread that sends it
//! pictures.
//!
//! Lifecycle (DirectShow): Stopped → Paused: the allocator is committed and the sending thread
//! starts, idle. Paused → Running: it sends, with time stamps from the graph's clock. Stop: the
//! allocator is decommitted (a thread blocked waiting for a buffer wakes up) and the thread ends.

use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use oculibre_vcam_shm::Shared;
use windows::core::implement;
use windows::Win32::Foundation::{
    E_FAIL, E_INVALIDARG, E_NOTIMPL, E_POINTER, E_UNEXPECTED, S_FALSE, S_OK,
};
use windows::Win32::Media::DirectShow::*;
use windows::Win32::Media::IReferenceClock;
use windows::Win32::Media::KernelStreaming::{IKsPropertySet, IKsPropertySet_Impl};

/// `KSPROPERTY_SUPPORT_GET` (ks.h): the property can be read.
const KSPROPERTY_SUPPORT_GET: u32 = 1;
use windows::Win32::Media::MediaFoundation::{
    AMPROPSETID_Pin, CLSID_MemoryAllocator, AM_MEDIA_TYPE, PIN_CATEGORY_CAPTURE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows_core::{IUnknownImpl, Interface, Ref, GUID, HRESULT, PWSTR};

use crate::com::{co_string, copy_name, guard, guard_hr, wide, Sendable};
use crate::enums::TypeEnum;
use crate::filter::FilterShared;
use crate::media::{free_media_type_content, Format};
use oculibre_vcam_shm::picture;

pub const PIN_NAME: &str = "Capture";

/// After this long without a new picture from Oculibre, show the "waiting" picture.
const STALE: Duration = Duration::from_secs(2);

#[derive(Default)]
struct Connection {
    peer: Option<IPin>,
    input: Option<IMemInputPin>,
    allocator: Option<IMemAllocator>,
    /// The format in use while connected.
    format: Option<Format>,
    /// The format the app asked for with `SetFormat` (used at the next connection).
    wanted: Option<Format>,
}

struct Sending {
    stop: Arc<AtomicBool>,
    thread: JoinHandle<()>,
}

/// What the sending thread reads from the filter while it runs.
pub struct RunState {
    pub running: AtomicBool,
    /// Graph time at which `Run` started, and the clock (none: our own time).
    pub start: AtomicI64,
    pub clock: Mutex<Option<Sendable<IReferenceClock>>>,
}

#[implement(IPin, IAMStreamConfig, IKsPropertySet)]
pub struct Pin {
    filter: Arc<FilterShared>,
    connection: Mutex<Connection>,
    sending: Mutex<Option<Sending>>,
    run: Arc<RunState>,
}

impl Pin {
    pub fn new(filter: Arc<FilterShared>) -> Self {
        Self {
            filter,
            connection: Mutex::new(Connection::default()),
            sending: Mutex::new(None),
            run: Arc::new(RunState {
                running: AtomicBool::new(false),
                start: AtomicI64::new(0),
                clock: Mutex::new(None),
            }),
        }
    }

    /// Formats in order of preference: the one asked with SetFormat first.
    fn formats(&self) -> Vec<Format> {
        let wanted = self.connection.lock().unwrap().wanted;
        let mut all: Vec<Format> = Format::all().collect();
        if let Some(w) = wanted {
            all.retain(|f| (f.width, f.height) != (w.width, w.height));
            all.insert(0, w);
        }
        all
    }

    /// Stopped → Paused: commit the allocator, start the (idle) sending thread.
    pub fn active(&self) -> windows_core::Result<()> {
        let c = self.connection.lock().unwrap();
        let (Some(input), Some(allocator), Some(format)) =
            (c.input.clone(), c.allocator.clone(), c.format)
        else {
            return Ok(()); // not connected: nothing to do
        };
        drop(c);
        // SAFETY: COM call on the allocator we chose.
        unsafe { allocator.Commit()? };
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = stop.clone();
            let run = self.run.clone();
            let (input, allocator) = (Sendable(input), Sendable(allocator));
            std::thread::Builder::new()
                .name("oculibre-camera".into())
                .spawn(move || {
                    // Whole wrappers moved in (not just their fields: those are not Send).
                    let (input, allocator) = (input, allocator);
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        send(&input.0, &allocator.0, format, &run, &stop)
                    }));
                })
                .map_err(|_| windows_core::Error::from(E_FAIL))?
        };
        *self.sending.lock().unwrap() = Some(Sending { stop, thread });
        Ok(())
    }

    pub fn run(&self, start: i64, clock: Option<IReferenceClock>) {
        *self.run.clock.lock().unwrap() = clock.map(Sendable);
        self.run.start.store(start, Ordering::Relaxed);
        self.run.running.store(true, Ordering::Release);
    }

    pub fn pause(&self) {
        self.run.running.store(false, Ordering::Release);
    }

    /// → Stopped: decommit (wakes the thread if it waits for a buffer), end the thread.
    pub fn inactive(&self) {
        self.run.running.store(false, Ordering::Release);
        let sending = self.sending.lock().unwrap().take();
        if let Some(allocator) = self.connection.lock().unwrap().allocator.clone() {
            // SAFETY: COM call on our allocator.
            let _ = unsafe { allocator.Decommit() };
        }
        if let Some(s) = sending {
            s.stop.store(true, Ordering::Release);
            let _ = s.thread.join();
        }
    }

    /// Connects to `peer` with `format`: the peer accepts it, then we agree on buffers.
    fn try_connect(&self, me: &IPin, peer: &IPin, format: Format) -> windows_core::Result<()> {
        let mut mt = format.media_type();
        // SAFETY: COM calls with a media type we own (freed below).
        let result = unsafe {
            peer.ReceiveConnection(me, &mt).and_then(|_| {
                let input: IMemInputPin = peer.cast()?;
                let allocator = self.allocator(&input, format).inspect_err(|_| {
                    let _ = peer.Disconnect();
                })?;
                let mut c = self.connection.lock().unwrap();
                c.peer = Some(peer.clone());
                c.input = Some(input);
                c.allocator = Some(allocator);
                c.format = Some(format);
                Ok(())
            })
        };
        // SAFETY: our own media type's blocks.
        unsafe { free_media_type_content(&mut mt) };
        result
    }

    /// The peer's allocator if it has one, else the system's; buffers of one picture each.
    fn allocator(
        &self,
        input: &IMemInputPin,
        format: Format,
    ) -> windows_core::Result<IMemAllocator> {
        // SAFETY: COM calls; structures passed by pointer live on our stack.
        unsafe {
            let needs = input.GetAllocatorRequirements().unwrap_or_default();
            let allocator = match input.GetAllocator() {
                Ok(a) => a,
                Err(_) => CoCreateInstance(&CLSID_MemoryAllocator, None, CLSCTX_INPROC_SERVER)?,
            };
            let want = ALLOCATOR_PROPERTIES {
                cBuffers: needs.cBuffers.max(3),
                cbBuffer: format.bytes() as i32,
                cbAlign: needs.cbAlign.max(1),
                cbPrefix: needs.cbPrefix,
            };
            let got = allocator.SetProperties(&want)?;
            if (got.cbBuffer as usize) < format.bytes() {
                return Err(E_FAIL.into());
            }
            input.NotifyAllocator(&allocator, false)?;
            Ok(allocator)
        }
    }
}

/// Sends pictures until told to stop: each new picture from Oculibre as it comes, at most one
/// per frame interval, and the last one again when none comes (apps expect a steady rate).
fn send(
    input: &IMemInputPin,
    allocator: &IMemAllocator,
    format: Format,
    run: &RunState,
    stop: &AtomicBool,
) {
    // SAFETY: COM for this thread.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let interval = Duration::from_nanos(format.interval as u64 * 100);
    let mut shared: Option<Shared> = None;
    let mut tried_open = Instant::now() - Duration::from_secs(10);
    let mut seen_frames = u64::MAX;
    let mut last_new = Instant::now() - STALE * 2;
    let mut last_sent = Instant::now() - interval;
    let mut last = oculibre_vcam_shm::LastPicture::default();
    let mut own_clock = Instant::now();
    let mut first = true;
    let started = Instant::now();
    while !stop.load(Ordering::Acquire) {
        if !run.running.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(5));
            own_clock = Instant::now();
            first = true;
            continue;
        }
        // Oculibre may start after the app: look for its memory once a second.
        if shared.is_none() && tried_open.elapsed() >= Duration::from_secs(1) {
            shared = Shared::open();
            tried_open = Instant::now();
        }
        // Wait for a new picture (not sooner than ~an interval after the last one we sent),
        // or for the interval to pass with nothing new.
        let frames = shared.as_ref().map(|s| s.frames().frames());
        let new = frames.is_some_and(|f| f != seen_frames);
        let since = last_sent.elapsed();
        // A phone sending: its pictures set the pace (sent as they come, no added delay); the
        // last one again only if one is late. No phone: our own steady pace.
        let phone = last_new.elapsed() < STALE;
        let repeat_after = if phone { interval * 3 / 2 } else { interval };
        if !(new && since >= interval * 3 / 4 || since >= repeat_after) {
            std::thread::sleep(Duration::from_millis(1));
            continue;
        }
        // When this picture is due: right now for a new one; on the beat for a repeat (filling
        // and sending take time, which must not stretch the period).
        let due = if new || since >= interval * 2 {
            Instant::now()
        } else {
            last_sent + repeat_after
        };
        if new {
            seen_frames = frames.unwrap_or(seen_frames);
            last_new = Instant::now();
        }

        let mut sample = None;
        // SAFETY: COM call; blocks until a buffer is free (or the allocator is decommitted).
        if unsafe { allocator.GetBuffer(&mut sample, None, None, 0) }.is_err() {
            if stop.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        let Some(sample) = sample else { continue };
        // SAFETY: the sample's buffer, of at least `format.bytes()` (agreed with the allocator).
        unsafe {
            let Ok(data) = sample.GetPointer() else {
                continue;
            };
            let size = (sample.GetSize() as usize).min(format.bytes());
            let out = std::slice::from_raw_parts_mut(data, size);
            if last_new.elapsed() >= STALE {
                last.clear();
            }
            let got = shared
                .as_ref()
                .filter(|_| last_new.elapsed() < STALE)
                .and_then(|s| last.update(s.frames()));
            match got {
                Some((frame, w, h)) if size == format.bytes() => {
                    picture::fit(frame, w, h, out, format.width, format.height)
                }
                _ if size == format.bytes() => picture::waiting(
                    out,
                    format.width,
                    format.height,
                    started.elapsed().as_millis() as u64,
                ),
                _ => {}
            }
            let _ = sample.SetActualDataLength(size as i32);
            let clock = run.clock.lock().unwrap().as_ref().map(|c| c.0.clone());
            let now = match clock.and_then(|c| c.GetTime().ok()) {
                Some(t) => t - run.start.load(Ordering::Relaxed),
                None => own_clock.elapsed().as_nanos() as i64 / 100,
            };
            let end = now + format.interval;
            let _ = sample.SetTime(Some(&now), Some(&end));
            let _ = sample.SetSyncPoint(true);
            let _ = sample.SetDiscontinuity(first);
            first = false;
            let _ = input.Receive(&sample);
        }
        last_sent = due;
    }
}

impl IPin_Impl for Pin_Impl {
    fn Connect(
        &self,
        receivepin: Ref<IPin>,
        pmt: *const AM_MEDIA_TYPE,
    ) -> windows_core::Result<()> {
        guard(|| {
            let peer = receivepin.ok()?;
            if self.connection.lock().unwrap().peer.is_some() {
                return Err(VFW_E_ALREADY_CONNECTED.into());
            }
            if !self.filter.stopped() {
                return Err(VFW_E_NOT_STOPPED.into());
            }
            let me: IPin = self.to_object().to_interface();
            // A full type from the app: that one only. Partial or none: ours, best first.
            let candidates: Vec<Format> = match unsafe { pmt.as_ref() } {
                // SAFETY: the app's media type.
                Some(mt) if mt.formattype != GUID::zeroed() => {
                    unsafe { Format::accept(mt) }.into_iter().collect()
                }
                _ => self.formats(),
            };
            for format in candidates {
                if self.try_connect(&me, peer, format).is_ok() {
                    return Ok(());
                }
            }
            Err(VFW_E_NO_ACCEPTABLE_TYPES.into())
        })
    }

    fn ReceiveConnection(&self, _: Ref<IPin>, _: *const AM_MEDIA_TYPE) -> windows_core::Result<()> {
        Err(E_UNEXPECTED.into()) // an output pin
    }

    fn Disconnect(&self) -> windows_core::Result<()> {
        guard(|| {
            if !self.filter.stopped() {
                return Err(VFW_E_NOT_STOPPED.into());
            }
            let mut c = self.connection.lock().unwrap();
            if c.peer.is_none() {
                return Err(S_FALSE.into());
            }
            if let Some(a) = c.allocator.take() {
                // SAFETY: COM call on our allocator.
                let _ = unsafe { a.Decommit() };
            }
            c.peer = None;
            c.input = None;
            c.format = None;
            Ok(())
        })
    }

    fn ConnectedTo(&self) -> windows_core::Result<IPin> {
        self.connection
            .lock()
            .unwrap()
            .peer
            .clone()
            .ok_or_else(|| VFW_E_NOT_CONNECTED.into())
    }

    fn ConnectionMediaType(&self, pmt: *mut AM_MEDIA_TYPE) -> windows_core::Result<()> {
        guard(|| {
            if pmt.is_null() {
                return Err(E_POINTER.into());
            }
            let format = self.connection.lock().unwrap().format;
            match format {
                // SAFETY: the app's structure, which it frees with FreeMediaType.
                Some(f) => unsafe {
                    pmt.write(f.media_type());
                    Ok(())
                },
                None => {
                    // SAFETY: as above: an empty type.
                    unsafe { pmt.write(AM_MEDIA_TYPE::default()) };
                    Err(VFW_E_NOT_CONNECTED.into())
                }
            }
        })
    }

    fn QueryPinInfo(&self, pinfo: *mut PIN_INFO) -> windows_core::Result<()> {
        guard(|| {
            if pinfo.is_null() {
                return Err(E_POINTER.into());
            }
            let mut info = PIN_INFO {
                pFilter: std::mem::ManuallyDrop::new(self.filter.filter()),
                dir: PINDIR_OUTPUT,
                achName: [0; 128],
            };
            copy_name(&wide(PIN_NAME), &mut info.achName);
            // SAFETY: the app's structure; it releases pFilter.
            unsafe { pinfo.write(info) };
            Ok(())
        })
    }

    fn QueryDirection(&self) -> windows_core::Result<PIN_DIRECTION> {
        Ok(PINDIR_OUTPUT)
    }

    fn QueryId(&self) -> windows_core::Result<PWSTR> {
        co_string(PIN_NAME)
    }

    fn QueryAccept(&self, pmt: *const AM_MEDIA_TYPE) -> HRESULT {
        guard_hr(|| {
            // SAFETY: the app's media type.
            match unsafe { pmt.as_ref() }.and_then(|mt| unsafe { Format::accept(mt) }) {
                Some(_) => S_OK,
                None => S_FALSE,
            }
        })
    }

    fn EnumMediaTypes(&self) -> windows_core::Result<IEnumMediaTypes> {
        guard(|| Ok(TypeEnum::create(self.formats())))
    }

    fn QueryInternalConnections(
        &self,
        _: windows_core::OutRef<IPin>,
        _: *mut u32,
    ) -> windows_core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    fn EndOfStream(&self) -> windows_core::Result<()> {
        Err(E_UNEXPECTED.into())
    }

    fn BeginFlush(&self) -> windows_core::Result<()> {
        Err(E_UNEXPECTED.into())
    }

    fn EndFlush(&self) -> windows_core::Result<()> {
        Err(E_UNEXPECTED.into())
    }

    fn NewSegment(&self, _: i64, _: i64, _: f64) -> windows_core::Result<()> {
        Ok(())
    }
}

impl IAMStreamConfig_Impl for Pin_Impl {
    fn SetFormat(&self, pmt: *const AM_MEDIA_TYPE) -> windows_core::Result<()> {
        guard(|| {
            // SAFETY: the app's media type.
            let mt = unsafe { pmt.as_ref() }.ok_or(windows_core::Error::from(E_POINTER))?;
            let format = unsafe { Format::accept(mt) }
                .ok_or(windows_core::Error::from(VFW_E_INVALIDMEDIATYPE))?;
            let mut c = self.connection.lock().unwrap();
            c.wanted = Some(format);
            let reconnect = c.format.is_some_and(|f| f != format) && c.peer.is_some();
            drop(c);
            if reconnect {
                // Connected with another format: ask the graph to connect us again.
                if let Some(graph) = self.filter.graph() {
                    let me: IPin = self.to_object().to_interface();
                    // SAFETY: COM call on the graph we belong to.
                    unsafe { graph.Reconnect(&me)? };
                }
            }
            Ok(())
        })
    }

    fn GetFormat(&self) -> windows_core::Result<*mut AM_MEDIA_TYPE> {
        guard(|| {
            let c = self.connection.lock().unwrap();
            let format = c.format.or(c.wanted).unwrap_or(Format::DEFAULT);
            Ok(format.new_media_type())
        })
    }

    fn GetNumberOfCapabilities(&self, count: *mut i32, size: *mut i32) -> windows_core::Result<()> {
        if count.is_null() || size.is_null() {
            return Err(E_POINTER.into());
        }
        // SAFETY: the app's integers.
        unsafe {
            count.write(Format::all().count() as i32);
            size.write(size_of::<VIDEO_STREAM_CONFIG_CAPS>() as i32);
        }
        Ok(())
    }

    fn GetStreamCaps(
        &self,
        index: i32,
        pmt: *mut *mut AM_MEDIA_TYPE,
        caps: *mut u8,
    ) -> windows_core::Result<()> {
        guard(|| {
            let format = usize::try_from(index)
                .ok()
                .and_then(|i| Format::all().nth(i))
                .ok_or(windows_core::Error::from(S_FALSE))?;
            if pmt.is_null() || caps.is_null() {
                return Err(E_POINTER.into());
            }
            // SAFETY: the app's pointers; `caps` has room for VIDEO_STREAM_CONFIG_CAPS (it asked
            // GetNumberOfCapabilities for the size).
            unsafe {
                pmt.write(format.new_media_type());
                (caps as *mut VIDEO_STREAM_CONFIG_CAPS).write_unaligned(format.caps());
            }
            Ok(())
        })
    }
}

impl IKsPropertySet_Impl for Pin_Impl {
    fn Set(
        &self,
        _: *const GUID,
        _: u32,
        _: *const core::ffi::c_void,
        _: u32,
        _: *const core::ffi::c_void,
        _: u32,
    ) -> windows_core::Result<()> {
        Err(E_NOTIMPL.into())
    }

    /// The only property: our pin's category, "capture" (apps look for it).
    fn Get(
        &self,
        set: *const GUID,
        id: u32,
        _: *const core::ffi::c_void,
        _: u32,
        data: *mut core::ffi::c_void,
        size: u32,
        returned: *mut u32,
    ) -> windows_core::Result<()> {
        // SAFETY: the app's pointers, checked; `data` holds `size` bytes.
        unsafe {
            if set.as_ref() != Some(&AMPROPSETID_Pin) || id != AMPROPERTY_PIN_CATEGORY.0 as u32 {
                return Err(E_PROP_ID_UNSUPPORTED.into());
            }
            if !returned.is_null() {
                returned.write(size_of::<GUID>() as u32);
            }
            if data.is_null() {
                return Ok(()); // the app asks for the size first
            }
            if (size as usize) < size_of::<GUID>() {
                return Err(E_INVALIDARG.into());
            }
            (data as *mut GUID).write_unaligned(PIN_CATEGORY_CAPTURE);
        }
        Ok(())
    }

    fn QuerySupported(&self, set: *const GUID, id: u32) -> windows_core::Result<u32> {
        // SAFETY: the app's GUID.
        if unsafe { set.as_ref() } == Some(&AMPROPSETID_Pin)
            && id == AMPROPERTY_PIN_CATEGORY.0 as u32
        {
            Ok(KSPROPERTY_SUPPORT_GET)
        } else {
            Err(E_PROP_ID_UNSUPPORTED.into())
        }
    }
}
