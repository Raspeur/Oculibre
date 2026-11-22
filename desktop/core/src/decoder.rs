//! H.264 decoding (#11): hardware decoding through FFmpeg when the computer can (VA-API for AMD
//! and Intel, NVDEC through CUDA for NVIDIA's own driver), FFmpeg's software decoder otherwise,
//! OpenH264 as a last resort (built without FFmpeg, or FFmpeg failing). Both libraries are the
//! system's, loaded at run time (#18): Oculibre starts without them and ships no H.264 code.
//!
//! On a Ryzen 5 9600X, for a 4K frame: OpenH264 10.6 ms (26–34 ms on real camera streams), FFmpeg
//! software 5.0 ms, VA-API on a Radeon RX 7800 XT 5.2 ms including the copy back to the CPU, and
//! about the same whatever the scene and the bitrate.
//!
//! Every decoder writes the picture into a [`Picture`] (I420) for the output thread. No decoder
//! here holds frames back: one access unit in, its picture out (protocol §12).

use std::sync::Mutex;

use crate::yuv::Picture;

#[cfg(windows)]
#[path = "decoder_mf.rs"]
mod mf;

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

static CHOICE: Mutex<Choice> = Mutex::new(Choice::Auto);

/// The decoder the user asked for (`--decoder`, the app's settings). Takes effect at the next
/// stream: the decoder is created again for every stream.
pub fn set_choice(choice: Choice) {
    *CHOICE.lock().unwrap() = choice;
}

fn choice() -> Choice {
    *CHOICE.lock().unwrap()
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
    #[cfg(ffmpeg)]
    Ffmpeg(ffmpeg::Ffmpeg),
    #[cfg(windows)]
    MediaFoundation(mf::MediaFoundation),
    OpenH264(Box<openh264::decoder::Decoder>),
}

impl VideoDecoder {
    /// A new decoder for a new stream, following `--decoder`. When the chosen one is not there
    /// (OpenH264 not installed…), the automatic choice instead of no video at all: the name of
    /// the decoder really used is shown anyway.
    pub fn new() -> Result<Self, String> {
        let choice = choice();
        Self::with(choice).or_else(|e| {
            if choice == Choice::Auto {
                Err(e)
            } else {
                Self::with(Choice::Auto)
            }
        })
    }

    pub fn with(choice: Choice) -> Result<Self, String> {
        #[cfg(ffmpeg)]
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
        // Windows: its own decoder first (licensed with Windows), OpenH264 after.
        #[cfg(windows)]
        if matches!(choice, Choice::Auto | Choice::Software) {
            // Auto: the graphics card first, quietly software when it cannot.
            let hardware = (choice == Choice::Auto)
                .then(|| mf::MediaFoundation::new(true).ok())
                .flatten();
            match hardware.map_or_else(|| mf::MediaFoundation::new(false), Ok) {
                Ok(d) => {
                    return Ok(Self {
                        backend: Backend::MediaFoundation(d),
                    })
                }
                Err(e) if choice != Choice::Auto => return Err(e),
                Err(_) => {} // OpenH264 below
            }
        }
        // Windows: "graphics card" (vaapi/nvdec on Linux) = Media Foundation on Direct3D 11.
        #[cfg(windows)]
        if matches!(choice, Choice::Vaapi | Choice::Nvdec) {
            return mf::MediaFoundation::new(true).map(|d| Self {
                backend: Backend::MediaFoundation(d),
            });
        }
        #[cfg(all(not(ffmpeg), not(windows)))]
        if choice != Choice::OpenH264 && choice != Choice::Auto {
            return Err("this oculibre was built without FFmpeg: only openh264".into());
        }
        let d = openh264_decoder().map_err(|why| {
            if choice == Choice::Auto {
                // Neither FFmpeg (Windows: its own decoder) nor OpenH264: nothing can decode (503).
                if cfg!(windows) {
                    format!("no H.264 decoder on this computer ({why}): on an \"N\" edition of Windows, install the Media Feature Pack")
                } else {
                    format!("no H.264 decoder on this computer ({why}): install FFmpeg (libavcodec) or OpenH264 (libopenh264)")
                }
            } else {
                why
            }
        })?;
        Ok(Self {
            backend: Backend::OpenH264(Box::new(d)),
        })
    }

    /// Decoding on the graphics card.
    pub fn hardware(&self) -> bool {
        match &self.backend {
            #[cfg(ffmpeg)]
            Backend::Ffmpeg(d) => d.hardware().is_some(),
            #[cfg(windows)]
            Backend::MediaFoundation(d) => d.hardware(),
            Backend::OpenH264(_) => false,
        }
    }

    /// A software decoder to replace this one when the graphics card turns out too slow for the
    /// frame rate: only on the automatic choice (`--decoder vaapi` is the user's call).
    pub fn software_instead(&self) -> Option<Self> {
        (self.hardware() && choice() == Choice::Auto)
            .then(|| Self::with(Choice::Software).ok())
            .flatten()
    }

    /// "VA-API (hardware)", "NVDEC (hardware)", "FFmpeg (software)" or "OpenH264 (software)".
    pub fn name(&self) -> &'static str {
        match &self.backend {
            #[cfg(ffmpeg)]
            Backend::Ffmpeg(d) => match d.hardware() {
                Some(ffmpeg::Hardware::Vaapi) => "VA-API (hardware)",
                Some(ffmpeg::Hardware::Cuda) => "NVDEC (hardware)",
                None => "FFmpeg (software)",
            },
            #[cfg(windows)]
            Backend::MediaFoundation(d) if d.hardware() => "Media Foundation (hardware)",
            #[cfg(windows)]
            Backend::MediaFoundation(_) => "Media Foundation (software)",
            Backend::OpenH264(_) => "OpenH264 (software)",
        }
    }

    /// Decodes one access unit (or SPS/PPS) into `out`. `Ok(true)`: `out` holds a new picture.
    pub fn decode(&mut self, au: &[u8], out: &mut Picture) -> Result<bool, String> {
        match &mut self.backend {
            #[cfg(ffmpeg)]
            Backend::Ffmpeg(d) => d.decode(au, out),
            #[cfg(windows)]
            Backend::MediaFoundation(d) => d.decode(au, out),
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

/// The system's OpenH264: the one version the `openh264` crate is written for (OpenH264 2.6,
/// ABI 8; Ubuntu's package libopenh264-8, Cisco's binary libopenh264-2.6.0-linux64.8.so).
#[cfg(all(any(not(feature = "bundled-openh264"), test), not(windows)))]
const OPENH264_SONAME: &str = "libopenh264.so.8";
/// Cisco's own binary for Windows (the one its patent licence covers), next to Oculibre.
#[cfg(all(any(not(feature = "bundled-openh264"), test), windows))]
const OPENH264_SONAME: &str = "openh264-2.6.0-win64.dll";

/// OpenH264's decoder. Oculibre ships no H.264 code: the library is the system's, loaded at run
/// time (#18). Only `bundled-openh264` builds, and the tests, use a copy compiled in.
fn openh264_decoder() -> Result<openh264::decoder::Decoder, String> {
    use openh264::decoder::{Decoder, DecoderConfig};
    Decoder::with_api_config(openh264_api()?, DecoderConfig::new()).map_err(|e| e.to_string())
}

#[cfg(any(feature = "bundled-openh264", test))]
fn openh264_api() -> Result<openh264::OpenH264API, String> {
    Ok(openh264::OpenH264API::from_source())
}

#[cfg(not(any(feature = "bundled-openh264", test)))]
fn openh264_api() -> Result<openh264::OpenH264API, String> {
    system_openh264()
}

/// The system's libopenh264, by its soname (searched like any library: ld.so.conf paths).
/// Not checked against Cisco's hashes: a distribution's own build is as good, as long as it is
/// the same ABI, which the soname guarantees.
#[cfg(any(not(feature = "bundled-openh264"), test))]
fn system_openh264() -> Result<openh264::OpenH264API, String> {
    // SAFETY: libopenh264.so.8 is OpenH264 2.6's ABI, the one openh264-sys2 0.9 is written for.
    unsafe { openh264::OpenH264API::from_blob_path_unchecked(OPENH264_SONAME) }
        .map_err(|_| format!("{OPENH264_SONAME} is not installed"))
}

/// Whether Windows' own H.264 decoder (Media Foundation) can be created (`Err`: why).
#[cfg(windows)]
pub fn media_foundation_available() -> Result<(), String> {
    mf::available()
}

/// Whether OpenH264 could be loaded (`Err`: why). With neither it nor FFmpeg, nothing decodes.
pub fn openh264_available() -> Result<(), String> {
    openh264_decoder().map(drop)
}

/// FFmpeg as found on this computer (About screen): its licence depends on how the distribution
/// built it (LGPL, or GPL with GPL parts enabled).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FfmpegInfo {
    /// "8.0.1-3ubuntu2": FFmpeg's own version string.
    pub version: String,
    /// "62.11.100"
    pub libavcodec: String,
    /// "LGPL version 2.1 or later", "GPL version 2 or later"…
    pub license: String,
}

pub fn ffmpeg_info() -> Result<FfmpegInfo, String> {
    #[cfg(ffmpeg)]
    return ffmpeg::info();
    #[cfg(not(ffmpeg))]
    Err("this Oculibre was built without FFmpeg".into())
}

/// "2.6.0": the OpenH264 library in use (About screen).
pub fn openh264_version() -> Result<String, String> {
    use openh264_sys2::API;
    let api = openh264_api()?;
    // SAFETY: a plain getter of the loaded library.
    let v = unsafe { api.WelsGetCodecVersion() };
    Ok(format!("{}.{}.{}", v.uMajor, v.uMinor, v.uRevision))
}

/// Whether FFmpeg's libavcodec could be loaded (`Err`: why, e.g. "libavcodec.so.62 is not
/// installed"). Without it, decoding falls back to OpenH264 (problem 502).
pub fn ffmpeg_available() -> Result<(), String> {
    #[cfg(ffmpeg)]
    return ffmpeg::lib().map(drop);
    #[cfg(not(ffmpeg))]
    Err("this Oculibre was built without FFmpeg".into())
}

#[cfg(ffmpeg)]
mod ffmpeg {
    use std::ffi::{c_char, c_int, c_void, CStr};
    use std::ptr;
    use std::sync::OnceLock;

    use ff::{
        AVBufferRef, AVCodec, AVCodecContext, AVCodecID, AVDictionary, AVFrame, AVHWDeviceType,
        AVPacket,
    };
    use ffmpeg_sys_next as ff;

    use crate::yuv::Picture;

    /// FFmpeg's functions, found at run time in libavcodec (and the libavutil it depends on)
    /// instead of being linked: Oculibre starts on a computer without FFmpeg, and uses OpenH264
    /// there. `ffmpeg-sys-next` still gives the types: the structures' layout belongs to one major
    /// version, so only the library of that version (`libavcodec.so.<major>`) is loaded.
    ///
    /// No function of `ffmpeg-sys-next` is called directly anywhere: the linker (`--as-needed`)
    /// then leaves libavcodec out of the binary's needed libraries. The test `same_signatures`
    /// checks that each type below is exactly the one of `ffmpeg-sys-next`.
    macro_rules! functions {
        ($( fn $name:ident($($arg:ty),*) $(-> $ret:ty)?; )*) => {
            #[allow(non_snake_case)]
            pub struct Lib {
                $( pub $name: unsafe extern "C" fn($($arg),*) $(-> $ret)?, )*
            }

            impl Lib {
                /// Every function from the library `handle` (dlsym also searches its
                /// dependencies: libavutil).
                unsafe fn find(handle: *mut c_void) -> Result<Self, String> {
                    // SAFETY: the caller gives a live dlopen handle; each symbol is cast to the
                    // type of its C declaration (checked by `same_signatures`).
                    unsafe {
                        Ok(Self {
                            $( $name: {
                                let symbol = libc::dlsym(handle, concat!(stringify!($name), "\0").as_ptr().cast());
                                if symbol.is_null() {
                                    return Err(format!("{} has no {}", soname(), stringify!($name)));
                                }
                                std::mem::transmute::<*mut c_void, unsafe extern "C" fn($($arg),*) $(-> $ret)?>(symbol)
                            }, )*
                        })
                    }
                }
            }

            /// Compiled only: each function type equals the declaration in ffmpeg-sys-next (a
            /// test binary may link FFmpeg, the real one never does).
            #[cfg(test)]
            #[allow(dead_code)]
            fn same_signatures() {
                $( let _: unsafe extern "C" fn($($arg),*) $(-> $ret)? = ff::$name; )*
            }
        };
    }

    functions! {
        fn av_strerror(c_int, *mut c_char, usize) -> c_int;
        fn av_log_set_level(c_int);
        fn avcodec_find_decoder(AVCodecID) -> *const AVCodec;
        fn avcodec_alloc_context3(*const AVCodec) -> *mut AVCodecContext;
        fn av_packet_alloc() -> *mut AVPacket;
        fn av_frame_alloc() -> *mut AVFrame;
        fn av_hwdevice_ctx_create(*mut *mut AVBufferRef, AVHWDeviceType, *const c_char, *mut AVDictionary, c_int) -> c_int;
        fn av_buffer_ref(*const AVBufferRef) -> *mut AVBufferRef;
        fn avcodec_open2(*mut AVCodecContext, *const AVCodec, *mut *mut AVDictionary) -> c_int;
        fn avcodec_send_packet(*mut AVCodecContext, *const AVPacket) -> c_int;
        fn avcodec_receive_frame(*mut AVCodecContext, *mut AVFrame) -> c_int;
        fn av_frame_unref(*mut AVFrame);
        fn av_hwframe_transfer_data(*mut AVFrame, *const AVFrame, c_int) -> c_int;
        fn av_frame_free(*mut *mut AVFrame);
        fn av_packet_free(*mut *mut AVPacket);
        fn avcodec_free_context(*mut *mut AVCodecContext);
        fn av_buffer_unref(*mut *mut AVBufferRef);
        fn av_version_info() -> *const c_char;
        fn avcodec_version() -> std::ffi::c_uint;
        fn avcodec_license() -> *const c_char;
    }

    /// FFmpeg as found on this computer.
    pub fn info() -> Result<super::FfmpegInfo, String> {
        let lib = lib()?;
        let text = |p: *const c_char| {
            // SAFETY: FFmpeg returns static NUL-terminated strings (or null, checked).
            if p.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
            }
        };
        // SAFETY: plain getters of the loaded library.
        let (version, packed, license) = unsafe {
            (
                text((lib.av_version_info)()),
                (lib.avcodec_version)(),
                text((lib.avcodec_license)()),
            )
        };
        Ok(super::FfmpegInfo {
            version,
            libavcodec: format!(
                "{}.{}.{}",
                packed >> 16,
                (packed >> 8) & 0xff,
                packed & 0xff
            ),
            license,
        })
    }

    /// "libavcodec.so.62": the major version Oculibre was built against (FFmpeg 8).
    pub fn soname() -> String {
        format!("libavcodec.so.{}", ff::LIBAVCODEC_VERSION_MAJOR)
    }

    /// The library, loaded once (and never unloaded).
    pub fn lib() -> Result<&'static Lib, String> {
        static LIB: OnceLock<Result<Lib, String>> = OnceLock::new();
        LIB.get_or_init(|| {
            let name = format!("{}\0", soname());
            // SAFETY: a NUL-terminated name; the handle stays open for the whole process, so the
            // function pointers taken from it stay valid.
            unsafe {
                let handle = libc::dlopen(name.as_ptr().cast(), libc::RTLD_NOW | libc::RTLD_LOCAL);
                if handle.is_null() {
                    return Err(format!("{} is not installed", soname()));
                }
                Lib::find(handle)
            }
        })
        .as_ref()
        .map_err(Clone::clone)
    }

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
        lib: &'static Lib,
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

    fn error_text(lib: &Lib, code: i32) -> String {
        let mut buf = [0 as libc::c_char; 128];
        // SAFETY: the buffer and its size are valid; av_strerror always NUL-terminates.
        unsafe {
            (lib.av_strerror)(code, buf.as_mut_ptr(), buf.len());
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
            let lib = lib()?;
            // SAFETY: plain FFmpeg API calls; every pointer is checked and owned by `Self` (freed
            // in Drop, also on the error paths through `Self`'s Drop).
            unsafe {
                (lib.av_log_set_level)(ff::AV_LOG_QUIET);
                let codec = (lib.avcodec_find_decoder)(ff::AVCodecID::AV_CODEC_ID_H264);
                if codec.is_null() {
                    return Err("FFmpeg has no H.264 decoder".into());
                }
                let mut me = Self {
                    lib,
                    ctx: (lib.avcodec_alloc_context3)(codec),
                    packet: (lib.av_packet_alloc)(),
                    frame: (lib.av_frame_alloc)(),
                    cpu_frame: (lib.av_frame_alloc)(),
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
                    let rc = (lib.av_hwdevice_ctx_create)(
                        &mut me.device,
                        kind.device_type(),
                        ptr::null(),
                        ptr::null_mut(),
                        0,
                    );
                    if rc >= 0 {
                        ctx.hw_device_ctx = (lib.av_buffer_ref)(me.device);
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
                let rc = (lib.avcodec_open2)(me.ctx, codec, ptr::null_mut());
                if rc < 0 {
                    return Err(format!(
                        "cannot open FFmpeg's decoder: {}",
                        error_text(lib, rc)
                    ));
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
            let lib = self.lib;
            unsafe {
                let packet = &mut *self.packet;
                packet.data = au.as_ptr() as *mut u8;
                packet.size = au.len() as i32;
                let rc = (lib.avcodec_send_packet)(self.ctx, self.packet);
                packet.data = ptr::null_mut();
                packet.size = 0;
                if rc < 0 && rc != -ff::EAGAIN {
                    return Err(error_text(lib, rc));
                }
                let mut got = false;
                loop {
                    let rc = (lib.avcodec_receive_frame)(self.ctx, self.frame);
                    if rc == -ff::EAGAIN || rc == ff::AVERROR_EOF {
                        break;
                    }
                    if rc < 0 {
                        return Err(error_text(lib, rc));
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
            let lib = self.lib;
            unsafe {
                let mut f = &*self.frame;
                if !f.hw_frames_ctx.is_null() {
                    self.hardware = true;
                    (lib.av_frame_unref)(self.cpu_frame);
                    let rc = (lib.av_hwframe_transfer_data)(self.cpu_frame, self.frame, 0);
                    if rc < 0 {
                        return Err(format!(
                            "hardware frame copy failed: {}",
                            error_text(lib, rc)
                        ));
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
                (lib.av_frame_unref)(self.frame);
                Ok(())
            }
        }
    }

    impl Drop for Ffmpeg {
        fn drop(&mut self) {
            // SAFETY: each pointer is null or owned by `self`; the free functions accept null.
            let lib = self.lib;
            unsafe {
                (lib.av_frame_free)(&mut self.cpu_frame);
                (lib.av_frame_free)(&mut self.frame);
                (lib.av_packet_free)(&mut self.packet);
                (lib.avcodec_free_context)(&mut self.ctx);
                (lib.av_buffer_unref)(&mut self.device);
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
        decodes_every_frame_at_once_sized(choice, 320, 240)
    }

    /// `h` not a multiple of 16 checks the crop (coded 368 rows, 360 shown).
    fn decodes_every_frame_at_once_sized(
        choice: Choice,
        w: usize,
        h: usize,
    ) -> Option<&'static str> {
        let mut d = match VideoDecoder::with(choice) {
            Ok(d) => d,
            Err(why) => {
                eprintln!("{choice:?} not available here: {why}");
                return None;
            }
        };
        let mut p = Picture::default();
        for (i, au) in clip(8, w, h).iter().enumerate() {
            // No delay: every access unit gives its picture right away.
            assert!(d.decode(au, &mut p).unwrap(), "{} frame {i}", d.name());
            assert_eq!(p.dimensions(), (w, h));
        }
        // All decoders agree on the pixels (same stream): compare the last luma row to OpenH264.
        let mut reference = VideoDecoder::with(Choice::OpenH264).unwrap();
        let mut q = Picture::default();
        for au in clip(8, w, h) {
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

    /// The system's libopenh264 (what published builds use), when this computer has it.
    #[test]
    fn system_openh264() {
        let Ok(api) = super::system_openh264() else {
            eprintln!("{OPENH264_SONAME} not installed here");
            return;
        };
        let mut d = openh264::decoder::Decoder::with_api_config(
            api,
            openh264::decoder::DecoderConfig::new(),
        )
        .unwrap();
        for (i, au) in clip(8, 320, 240).iter().enumerate() {
            let picture = d.decode(au).unwrap();
            assert!(picture.is_some(), "frame {i}");
        }
    }

    #[cfg(ffmpeg)]
    #[test]
    fn ffmpeg_software() {
        decodes_every_frame_at_once(Choice::Software);
    }

    /// Needs a VA-API device: `cargo test -- --ignored vaapi`.
    #[cfg(ffmpeg)]
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
    #[cfg(ffmpeg)]
    #[test]
    #[ignore]
    fn nvdec() {
        assert_eq!(
            decodes_every_frame_at_once(Choice::Nvdec),
            Some("NVDEC (hardware)"),
            "NVDEC fell back to software"
        );
    }

    /// Windows' own decoder (software): no delay, cropped right, same pixels as OpenH264.
    #[cfg(windows)]
    #[test]
    fn media_foundation() {
        for (w, h) in [(320, 240), (640, 360)] {
            assert_eq!(
                decodes_every_frame_at_once_sized(Choice::Software, w, h),
                Some("Media Foundation (software)")
            );
        }
        // Auto: the graphics card when there is one (not in a virtual machine), else software.
        assert!(VideoDecoder::new()
            .unwrap()
            .name()
            .starts_with("Media Foundation"));
    }

    /// On a real graphics card (not in a VM): `cargo test -- --ignored media_foundation_hardware`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn media_foundation_hardware() {
        for (w, h) in [(320, 240), (640, 360), (1920, 1080)] {
            assert_eq!(
                decodes_every_frame_at_once_sized(Choice::Vaapi, w, h),
                Some("Media Foundation (hardware)"),
                "no hardware decoding here"
            );
        }
    }

    /// Time per 1080p picture: `cargo test --release -- --ignored --nocapture decode_speed`.
    #[test]
    #[ignore]
    fn decode_speed() {
        let clip = clip(60, 1920, 1080);
        for choice in [Choice::Auto, Choice::OpenH264] {
            let mut d = VideoDecoder::with(choice).unwrap();
            let mut p = Picture::default();
            let mut times: Vec<_> = clip
                .iter()
                .map(|au| {
                    let t = std::time::Instant::now();
                    d.decode(au, &mut p).unwrap();
                    t.elapsed()
                })
                .skip(1)
                .collect();
            times.sort();
            eprintln!(
                "{}: median {:?} per 1080p picture",
                d.name(),
                times[times.len() / 2]
            );
        }
    }

    #[test]
    fn choices() {
        assert_eq!(Choice::parse("vaapi"), Some(Choice::Vaapi));
        assert_eq!(Choice::parse("nvdec"), Some(Choice::Nvdec));
        assert_eq!(Choice::parse("gpu"), None);
    }
}
