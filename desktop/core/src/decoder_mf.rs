//! Windows' own H.264 decoder (Media Foundation, #17): licensed with Windows, so Oculibre ships
//! no decoder there (`docs/research/windows.typ`). Software for now; D3D11 hardware decoding next.
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
    pub fn new() -> Result<Self, String> {
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
            if let Ok(attributes) = mft.GetAttributes() {
                let _ = attributes.SetUINT32(&MF_LOW_LATENCY, 1);
            }
            let input = MFCreateMediaType().map_err(|e| mf_error("media type", e))?;
            input
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                .and_then(|_| input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264))
                .and_then(|_| mft.SetInputType(0, &input, 0))
                .map_err(|e| mf_error("H.264 input", e))?;
            let mut decoder = Self {
                mft,
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
    fn copy(&self, sample: &IMFSample, out: &mut Picture) -> Result<(), String> {
        let (w, h) = self.visible;
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
    MediaFoundation::new().map(drop)
}
