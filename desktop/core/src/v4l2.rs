//! Minimal v4l2loopback writer: set the output format, read back what the driver really
//! accepted, then `write()` exactly one frame per call.
//! Uses two ioctls (VIDIOC_G_FMT / VIDIOC_S_FMT), declared by hand to avoid a heavy dependency.
//!
//! v4l2loopback can refuse a new size (an app is reading the camera, or the format is
//! locked) and does so *silently*: S_FMT succeeds but returns the old size. So the size
//! actually in use is always read back, and the caller scales frames to it.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

const V4L2_BUF_TYPE_VIDEO_OUTPUT: u32 = 2;
const V4L2_FIELD_NONE: u32 = 1;
const V4L2_COLORSPACE_SRGB: u32 = 8;

const fn fourcc(code: &[u8; 4]) -> u32 {
    (code[0] as u32) | ((code[1] as u32) << 8) | ((code[2] as u32) << 16) | ((code[3] as u32) << 24)
}
const V4L2_PIX_FMT_YUYV: u32 = fourcc(b"YUYV");

/// `struct v4l2_pix_format` (linux/videodev2.h)
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct PixFormat {
    width: u32,
    height: u32,
    pixelformat: u32,
    field: u32,
    bytesperline: u32,
    sizeimage: u32,
    colorspace: u32,
    priv_: u32,
    flags: u32,
    ycbcr_enc: u32,
    quantization: u32,
    xfer_func: u32,
}

/// `struct v4l2_format`: a u32 type followed by a 200-byte, 8-byte-aligned union.
#[repr(C)]
struct Format {
    type_: u32,
    fmt: [u64; 25],
}

const _: () = assert!(std::mem::size_of::<Format>() == 208);
const _: () = assert!(std::mem::size_of::<PixFormat>() <= 200);

const fn iowr(nr: u64) -> u64 {
    (3u64 << 30) | ((std::mem::size_of::<Format>() as u64) << 16) | ((b'V' as u64) << 8) | nr
}
/// `_IOWR('V', 4, struct v4l2_format)`
const VIDIOC_G_FMT: u64 = iowr(4);
/// `_IOWR('V', 5, struct v4l2_format)`
const VIDIOC_S_FMT: u64 = iowr(5);

impl Format {
    fn output(pix: PixFormat) -> Self {
        let mut f = Format { type_: V4L2_BUF_TYPE_VIDEO_OUTPUT, fmt: [0; 25] };
        // SAFETY: PixFormat is plain old data and smaller than the union.
        unsafe { std::ptr::write_unaligned(f.fmt.as_mut_ptr().cast::<PixFormat>(), pix) };
        f
    }

    fn pix(&self) -> PixFormat {
        // SAFETY: the union starts with a v4l2_pix_format for VIDEO_OUTPUT.
        unsafe { std::ptr::read_unaligned(self.fmt.as_ptr().cast::<PixFormat>()) }
    }
}

pub struct OutputDevice {
    file: File,
    size: (usize, usize),
}

impl OutputDevice {
    /// Opens the device and asks for YUYV `width`×`height`.
    /// Check [`size`](Self::size): the driver may have kept another size.
    pub fn open(path: &Path, width: usize, height: usize) -> io::Result<Self> {
        let file = open_rw(path)?;
        let requested = PixFormat {
            width: width as u32,
            height: height as u32,
            pixelformat: V4L2_PIX_FMT_YUYV,
            field: V4L2_FIELD_NONE,
            bytesperline: (width * 2) as u32,
            sizeimage: (width * height * 2) as u32,
            colorspace: V4L2_COLORSPACE_SRGB,
            ..Default::default()
        };

        let mut fmt = Format::output(requested);
        // SAFETY: valid fd, correctly sized and initialised struct for this request.
        let ret = unsafe { libc::ioctl(file.as_raw_fd(), VIDIOC_S_FMT as _, &mut fmt as *mut Format) };
        if ret < 0 {
            // Refused outright (e.g. EBUSY): fall back to the format currently in use.
            let mut current = Format::output(PixFormat::default());
            // SAFETY: as above.
            let ret = unsafe {
                libc::ioctl(file.as_raw_fd(), VIDIOC_G_FMT as _, &mut current as *mut Format)
            };
            if ret < 0 {
                let err = io::Error::last_os_error();
                return Err(io::Error::new(
                    err.kind(),
                    format!("cannot set or read the format of {}: {err}", path.display()),
                ));
            }
            fmt = current;
        }

        let pix = fmt.pix();
        if pix.pixelformat != V4L2_PIX_FMT_YUYV || pix.width == 0 || pix.height == 0 {
            return Err(io::Error::other(format!(
                "{} is locked in a format other than YUYV ({}x{}, fourcc {:#x}) — \
                 close the apps using it, or reload it with: bash scripts/vcam-load.sh",
                path.display(),
                pix.width,
                pix.height,
                pix.pixelformat
            )));
        }
        Ok(Self { file, size: (pix.width as usize, pix.height as usize) })
    }

    /// The size frames must have (what the driver really uses).
    pub fn size(&self) -> (usize, usize) {
        self.size
    }

    /// Writes one YUYV frame. v4l2loopback treats every `write()` as one frame,
    /// so it must go out in a single call.
    pub fn write_frame(&mut self, frame: &[u8]) -> io::Result<()> {
        let n = self.file.write(frame)?;
        if n != frame.len() {
            return Err(io::Error::other(format!(
                "virtual camera accepted {n} of {} bytes (frame size mismatch)",
                frame.len()
            )));
        }
        Ok(())
    }
}

fn open_rw(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| io::Error::new(e.kind(), format!("cannot open {}: {e}", path.display())))
}

/// Fails early (permissions, wrong path) before waiting for the phone.
pub fn check_access(path: &Path) -> io::Result<()> {
    open_rw(path).map(drop)
}

/// Finds the v4l2loopback device: first one named "Oculibre", else any loopback device.
pub fn find_loopback() -> io::Result<PathBuf> {
    let mut fallback = None;
    let mut entries: Vec<_> = fs::read_dir("/sys/class/video4linux")
        .map(|it| it.filter_map(Result::ok).collect())
        .unwrap_or_default();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let name = fs::read_to_string(entry.path().join("name")).unwrap_or_default();
        let dev = Path::new("/dev").join(entry.file_name());
        if name.contains("Oculibre") {
            return Ok(dev);
        }
        let is_loopback = fs::read_link(entry.path().join("device/driver"))
            .map(|p| p.to_string_lossy().contains("v4l2loopback"))
            .unwrap_or(false)
            || name.contains("Dummy video device");
        if is_loopback && fallback.is_none() {
            fallback = Some(dev);
        }
    }
    fallback.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no v4l2loopback device found — run: bash scripts/vcam-load.sh",
        )
    })
}
