//! Minimal v4l2loopback writer: set the output format once, then `write()` raw frames.
//! Uses a single ioctl (VIDIOC_S_FMT), declared by hand to avoid a heavy dependency.

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
#[derive(Default)]
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

/// `_IOWR('V', 5, struct v4l2_format)`
const VIDIOC_S_FMT: u64 =
    (3u64 << 30) | ((std::mem::size_of::<Format>() as u64) << 16) | ((b'V' as u64) << 8) | 5;

pub struct OutputDevice {
    file: File,
    path: PathBuf,
    format: Option<(u32, u32)>,
}

impl OutputDevice {
    pub fn open(path: &Path) -> io::Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| {
                io::Error::new(e.kind(), format!("cannot open {}: {e}", path.display()))
            })?;
        Ok(Self {
            file,
            path: path.to_owned(),
            format: None,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Sets YUYV `width`×`height` the first time, or when the resolution changes.
    pub fn ensure_format(&mut self, width: u32, height: u32) -> io::Result<()> {
        if self.format == Some((width, height)) {
            return Ok(());
        }
        let pix = PixFormat {
            width,
            height,
            pixelformat: V4L2_PIX_FMT_YUYV,
            field: V4L2_FIELD_NONE,
            bytesperline: width * 2,
            sizeimage: width * height * 2,
            colorspace: V4L2_COLORSPACE_SRGB,
            ..Default::default()
        };
        let mut fmt = Format {
            type_: V4L2_BUF_TYPE_VIDEO_OUTPUT,
            fmt: [0; 25],
        };
        // SAFETY: PixFormat is plain old data and smaller than the union.
        unsafe {
            std::ptr::write_unaligned(fmt.fmt.as_mut_ptr().cast::<PixFormat>(), pix);
        }
        // SAFETY: valid fd, correctly sized and initialised struct for this request.
        let ret = unsafe {
            libc::ioctl(
                self.file.as_raw_fd(),
                VIDIOC_S_FMT as _,
                &mut fmt as *mut Format,
            )
        };
        if ret < 0 {
            let err = io::Error::last_os_error();
            return Err(io::Error::new(
                err.kind(),
                format!(
                    "VIDIOC_S_FMT {width}x{height} on {} failed: {err} \
                     (is the camera already open in an app at another resolution?)",
                    self.path.display()
                ),
            ));
        }
        self.format = Some((width, height));
        Ok(())
    }

    pub fn write_frame(&mut self, frame: &[u8]) -> io::Result<()> {
        self.file.write_all(frame)
    }
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
