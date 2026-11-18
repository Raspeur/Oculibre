//! The formats the camera offers, as DirectShow describes them (`AM_MEDIA_TYPE` +
//! `VIDEOINFOHEADER`), and the memory rules that go with them: types handed to the app are
//! allocated with `CoTaskMemAlloc`, the app frees them (`DeleteMediaType`).

use std::mem::{size_of, ManuallyDrop};

use windows::Win32::Foundation::{RECT, SIZE};
use windows::Win32::Graphics::Gdi::BITMAPINFOHEADER;
use windows::Win32::Media::DirectShow::VIDEO_STREAM_CONFIG_CAPS;
use windows::Win32::Media::MediaFoundation::{
    FORMAT_VideoInfo, MEDIATYPE_Video, AM_MEDIA_TYPE, MEDIASUBTYPE_YUY2, VIDEOINFOHEADER,
};
use windows::Win32::System::Com::{CoTaskMemAlloc, CoTaskMemFree};
use windows_core::GUID;

/// 100 ns units per second (DirectShow's time unit).
pub const UNITS: i64 = 10_000_000;

/// What the camera offers, best first: the size Oculibre writes (1920×1080), then smaller ones
/// for apps that want them. 30 or 60 pictures per second.
pub const SIZES: [(usize, usize); 3] = [(1920, 1080), (1280, 720), (640, 360)];
pub const DEFAULT_INTERVAL: i64 = UNITS / 30;
pub const FASTEST_INTERVAL: i64 = UNITS / 60;
const SLOWEST_INTERVAL: i64 = UNITS / 5;

/// A format the camera can give: size and time between pictures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    pub width: usize,
    pub height: usize,
    pub interval: i64,
}

impl Format {
    pub const DEFAULT: Format = Format {
        width: SIZES[0].0,
        height: SIZES[0].1,
        interval: DEFAULT_INTERVAL,
    };

    pub fn all() -> impl Iterator<Item = Format> {
        SIZES.iter().map(|&(width, height)| Format {
            width,
            height,
            interval: DEFAULT_INTERVAL,
        })
    }

    pub fn bytes(&self) -> usize {
        self.width * self.height * 2
    }

    fn header(&self) -> VIDEOINFOHEADER {
        VIDEOINFOHEADER {
            rcSource: RECT::default(),
            rcTarget: RECT::default(),
            dwBitRate: (self.bytes() as i64 * 8 * UNITS / self.interval) as u32,
            dwBitErrorRate: 0,
            AvgTimePerFrame: self.interval,
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: self.width as i32,
                biHeight: self.height as i32,
                biPlanes: 1,
                biBitCount: 16,
                biCompression: MEDIASUBTYPE_YUY2.data1,
                biSizeImage: self.bytes() as u32,
                ..Default::default()
            },
        }
    }

    /// The media type, to fill a structure the app gave (its `pbFormat` is ours to allocate).
    pub fn media_type(&self) -> AM_MEDIA_TYPE {
        // SAFETY: a block of the header's size, filled at once; the app frees it.
        let format = unsafe {
            let p = CoTaskMemAlloc(size_of::<VIDEOINFOHEADER>()) as *mut VIDEOINFOHEADER;
            if !p.is_null() {
                p.write(self.header());
            }
            p
        };
        AM_MEDIA_TYPE {
            majortype: MEDIATYPE_Video,
            subtype: MEDIASUBTYPE_YUY2,
            bFixedSizeSamples: true.into(),
            bTemporalCompression: false.into(),
            lSampleSize: self.bytes() as u32,
            formattype: FORMAT_VideoInfo,
            pUnk: ManuallyDrop::new(None),
            cbFormat: if format.is_null() {
                0
            } else {
                size_of::<VIDEOINFOHEADER>() as u32
            },
            pbFormat: format.cast(),
        }
    }

    /// A media type allocated whole for the app (GetFormat, GetStreamCaps, enumerators).
    pub fn new_media_type(&self) -> *mut AM_MEDIA_TYPE {
        // SAFETY: as above; the app frees both blocks.
        unsafe {
            let p = CoTaskMemAlloc(size_of::<AM_MEDIA_TYPE>()) as *mut AM_MEDIA_TYPE;
            if !p.is_null() {
                p.write(self.media_type());
            }
            p
        }
    }

    pub fn caps(&self) -> VIDEO_STREAM_CONFIG_CAPS {
        let size = SIZE {
            cx: self.width as i32,
            cy: self.height as i32,
        };
        let bits = |interval: i64| (self.bytes() as i64 * 8 * UNITS / interval) as i32;
        VIDEO_STREAM_CONFIG_CAPS {
            guid: FORMAT_VideoInfo,
            VideoStandard: 0,
            InputSize: size,
            MinCroppingSize: size,
            MaxCroppingSize: size,
            CropGranularityX: 1,
            CropGranularityY: 1,
            CropAlignX: 1,
            CropAlignY: 1,
            MinOutputSize: size,
            MaxOutputSize: size,
            OutputGranularityX: 0,
            OutputGranularityY: 0,
            StretchTapsX: 0,
            StretchTapsY: 0,
            ShrinkTapsX: 0,
            ShrinkTapsY: 0,
            MinFrameInterval: FASTEST_INTERVAL,
            MaxFrameInterval: SLOWEST_INTERVAL,
            MinBitsPerSecond: bits(SLOWEST_INTERVAL),
            MaxBitsPerSecond: bits(FASTEST_INTERVAL),
        }
    }

    /// The format an app asks for, if we can give it. `GUID_NULL` fields (a partial type, as in
    /// `Connect`) match anything; a missing interval means 30 per second.
    ///
    /// # Safety
    /// `mt` is a valid media type (its `pbFormat` holds `cbFormat` bytes).
    pub unsafe fn accept(mt: &AM_MEDIA_TYPE) -> Option<Format> {
        let any = GUID::zeroed();
        if (mt.majortype != MEDIATYPE_Video && mt.majortype != any)
            || (mt.subtype != MEDIASUBTYPE_YUY2 && mt.subtype != any)
        {
            return None;
        }
        if mt.formattype != FORMAT_VideoInfo
            || mt.pbFormat.is_null()
            || (mt.cbFormat as usize) < size_of::<VIDEOINFOHEADER>()
        {
            return None;
        }
        // SAFETY: checked above: a VIDEOINFOHEADER is there (maybe unaligned).
        let vih = unsafe { (mt.pbFormat as *const VIDEOINFOHEADER).read_unaligned() };
        let (w, h) = (vih.bmiHeader.biWidth, vih.bmiHeader.biHeight.abs());
        let size = SIZES
            .iter()
            .find(|&&(sw, sh)| (sw as i32, sh as i32) == (w, h))?;
        let interval = match vih.AvgTimePerFrame {
            0 => DEFAULT_INTERVAL,
            i if (FASTEST_INTERVAL..=SLOWEST_INTERVAL).contains(&i) => i,
            _ => return None,
        };
        Some(Format {
            width: size.0,
            height: size.1,
            interval,
        })
    }
}

/// Frees what a media type points to (not the structure itself), like `FreeMediaType`.
///
/// # Safety
/// `mt` was filled by us or by DirectShow (CoTaskMem blocks).
pub unsafe fn free_media_type_content(mt: &mut AM_MEDIA_TYPE) {
    // SAFETY: the caller's contract.
    unsafe {
        if !mt.pbFormat.is_null() {
            CoTaskMemFree(Some(mt.pbFormat as *const _));
            mt.pbFormat = std::ptr::null_mut();
            mt.cbFormat = 0;
        }
        ManuallyDrop::drop(&mut mt.pUnk);
        mt.pUnk = ManuallyDrop::new(None);
    }
}
