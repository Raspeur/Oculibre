//! H.264 decoding (#11): hardware decoding through FFmpeg when the computer can (VA-API for AMD
//! and Intel, NVDEC through CUDA for NVIDIA's own driver), FFmpeg's software decoder otherwise,
//! OpenH264 as a last resort (built without FFmpeg, or FFmpeg failing).
//!
//! On a Ryzen 5 9600X, for a 4K frame: OpenH264 10.6 ms (26–34 ms on real camera streams), FFmpeg
//! software 5.0 ms, VA-API on a Radeon RX 7800 XT 5.2 ms including the copy back to the CPU, and
//! about the same whatever the scene and the bitrate.
//!
//! Every decoder writes the picture into a [`Picture`] (I420) for the output thread. No decoder
//! here holds frames back: one access unit in, its picture out (protocol §12).

use std::sync::OnceLock;

use crate::yuv::Picture;

/// `--decoder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// VA-API, else NVDEC, else FFmpeg software, else OpenH264.
    Auto,
    Vaapi,
    /// NVIDIA (proprietary driver). Not tested on a real card yet (#11).
    Nvdec,
    Software,
    OpenH264,
}

impl Choice {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "auto" => Self::Auto,
            "vaapi" => Self::Vaapi,
            "nvdec" => Self::Nvdec,
            "software" => Self::Software,
            "openh264" => Self::OpenH264,
            _ => return None,
        })
    }
}

static CHOICE: OnceLock<Choice> = OnceLock::new();

/// The decoder the user asked for, set once at start.
pub fn set_choice(choice: Choice) {
    let _ = CHOICE.set(choice);
}

fn choice() -> Choice {
    *CHOICE.get().unwrap_or(&Choice::Auto)
}

/// Prepares the environment of the VA-API library. Call it first thing in `main`, while the
/// process has a single thread (changing the environment later is not thread-safe).
///
/// - AMD's own driver stack (amdgpu-install) puts its VA-API driver in `/opt/amdgpu`, where libva
///   does not look by itself: point it there when the system has no driver of its own.
/// - libva prints "libva info" lines on every start: errors only.
pub fn prepare_environment() {
    const AMDGPU: &str = "/opt/amdgpu/lib/x86_64-linux-gnu/dri";
    const SYSTEM: &str = "/usr/lib/x86_64-linux-gnu/dri";
    let has_driver = |dir: &str| {
        std::fs::read_dir(dir).is_ok_and(|entries| {
            entries
                .flatten()
                .any(|e| e.file_name().to_string_lossy().ends_with("_drv_video.so"))
        })
    };
    if std::env::var_os("LIBVA_DRIVERS_PATH").is_none() && !has_driver(SYSTEM) && has_driver(AMDGPU)
    {
        std::env::set_var("LIBVA_DRIVERS_PATH", AMDGPU);
    }
    if std::env::var_os("LIBVA_MESSAGING_LEVEL").is_none() {
        std::env::set_var("LIBVA_MESSAGING_LEVEL", "1");
    }
}

/// A decoder of the chosen kind.
pub struct VideoDecoder {
    backend: Backend,
}

enum Backend {
    #[cfg(feature = "ffmpeg")]
    Ffmpeg(ffmpeg::Ffmpeg),
    OpenH264(Box<openh264::decoder::Decoder>),
}

impl VideoDecoder {
    /// A new decoder for a new stream, following `--decoder`.
    pub fn new() -> Result<Self, String> {
        Self::with(choice())
    }

    pub fn with(choice: Choice) -> Result<Self, String> {
        #[cfg(feature = "ffmpeg")]
        {
            use ffmpeg::Hardware;
            let devices: &[Hardware] = match choice {
                Choice::Auto => &[Hardware::Vaapi, Hardware::Cuda],
                Choice::Vaapi => &[Hardware::Vaapi],
                Choice::Nvdec => &[Hardware::Cuda],
                Choice::Software | Choice::OpenH264 => &[],
            };
            if choice != Choice::OpenH264 {
                match ffmpeg::Ffmpeg::new(devices) {
                    Ok(d) => {
                        if !devices.is_empty() && choice != Choice::Auto && d.hardware().is_none() {
                            return Err(format!(
                                "{} is not available on this computer",
                                if choice == Choice::Vaapi {
                                    "VA-API"
                                } else {
                                    "NVDEC (CUDA)"
                                }
                            ));
                        }
                        return Ok(Self {
                            backend: Backend::Ffmpeg(d),
                        });
                    }
                    Err(e) if choice != Choice::Auto => return Err(e),
                    Err(_) => {} // OpenH264 below
                }
            }
        }
        #[cfg(not(feature = "ffmpeg"))]
        if choice != Choice::OpenH264 && choice != Choice::Auto {
            return Err("this oculibre was built without FFmpeg: only openh264".into());
        }
        let d = openh264::decoder::Decoder::new().map_err(|e| e.to_string())?;
        Ok(Self {
            backend: Backend::OpenH264(Box::new(d)),
        })
    }

    /// "VA-API (hardware)", "NVDEC (hardware)", "FFmpeg (software)" or "OpenH264 (software)".
    pub fn name(&self) -> &'static str {
        match &self.backend {
            #[cfg(feature = "ffmpeg")]
            Backend::Ffmpeg(d) => match d.hardware() {
                Some(ffmpeg::Hardware::Vaapi) => "VA-API (hardware)",
                Some(ffmpeg::Hardware::Cuda) => "NVDEC (hardware)",
                None => "FFmpeg (software)",
            },
            Backend::OpenH264(_) => "OpenH264 (software)",
        }
    }

    /// Decodes one access unit (or SPS/PPS) into `out`. `Ok(true)`: `out` holds a new picture.
    pub fn decode(&mut self, au: &[u8], out: &mut Picture) -> Result<bool, String> {
        match &mut self.backend {
            #[cfg(feature = "ffmpeg")]
            Backend::Ffmpeg(d) => d.decode(au, out),
            Backend::OpenH264(d) => match d.decode(au) {
                Ok(Some(picture)) => {
                    out.copy_from(&picture);
                    Ok(true)
                }
                Ok(None) => Ok(false),
                Err(e) => Err(e.to_string()),
            },
        }
    }
}

#[cfg(feature = "ffmpeg")]
mod ffmpeg {
    use std::ffi::CStr;
    use std::ptr;

    use ffmpeg_sys_next as ff;

    use crate::yuv::Picture;

    /// Hardware decoders FFmpeg can use.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Hardware {
        /// AMD, Intel (and some others) on Linux.
        Vaapi,
        /// NVIDIA's proprietary driver (NVDEC). Not tested on a real card yet.
        Cuda,
    }

    impl Hardware {
        fn device_type(self) -> ff::AVHWDeviceType {
            match self {
                Self::Vaapi => ff::AVHWDeviceType::AV_HWDEVICE_TYPE_VAAPI,
                Self::Cuda => ff::AVHWDeviceType::AV_HWDEVICE_TYPE_CUDA,
            }
        }

        fn pixel_format(self) -> ff::AVPixelFormat {
            match self {
                Self::Vaapi => ff::AVPixelFormat::AV_PIX_FMT_VAAPI,
                Self::Cuda => ff::AVPixelFormat::AV_PIX_FMT_CUDA,
            }
        }
    }

    /// libavcodec's H.264 decoder, with a hardware device when there is one.
    pub struct Ffmpeg {
        ctx: *mut ff::AVCodecContext,
        packet: *mut ff::AVPacket,
        frame: *mut ff::AVFrame,
        /// Hardware frames copied back to the CPU (NV12).
        cpu_frame: *mut ff::AVFrame,
        device: *mut ff::AVBufferRef,
        /// The hardware device opened, if any.
        device_kind: Option<Hardware>,
        /// The last picture came from the hardware decoder (FFmpeg may refuse a stream on it).
        hardware: bool,
    }

    fn error_text(code: i32) -> String {
        let mut buf = [0 as libc::c_char; 128];
        // SAFETY: the buffer and its size are valid; av_strerror always NUL-terminates.
        unsafe {
            ff::av_strerror(code, buf.as_mut_ptr(), buf.len());
            CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned()
        }
    }

    /// FFmpeg offers the hardware format first when it can decode this stream on the device;
    /// otherwise (profile not supported…) we take the software one. The wanted hardware format is
    /// kept in the context's `opaque`.
    unsafe extern "C" fn pick_format(
        ctx: *mut ff::AVCodecContext,
        formats: *const ff::AVPixelFormat,
    ) -> ff::AVPixelFormat {
        let mut p = formats;
        let mut software = ff::AVPixelFormat::AV_PIX_FMT_NONE;
        // SAFETY: FFmpeg passes its context and a list terminated by AV_PIX_FMT_NONE.
        unsafe {
            let wanted = (*ctx).opaque as usize as i32;
            while *p != ff::AVPixelFormat::AV_PIX_FMT_NONE {
                if *p as i32 == wanted {
                    return *p;
                }
                if software == ff::AVPixelFormat::AV_PIX_FMT_NONE {
                    software = *p;
                }
                p = p.add(1);
            }
        }
        software
    }

    impl Ffmpeg {
        /// Tries the hardware `devices` in order; software decoding when none opens.
        pub fn new(devices: &[Hardware]) -> Result<Self, String> {
            // SAFETY: plain FFmpeg API calls; every pointer is checked and owned by `Self` (freed
            // in Drop, also on the error paths through `Self`'s Drop).
            unsafe {
                ff::av_log_set_level(ff::AV_LOG_QUIET);
                let codec = ff::avcodec_find_decoder(ff::AVCodecID::AV_CODEC_ID_H264);
                if codec.is_null() {
                    return Err("FFmpeg has no H.264 decoder".into());
                }
                let mut me = Self {
                    ctx: ff::avcodec_alloc_context3(codec),
                    packet: ff::av_packet_alloc(),
                    frame: ff::av_frame_alloc(),
                    cpu_frame: ff::av_frame_alloc(),
                    device: ptr::null_mut(),
                    device_kind: None,
                    hardware: false,
                };
                if me.ctx.is_null()
                    || me.packet.is_null()
                    || me.frame.is_null()
                    || me.cpu_frame.is_null()
                {
                    return Err("out of memory".into());
                }
                let ctx = &mut *me.ctx;
                // One access unit in, its picture out: no frame threading, no reordering delay.
                ctx.flags |= ff::AV_CODEC_FLAG_LOW_DELAY as i32;
                ctx.thread_count = 1;
                for &kind in devices {
                    let rc = ff::av_hwdevice_ctx_create(
                        &mut me.device,
                        kind.device_type(),
                        ptr::null(),
                        ptr::null_mut(),
                        0,
                    );
                    if rc >= 0 {
                        ctx.hw_device_ctx = ff::av_buffer_ref(me.device);
                        ctx.opaque = kind.pixel_format() as i32 as usize as *mut libc::c_void;
                        ctx.get_format = Some(pick_format);
                        // Phones send Baseline, hardware decoders list Constrained Baseline: the
                        // same thing in practice (no FMO/ASO in phone encoders).
                        ctx.hwaccel_flags |= ff::AV_HWACCEL_FLAG_ALLOW_PROFILE_MISMATCH;
                        me.device_kind = Some(kind);
                        me.hardware = true;
                        break;
                    }
                }
                let rc = ff::avcodec_open2(me.ctx, codec, ptr::null_mut());
                if rc < 0 {
                    return Err(format!("cannot open FFmpeg's decoder: {}", error_text(rc)));
                }
                Ok(me)
            }
        }

        /// The hardware decoder in use (or about to be, before the first picture).
        pub fn hardware(&self) -> Option<Hardware> {
            self.device_kind.filter(|_| self.hardware)
        }

        pub fn decode(&mut self, au: &[u8], out: &mut Picture) -> Result<bool, String> {
            // SAFETY: the packet borrows `au` only for avcodec_send_packet, which copies data
            // that is not reference-counted (with the padding FFmpeg needs). Frames are owned by
            // `self`; plane slices are built from FFmpeg's own data/linesize/width/height.
            unsafe {
                let packet = &mut *self.packet;
                packet.data = au.as_ptr() as *mut u8;
                packet.size = au.len() as i32;
                let rc = ff::avcodec_send_packet(self.ctx, self.packet);
                packet.data = ptr::null_mut();
                packet.size = 0;
                if rc < 0 && rc != -ff::EAGAIN {
                    return Err(error_text(rc));
                }
                let mut got = false;
                loop {
                    let rc = ff::avcodec_receive_frame(self.ctx, self.frame);
                    if rc == -ff::EAGAIN || rc == ff::AVERROR_EOF {
                        break;
                    }
                    if rc < 0 {
                        return Err(error_text(rc));
                    }
                    self.copy_out(out)?;
                    got = true;
                }
                Ok(got)
            }
        }

        /// Copies the decoded frame into `out`, through the CPU for a hardware frame.
        unsafe fn copy_out(&mut self, out: &mut Picture) -> Result<(), String> {
            // SAFETY: see `decode`.
            unsafe {
                let mut f = &*self.frame;
                if !f.hw_frames_ctx.is_null() {
                    self.hardware = true;
                    ff::av_frame_unref(self.cpu_frame);
                    let rc = ff::av_hwframe_transfer_data(self.cpu_frame, self.frame, 0);
                    if rc < 0 {
                        return Err(format!("hardware frame copy failed: {}", error_text(rc)));
                    }
                    f = &*self.cpu_frame;
                } else {
                    self.hardware = false; // the hardware refused this stream: software
                }
                let (w, h) = (f.width as usize, f.height as usize);
                let plane = |i: usize, rows: usize, row_bytes: usize| -> (&[u8], usize) {
                    let stride = f.linesize[i] as usize;
                    let len = stride * (rows.max(1) - 1) + row_bytes;
                    (std::slice::from_raw_parts(f.data[i], len), stride)
                };
                let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
                match f.format {
                    x if x == ff::AVPixelFormat::AV_PIX_FMT_NV12 as i32 => {
                        out.copy_nv12(w, h, plane(0, h, w), plane(1, ch, 2 * cw));
                    }
                    x if x == ff::AVPixelFormat::AV_PIX_FMT_YUV420P as i32
                        || x == ff::AVPixelFormat::AV_PIX_FMT_YUVJ420P as i32 =>
                    {
                        out.copy_i420(w, h, plane(0, h, w), plane(1, ch, cw), plane(2, ch, cw));
                    }
                    other => return Err(format!("unexpected picture format {other}")),
                }
                ff::av_frame_unref(self.frame);
                Ok(())
            }
        }
    }

    impl Drop for Ffmpeg {
        fn drop(&mut self) {
            // SAFETY: each pointer is null or owned by `self`; the free functions accept null.
            unsafe {
                ff::av_frame_free(&mut self.cpu_frame);
                ff::av_frame_free(&mut self.frame);
                ff::av_packet_free(&mut self.packet);
                ff::avcodec_free_context(&mut self.ctx);
                ff::av_buffer_unref(&mut self.device);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openh264::encoder::Encoder;
    use openh264::formats::{YUVBuffer, YUVSource};

    /// Access units of a small clip: SPS + PPS + IDR first, then P frames.
    fn clip(n: u8, w: usize, h: usize) -> Vec<Vec<u8>> {
        let mut encoder = Encoder::new().unwrap();
        (0..n)
            .map(|i| {
                let mut yuv = vec![i.wrapping_mul(17); w * h * 3 / 2];
                yuv[usize::from(i) * 7] = 255;
                encoder
                    .encode(&YUVBuffer::from_vec(yuv, w, h))
                    .unwrap()
                    .to_vec()
            })
            .collect()
    }

    /// Returns the decoder that really decoded (VA-API may fall back to software).
    fn decodes_every_frame_at_once(choice: Choice) -> Option<&'static str> {
        let Ok(mut d) = VideoDecoder::with(choice) else {
            eprintln!("{choice:?} not available here");
            return None;
        };
        let mut p = Picture::default();
        for (i, au) in clip(8, 320, 240).iter().enumerate() {
            // No delay: every access unit gives its picture right away.
            assert!(d.decode(au, &mut p).unwrap(), "{} frame {i}", d.name());
            assert_eq!(p.dimensions(), (320, 240));
        }
        // All decoders agree on the pixels (same stream): compare the last luma row to OpenH264.
        let mut reference = VideoDecoder::with(Choice::OpenH264).unwrap();
        let mut q = Picture::default();
        for au in clip(8, 320, 240) {
            reference.decode(&au, &mut q).unwrap();
        }
        let diff = p
            .y()
            .iter()
            .zip(q.y())
            .map(|(a, b)| (i32::from(*a) - i32::from(*b)).abs())
            .max()
            .unwrap();
        assert!(diff <= 2, "{} differs from OpenH264 by {diff}", d.name());
        Some(d.name())
    }

    #[test]
    fn openh264() {
        decodes_every_frame_at_once(Choice::OpenH264);
    }

    #[cfg(feature = "ffmpeg")]
    #[test]
    fn ffmpeg_software() {
        decodes_every_frame_at_once(Choice::Software);
    }

    /// Needs a VA-API device: `cargo test -- --ignored vaapi`.
    #[cfg(feature = "ffmpeg")]
    #[test]
    #[ignore]
    fn vaapi() {
        prepare_environment();
        let d = VideoDecoder::with(Choice::Vaapi).expect("VA-API");
        assert_eq!(d.name(), "VA-API (hardware)");
        drop(d);
        assert_eq!(
            decodes_every_frame_at_once(Choice::Vaapi),
            Some("VA-API (hardware)"),
            "VA-API fell back to software"
        );
    }

    /// Needs an NVIDIA card with the proprietary driver: `cargo test -- --ignored nvdec`.
    #[cfg(feature = "ffmpeg")]
    #[test]
    #[ignore]
    fn nvdec() {
        assert_eq!(
            decodes_every_frame_at_once(Choice::Nvdec),
            Some("NVDEC (hardware)"),
            "NVDEC fell back to software"
        );
    }

    #[test]
    fn choices() {
        assert_eq!(Choice::parse("vaapi"), Some(Choice::Vaapi));
        assert_eq!(Choice::parse("nvdec"), Some(Choice::Nvdec));
        assert_eq!(Choice::parse("gpu"), None);
    }
}
