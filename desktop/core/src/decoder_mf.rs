//! Windows' own H.264 decoder (Media Foundation, #17): licensed with Windows, so Oculibre ships
//! no decoder there (`docs/research/windows.typ`). On the graphics card when there is one
//! (Direct3D 11: AMD, Intel, NVIDIA through the same decoder), in software otherwise.
//!
//! The decoder is an MFT (Media Foundation Transform): one H.264 access unit in (Annex B, as the
//! phone sends it), pictures out in NV12. `MF_LOW_LATENCY` makes it give each picture back at
//! once instead of keeping a few for reordering (phones send no B-frames): protocol §12.
//! "N" editions of Windows have no H.264 decoder until the Media Feature Pack is installed: the
//! decoder then cannot be created, and the error says so.

use std::mem::ManuallyDrop;

use windows::core::Interface;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};

use crate::yuv::Picture;

/// One stream's decoder.
pub struct MediaFoundation {
    mft: IMFTransform,
    /// The graphics card's device manager while decoding on it (kept alive with the decoder).
    gpu: Option<IMFDXGIDeviceManager>,
    /// Our own way back from video memory, while decoding on the graphics card.
    readback: Option<Readback>,
    /// Coded size of the output pictures (height rounded up to 16: 1088 for 1080p), the stride
    /// of their rows, and the visible size (what the phone filmed).
    coded: (usize, usize),
    stride: usize,
    visible: (usize, usize),
    /// Whether we must allocate the output samples (the software decoder wants us to).
    our_samples: bool,
    time: i64,
}

fn mf_error(what: &str, e: windows::core::Error) -> String {
    format!("Media Foundation: {what}: {}", e.message())
}

/// Starts Media Foundation once per process, and COM on the calling thread.
fn start() -> Result<(), String> {
    use std::sync::OnceLock;
    static STARTED: OnceLock<Result<(), String>> = OnceLock::new();
    // SAFETY: COM initialisation for this thread; "already initialised" is fine.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    STARTED
        .get_or_init(|| {
            // SAFETY: plain library initialisation, never undone (the process keeps it).
            unsafe { MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET) }
                .map_err(|e| mf_error("cannot start", e))
        })
        .clone()
}

impl MediaFoundation {
    pub fn hardware(&self) -> bool {
        self.gpu.is_some()
    }

    /// `hardware`: decode on the graphics card (Direct3D 11), or fail (the caller then tries
    /// software). The pictures come back from video memory through the same `copy`.
    pub fn new(hardware: bool) -> Result<Self, String> {
        start()?;
        // SAFETY: COM calls on interfaces we own; arguments are valid for each call.
        unsafe {
            let mft: IMFTransform = CoCreateInstance(&CLSID_MSH264DecoderMFT, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| {
                    format!(
                        "Windows has no H.264 decoder ({}): on an \"N\" edition, install the Media Feature Pack",
                        e.message()
                    )
                })?;
            let attributes = mft.GetAttributes().ok();
            if let Some(a) = &attributes {
                let _ = a.SetUINT32(&MF_LOW_LATENCY, 1);
            }
            let gpu = if hardware {
                if attributes
                    .as_ref()
                    .and_then(|a| a.GetUINT32(&MF_SA_D3D11_AWARE).ok())
                    != Some(1)
                {
                    return Err(
                        "Media Foundation: this H.264 decoder cannot use Direct3D 11".into(),
                    );
                }
                let (manager, device) = gpu_manager()?;
                mft.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize)
                    .map_err(|e| mf_error("graphics card", e))?;
                Some((manager, Readback::new(device)))
            } else {
                None
            };
            let (gpu, readback) = match gpu {
                Some((m, r)) => (Some(m), r),
                None => (None, None),
            };
            let input = MFCreateMediaType().map_err(|e| mf_error("media type", e))?;
            input
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .and_then(|_| input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264))
                .and_then(|_| mft.SetInputType(0, &input, 0))
                .map_err(|e| mf_error("H.264 input", e))?;
            let mut decoder = Self {
                mft,
                gpu,
                readback,
                coded: (0, 0),
                stride: 0,
                visible: (0, 0),
                our_samples: true,
                time: 0,
            };
            decoder.choose_output()?;
            decoder
                .mft
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .and_then(|_| {
                    decoder
                        .mft
                        .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                })
                .map_err(|e| mf_error("start streaming", e))?;
            Ok(decoder)
        }
    }

    /// Picks NV12 among the output types the decoder offers (again after a format change: a new
    /// size), and reads the picture's geometry from it.
    fn choose_output(&mut self) -> Result<(), String> {
        // SAFETY: COM calls on interfaces we own.
        unsafe {
            let mut index = 0;
            let chosen = loop {
                let Ok(t) = self.mft.GetOutputAvailableType(0, index) else {
                    return Err("Media Foundation: the H.264 decoder offers no NV12 output".into());
                };
                if t.GetGUID(&MF_MT_SUBTYPE).ok() == Some(MFVideoFormat_NV12) {
                    break t;
                }
                index += 1;
            };
            self.mft
                .SetOutputType(0, &chosen, 0)
                .map_err(|e| mf_error("NV12 output", e))?;
            if let Ok(size) = chosen.GetUINT64(&MF_MT_FRAME_SIZE) {
                self.coded = ((size >> 32) as usize, (size & 0xffff_ffff) as usize);
            }
            self.stride = chosen
                .GetUINT32(&MF_MT_DEFAULT_STRIDE)
                .map(|s| (s as i32).unsigned_abs() as usize)
                .unwrap_or(self.coded.0);
            self.visible = aperture(&chosen).unwrap_or(self.coded);
            let info = self
                .mft
                .GetOutputStreamInfo(0)
                .map_err(|e| mf_error("output stream", e))?;
            self.our_samples = info.dwFlags
                & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0)
                    as u32
                == 0;
            Ok(())
        }
    }

    /// Decodes one access unit (or SPS/PPS) into `out`. `Ok(true)`: `out` holds a new picture.
    pub fn decode(&mut self, au: &[u8], out: &mut Picture) -> Result<bool, String> {
        let sample = memory_sample(au, self.time)?;
        self.time += 1;
        // SAFETY: COM call with a sample we own.
        let fed = unsafe { self.mft.ProcessInput(0, &sample, 0) };
        if let Err(e) = fed {
            if e.code() != MF_E_NOTACCEPTING {
                return Err(mf_error("decode", e));
            }
            // Pictures are waiting: take them, then feed again.
            let got = self.drain(out)?;
            // SAFETY: as above.
            unsafe { self.mft.ProcessInput(0, &sample, 0) }.map_err(|e| mf_error("decode", e))?;
            return Ok(self.drain(out)? || got);
        }
        self.drain(out)
    }

    /// Takes every picture ready (normally one, at once); the last one ends in `out`.
    fn drain(&mut self, out: &mut Picture) -> Result<bool, String> {
        let mut got = false;
        loop {
            let provided = if self.our_samples {
                Some(picture_sample(self.coded)?)
            } else {
                None
            };
            let mut buffers = [MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(provided),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            }];
            let mut status = 0u32;
            // SAFETY: one output buffer description, ours; its COM fields are released below.
            let result = unsafe { self.mft.ProcessOutput(0, &mut buffers, &mut status) };
            let [buffer] = buffers;
            let sample = ManuallyDrop::into_inner(buffer.pSample);
            drop(ManuallyDrop::into_inner(buffer.pEvents));
            match result {
                Ok(()) => {
                    if let Some(sample) = sample {
                        self.copy(&sample, out)?;
                        got = true;
                    }
                }
                Err(e) if e.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(got),
                Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => self.choose_output()?,
                Err(e) => return Err(mf_error("picture", e)),
            }
        }
    }

    /// NV12 sample → `out` (visible area only).
    fn copy(&mut self, sample: &IMFSample, out: &mut Picture) -> Result<(), String> {
        let (w, h) = self.visible;
        if let Some(r) = &mut self.readback {
            match r.copy(sample, (w, h), out) {
                Ok(()) => return Ok(()),
                // Not a graphics card texture after all: Windows' own way below, from now on.
                Err(_) => self.readback = None,
            }
        }
        let rows = self.coded.1.max(h);
        // SAFETY: the buffer stays locked while we read it; reads stay within the locked size.
        unsafe {
            let buffer = sample
                .ConvertToContiguousBuffer()
                .map_err(|e| mf_error("picture buffer", e))?;
            if let Ok(two_d) = buffer.cast::<IMF2DBuffer>() {
                let mut scan0 = std::ptr::null_mut();
                let mut pitch = 0i32;
                two_d
                    .Lock2D(&mut scan0, &mut pitch)
                    .map_err(|e| mf_error("lock picture", e))?;
                let stride = pitch.unsigned_abs() as usize;
                let y = std::slice::from_raw_parts(scan0, stride * rows);
                let uv = std::slice::from_raw_parts(scan0.add(stride * rows), stride * rows / 2);
                out.copy_nv12(w, h, (y, stride), (uv, stride));
                let _ = two_d.Unlock2D();
            } else {
                let mut data = std::ptr::null_mut();
                let mut length = 0u32;
                buffer
                    .Lock(&mut data, None, Some(&mut length))
                    .map_err(|e| mf_error("lock picture", e))?;
                let stride = self.stride.max(w);
                let all = std::slice::from_raw_parts(data, length as usize);
                if all.len() < stride * rows * 3 / 2 {
                    let _ = buffer.Unlock();
                    return Err("Media Foundation: picture smaller than its size".into());
                }
                let (y, uv) = all.split_at(stride * rows);
                out.copy_nv12(w, h, (y, stride), (uv, stride));
                let _ = buffer.Unlock();
            }
        }
        Ok(())
    }
}

/// A Direct3D 11 device on the graphics card, for the decoder. Not on Microsoft's "Basic Render
/// Driver" (a virtual machine, or no graphics driver): it has no video decoder, software is as
/// good there.
fn gpu_manager() -> Result<
    (
        IMFDXGIDeviceManager,
        windows::Win32::Graphics::Direct3D11::ID3D11Device,
    ),
    String,
> {
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, ID3D11Device, ID3D11Multithread, D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
        D3D11_SDK_VERSION,
    };
    use windows::Win32::Graphics::Dxgi::IDXGIDevice;
    const MICROSOFT: u32 = 0x1414;
    // SAFETY: device creation and plain COM calls on what it returns.
    unsafe {
        let mut device: Option<ID3D11Device> = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_VIDEO_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
        .map_err(|e| mf_error("Direct3D 11", e))?;
        let device = device.ok_or("Direct3D 11: no device")?;
        let adapter = device
            .cast::<IDXGIDevice>()
            .and_then(|d| d.GetAdapter())
            .and_then(|a| a.GetDesc())
            .map_err(|e| mf_error("graphics card", e))?;
        if adapter.VendorId == MICROSOFT {
            return Err(
                "no graphics card with a video decoder (Microsoft Basic Render Driver)".into(),
            );
        }
        // The decoder uses the device from its own threads.
        if let Ok(m) = device.cast::<ID3D11Multithread>() {
            let _ = m.SetMultithreadProtected(true);
        }
        let mut token = 0u32;
        let mut manager = None;
        MFCreateDXGIDeviceManager(&mut token, &mut manager)
            .map_err(|e| mf_error("device manager", e))?;
        let manager = manager.ok_or("Media Foundation: no device manager")?;
        manager
            .ResetDevice(&device, token)
            .map_err(|e| mf_error("device manager", e))?;
        Ok((manager, device))
    }
}

/// Pictures back from the graphics card through one staging texture of ours, reused for every
/// picture. Windows' generic way (`ConvertToContiguousBuffer` on a video memory sample) costs
/// far more: about 20 ms a picture on an Intel Iris Xe laptop at 1080p, more than the 16.6 ms
/// a picture has at 60 fps, so the video fell behind (#4).
struct Readback {
    device: windows::Win32::Graphics::Direct3D11::ID3D11Device,
    context: windows::Win32::Graphics::Direct3D11::ID3D11DeviceContext,
    /// The staging texture and the size and format it was made for.
    staging: Option<(
        windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
        u32,
        u32,
        windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT,
    )>,
}

impl Readback {
    fn new(device: windows::Win32::Graphics::Direct3D11::ID3D11Device) -> Option<Self> {
        // SAFETY: plain COM call on a device we own.
        let context = unsafe { device.GetImmediateContext() }.ok()?;
        Some(Self {
            device,
            context,
            staging: None,
        })
    }

    /// The decoder's texture (one slice of its texture array) → staging → `out`.
    fn copy(
        &mut self,
        sample: &IMFSample,
        (w, h): (usize, usize),
        out: &mut Picture,
    ) -> Result<(), String> {
        use windows::Win32::Graphics::Direct3D11::*;
        // SAFETY: COM calls on interfaces we own. The device is multithread protected (the
        // decoder uses it from its own threads); the mapped memory is read only while mapped,
        // within RowPitch × rows.
        unsafe {
            let buffer = sample
                .GetBufferByIndex(0)
                .map_err(|e| mf_error("picture buffer", e))?;
            let dxgi = buffer
                .cast::<IMFDXGIBuffer>()
                .map_err(|e| mf_error("picture buffer", e))?;
            let mut raw = std::ptr::null_mut();
            dxgi.GetResource(&ID3D11Texture2D::IID, &mut raw)
                .map_err(|e| mf_error("picture texture", e))?;
            let texture = ID3D11Texture2D::from_raw(raw);
            let slice = dxgi
                .GetSubresourceIndex()
                .map_err(|e| mf_error("picture texture", e))?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut desc);
            let fits = matches!(&self.staging,
                Some((_, sw, sh, f)) if *sw == desc.Width && *sh == desc.Height && *f == desc.Format);
            if !fits {
                let staging_desc = D3D11_TEXTURE2D_DESC {
                    MipLevels: 1,
                    ArraySize: 1,
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0,
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                    ..desc
                };
                let mut staging = None;
                self.device
                    .CreateTexture2D(&staging_desc, None, Some(&mut staging))
                    .map_err(|e| mf_error("staging texture", e))?;
                let staging = staging.ok_or("Direct3D 11: no staging texture")?;
                self.staging = Some((staging, desc.Width, desc.Height, desc.Format));
            }
            let (staging, _, rows, _) = self.staging.as_ref().unwrap();
            let rows = *rows as usize;
            if rows < h {
                return Err("Direct3D 11: texture smaller than the picture".into());
            }
            self.context
                .CopySubresourceRegion(staging, 0, 0, 0, 0, &texture, slice, None);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context
                .Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|e| mf_error("read picture", e))?;
            let pitch = mapped.RowPitch as usize;
            let base = mapped.pData as *const u8;
            // NV12 in one texture: the luma rows, then the chroma rows (half as many).
            let y = std::slice::from_raw_parts(base, pitch * rows);
            let uv = std::slice::from_raw_parts(base.add(pitch * rows), pitch * rows / 2);
            out.copy_nv12(w, h, (y, pitch), (uv, pitch));
            self.context.Unmap(staging, 0);
        }
        Ok(())
    }
}

/// The visible part of the picture (1920×1080 inside 1920×1088).
fn aperture(t: &IMFMediaType) -> Option<(usize, usize)> {
    let mut area = MFVideoArea::default();
    let bytes = std::mem::size_of::<MFVideoArea>() as u32;
    // SAFETY: the blob is an MFVideoArea, read into one of the same size.
    unsafe {
        t.GetBlob(
            &MF_MT_MINIMUM_DISPLAY_APERTURE,
            std::slice::from_raw_parts_mut(
                (&mut area as *mut MFVideoArea).cast::<u8>(),
                bytes as usize,
            ),
            None,
        )
        .ok()?;
    }
    let (w, h) = (area.Area.cx as usize, area.Area.cy as usize);
    (w > 0 && h > 0).then_some((w, h))
}

/// An empty NV12 picture for the decoder to fill: a 2D buffer (it knows its row pitch; the
/// software decoder refuses a plain memory buffer with "CopyDecodedFrame failed").
fn picture_sample((w, h): (usize, usize)) -> Result<IMFSample, String> {
    // SAFETY: plain constructors.
    unsafe {
        let buffer = MFCreate2DMediaBuffer(w as u32, h as u32, MFVideoFormat_NV12.data1, false)
            .map_err(|e| mf_error("picture buffer", e))?;
        let sample = MFCreateSample().map_err(|e| mf_error("sample", e))?;
        sample
            .AddBuffer(&buffer)
            .map_err(|e| mf_error("sample", e))?;
        Ok(sample)
    }
}

/// A sample holding a copy of `bytes`.
fn memory_sample(bytes: &[u8], time: i64) -> Result<IMFSample, String> {
    // SAFETY: the buffer is locked while we copy into it, within its size.
    unsafe {
        let buffer =
            MFCreateMemoryBuffer(bytes.len().max(1) as u32).map_err(|e| mf_error("buffer", e))?;
        let mut data = std::ptr::null_mut();
        buffer
            .Lock(&mut data, None, None)
            .map_err(|e| mf_error("buffer", e))?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
        let _ = buffer.Unlock();
        buffer
            .SetCurrentLength(bytes.len() as u32)
            .map_err(|e| mf_error("buffer", e))?;
        let sample = MFCreateSample().map_err(|e| mf_error("sample", e))?;
        sample
            .AddBuffer(&buffer)
            .map_err(|e| mf_error("sample", e))?;
        // 100 ns units; only the order matters to the decoder.
        let _ = sample.SetSampleTime(time * 333_333);
        Ok(sample)
    }
}

/// Whether Windows can decode H.264 (`Err`: why; an "N" edition without the Media Feature Pack).
pub fn available() -> Result<(), String> {
    MediaFoundation::new(false).map(drop)
}
