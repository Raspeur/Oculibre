//! The picture Oculibre hands to its Windows virtual cameras (#17), through shared memory.
//!
//! One writer (Oculibre), any number of readers (the DirectShow filter is loaded into every app
//! that opens the camera: OBS, Zoom, Chrome…). One picture, the latest: readers never wait for
//! the writer and the writer never waits for readers, so a slow app cannot slow the phone down.
//!
//! Layout (version 1), all little-endian, in a block named [`NAME`]:
//!
//! | offset | field | |
//! |---|---|---|
//! | 0 | magic `OCVC` | |
//! | 4 | version (1) | |
//! | 8 | width, height (u32 each) | of the picture now in the block |
//! | 16 | sequence (u64) | odd while the writer writes, even when the picture is whole |
//! | 24 | alive (u64) | milliseconds since the writer started, updated with each picture |
//! | 32 | frames (u64) | pictures written so far |
//! | [`HEADER`] | the picture, YUY2 (Y0 U Y1 V), rows of `width * 2` bytes | |
//!
//! Readers copy the picture, then check that the sequence did not change meanwhile (a
//! *seqlock*): no lock shared with other processes, so a reader that crashes cannot block
//! anyone.

use std::sync::atomic::{fence, AtomicU64, Ordering};

pub mod picture;

/// The block's name. `Local\`: the user's session, where the apps that load the DirectShow
/// filter run.
pub const NAME: &str = "Local\\OculibreVirtualCamera";
/// The Media Foundation camera's block. Its source runs in a Windows service (Frame Server, in
/// session 0), which cannot see the user's `Local\` objects: so a `Global\` block, which the
/// source creates (a normal user may not create `Global\` objects) and Oculibre opens to write.
pub const GLOBAL_NAME: &str = "Global\\OculibreVirtualCameraMF";
/// Who may use the global block: the system and services fully, logged-in users read and write.
pub const GLOBAL_SDDL: &str = "D:(A;;GA;;;SY)(A;;GA;;;LS)(A;;GA;;;BA)(A;;GRGW;;;AU)";
pub const MAGIC: u32 = u32::from_le_bytes(*b"OCVC");
pub const VERSION: u32 = 1;
/// Bytes before the picture.
pub const HEADER: usize = 4096;
/// Largest picture: 4K, either way round.
pub const MAX_PIXELS: usize = 3840 * 2160;
/// Size of the whole block.
pub const SIZE: usize = HEADER + MAX_PIXELS * 2;

/// The header at the start of the block.
#[repr(C)]
struct Header {
    magic: u32,
    version: u32,
    width: u32,
    height: u32,
    sequence: AtomicU64,
    alive: AtomicU64,
    frames: AtomicU64,
}

/// The block, wherever it lives (shared memory on Windows, a plain buffer in tests).
pub struct Frames {
    base: *mut u8,
}

// SAFETY: the block is made for concurrent access: atomics for the sequence, a seqlock for the
// rest (readers check that what they copied did not change underneath).
unsafe impl Send for Frames {}
unsafe impl Sync for Frames {}

impl Frames {
    /// # Safety
    /// `base` points to [`SIZE`] bytes, aligned to 8, that stay valid while `Frames` lives.
    pub unsafe fn new(base: *mut u8) -> Self {
        Self { base }
    }

    fn header(&self) -> &Header {
        // SAFETY: `new`'s contract; the header is plain data and atomics.
        unsafe { &*(self.base as *const Header) }
    }

    /// Writer: marks the block as ours (once, when creating it).
    pub fn init(&self) {
        // SAFETY: the writer owns the header's plain fields.
        unsafe {
            let h = self.base as *mut Header;
            (*h).version = VERSION;
            (*h).width = 0;
            (*h).height = 0;
            fence(Ordering::Release);
            (*h).magic = MAGIC;
        }
    }

    /// Whether a writer of our version set the block up.
    pub fn valid(&self) -> bool {
        let h = self.header();
        // SAFETY: plain reads of the header.
        unsafe {
            std::ptr::read_volatile(&h.magic) == MAGIC
                && std::ptr::read_volatile(&h.version) == VERSION
        }
    }

    /// Writer: a new picture, YUY2, `width * height * 2` bytes. `now_ms`: the writer's clock.
    pub fn write(&self, width: usize, height: usize, yuy2: &[u8], now_ms: u64) {
        let len = width * height * 2;
        assert!(
            width * height <= MAX_PIXELS && yuy2.len() >= len,
            "picture too big"
        );
        let h = self.header();
        let seq = h.sequence.load(Ordering::Relaxed);
        h.sequence.store(seq | 1, Ordering::Relaxed); // odd: writing
        fence(Ordering::Release);
        // SAFETY: within the block (checked above); readers detect the overlap by the sequence.
        unsafe {
            let hp = self.base as *mut Header;
            std::ptr::write_volatile(&mut (*hp).width, width as u32);
            std::ptr::write_volatile(&mut (*hp).height, height as u32);
            std::ptr::copy_nonoverlapping(yuy2.as_ptr(), self.base.add(HEADER), len);
        }
        h.alive.store(now_ms, Ordering::Relaxed);
        h.frames.fetch_add(1, Ordering::Relaxed);
        h.sequence.store((seq | 1) + 1, Ordering::Release); // even: whole
    }

    /// Writer: still there, no new picture (the phone is away).
    pub fn heartbeat(&self, now_ms: u64) {
        self.header().alive.store(now_ms, Ordering::Relaxed);
    }

    /// Reader: pictures written so far (changes when there is a new one).
    pub fn frames(&self) -> u64 {
        self.header().frames.load(Ordering::Acquire)
    }

    /// Reader: the writer's clock at its last sign of life.
    pub fn alive(&self) -> u64 {
        self.header().alive.load(Ordering::Relaxed)
    }

    /// Reader: copies the latest whole picture into `out` (resized). `None`: no picture yet, or
    /// the writer kept changing it (try again at the next frame).
    pub fn read(&self, out: &mut Vec<u8>) -> Option<(usize, usize)> {
        let h = self.header();
        // Writing a 1080p picture takes about a millisecond: retry for up to 10 ms (4 quick
        // tries were not enough at 60 pictures a second: the camera then showed the waiting
        // picture for one frame, a visible flash).
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(10);
        while std::time::Instant::now() < deadline {
            let before = h.sequence.load(Ordering::Acquire);
            if before & 1 == 1 {
                std::thread::yield_now();
                continue;
            }
            // SAFETY: reads within the block; a torn copy is detected below and thrown away.
            let (w, hh) = unsafe {
                let hp = self.base as *const Header;
                (
                    std::ptr::read_volatile(&(*hp).width) as usize,
                    std::ptr::read_volatile(&(*hp).height) as usize,
                )
            };
            if w == 0 || hh == 0 || w * hh > MAX_PIXELS {
                return None;
            }
            let len = w * hh * 2;
            out.resize(len, 0);
            // SAFETY: as above.
            unsafe {
                std::ptr::copy_nonoverlapping(self.base.add(HEADER), out.as_mut_ptr(), len);
            }
            fence(Ordering::Acquire);
            if h.sequence.load(Ordering::Relaxed) == before {
                return Some((w, hh));
            }
        }
        None
    }
}

/// The latest whole picture a camera got: a failed read (the writer kept changing it) keeps the
/// previous one instead of showing nothing.
#[derive(Default)]
pub struct LastPicture {
    frame: Vec<u8>,
    scratch: Vec<u8>,
    size: Option<(usize, usize)>,
}

impl LastPicture {
    /// Reads a new picture if it can; returns the latest whole one (`None`: never had one).
    pub fn update(&mut self, frames: &Frames) -> Option<(&[u8], usize, usize)> {
        if let Some(size) = frames.read(&mut self.scratch) {
            std::mem::swap(&mut self.frame, &mut self.scratch);
            self.size = Some(size);
        }
        self.size.map(|(w, h)| (&self.frame[..w * h * 2], w, h))
    }

    /// Forgets the picture (Oculibre stopped sending: the next one starts fresh).
    pub fn clear(&mut self) {
        self.size = None;
    }
}

#[cfg(windows)]
mod windows_shm {
    use super::*;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{
        CloseHandle, LocalFree, HANDLE, HLOCAL, INVALID_HANDLE_VALUE,
    };
    use windows::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows::Win32::System::Memory::{
        CreateFileMappingW, MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP,
        FILE_MAP_ALL_ACCESS, FILE_MAP_READ, FILE_MAP_WRITE, MEMORY_MAPPED_VIEW_ADDRESS,
        PAGE_READWRITE,
    };

    /// The block mapped into this process.
    pub struct Shared {
        mapping: HANDLE,
        view: MEMORY_MAPPED_VIEW_ADDRESS,
        frames: Frames,
    }

    // SAFETY: handles and a view usable from any thread; Frames is Sync.
    unsafe impl Send for Shared {}
    unsafe impl Sync for Shared {}

    impl Shared {
        /// Oculibre: creates the block (or opens it if a previous Oculibre left it, while an app
        /// still has the camera open).
        pub fn create() -> Result<Self, String> {
            Self::create_named(NAME, None)
        }

        /// The Media Foundation source (in the Frame Server service): creates the global block
        /// with [`GLOBAL_SDDL`], so that Oculibre, in the user's session, can write into it.
        pub fn create_global() -> Result<Self, String> {
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            // SAFETY: a constant SDDL string; the descriptor is freed below.
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    &HSTRING::from(GLOBAL_SDDL),
                    SDDL_REVISION_1,
                    &mut descriptor,
                    None,
                )
                .map_err(|e| format!("security descriptor: {e}"))?;
            }
            let attributes = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: descriptor.0,
                bInheritHandle: false.into(),
            };
            let result = Self::create_named(GLOBAL_NAME, Some(&attributes));
            // SAFETY: allocated by the conversion above, no longer used.
            unsafe {
                let _ = LocalFree(Some(HLOCAL(descriptor.0)));
            }
            result
        }

        fn create_named(
            name: &str,
            attributes: Option<&SECURITY_ATTRIBUTES>,
        ) -> Result<Self, String> {
            // SAFETY: a named, page-file backed mapping of SIZE bytes, mapped whole.
            unsafe {
                let mapping = CreateFileMappingW(
                    INVALID_HANDLE_VALUE,
                    attributes.map(|a| a as *const _),
                    PAGE_READWRITE,
                    0,
                    SIZE as u32,
                    &HSTRING::from(name),
                )
                .map_err(|e| format!("cannot create the virtual camera's memory: {e}"))?;
                let view = MapViewOfFile(mapping, FILE_MAP_ALL_ACCESS, 0, 0, SIZE);
                if view.Value.is_null() {
                    let _ = CloseHandle(mapping);
                    return Err("cannot map the virtual camera's memory".into());
                }
                let frames = Frames::new(view.Value as *mut u8);
                if !frames.valid() {
                    frames.init();
                }
                Ok(Self {
                    mapping,
                    view,
                    frames,
                })
            }
        }

        /// The DirectShow camera (in an app): opens the block if Oculibre runs. `None`: it does
        /// not.
        pub fn open() -> Option<Self> {
            Self::open_named(NAME, FILE_MAP_READ)
        }

        /// Oculibre: opens the global block to write, if the Media Foundation camera is in use
        /// (its source made it). `None`: no app uses that camera now.
        pub fn open_global() -> Option<Self> {
            Self::open_named(GLOBAL_NAME, FILE_MAP_READ | FILE_MAP_WRITE)
        }

        fn open_named(name: &str, access: FILE_MAP) -> Option<Self> {
            // SAFETY: as above.
            unsafe {
                let mapping = OpenFileMappingW(access.0, false, &HSTRING::from(name)).ok()?;
                let view = MapViewOfFile(mapping, access, 0, 0, SIZE);
                if view.Value.is_null() {
                    let _ = CloseHandle(mapping);
                    return None;
                }
                let frames = Frames::new(view.Value as *mut u8);
                if !frames.valid() {
                    let _ = UnmapViewOfFile(view);
                    let _ = CloseHandle(mapping);
                    return None;
                }
                Some(Self {
                    mapping,
                    view,
                    frames,
                })
            }
        }

        pub fn frames(&self) -> &Frames {
            &self.frames
        }
    }

    impl Drop for Shared {
        fn drop(&mut self) {
            // SAFETY: our own view and handle, released once.
            unsafe {
                let _ = UnmapViewOfFile(self.view);
                let _ = CloseHandle(self.mapping);
            }
        }
    }
}

#[cfg(windows)]
pub use windows_shm::Shared;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A block in plain memory, aligned to 8.
    fn block() -> (Vec<u64>, Frames) {
        let mut memory = vec![0u64; SIZE / 8];
        // SAFETY: SIZE bytes, aligned, kept alive by the caller with the Frames.
        let frames = unsafe { Frames::new(memory.as_mut_ptr().cast()) };
        (memory, frames)
    }

    #[test]
    fn a_picture_goes_through() {
        let (_memory, frames) = block();
        assert!(!frames.valid());
        frames.init();
        assert!(frames.valid());
        let mut out = Vec::new();
        assert_eq!(frames.read(&mut out), None); // nothing yet
        let picture: Vec<u8> = (0..64 * 32 * 2).map(|i| i as u8).collect();
        frames.write(64, 32, &picture, 1234);
        assert_eq!(frames.read(&mut out), Some((64, 32)));
        assert_eq!(out, picture);
        assert_eq!((frames.frames(), frames.alive()), (1, 1234));
    }

    /// A read that fails (writer in the middle of a picture) keeps the last whole picture.
    #[test]
    fn last_picture_survives_a_failed_read() {
        let (_memory, frames) = block();
        frames.init();
        let mut last = LastPicture::default();
        assert!(last.update(&frames).is_none());
        frames.write(4, 2, &[7; 16], 1);
        assert_eq!(
            last.update(&frames).map(|(p, w, h)| (p.to_vec(), w, h)),
            Some((vec![7; 16], 4, 2))
        );
        // The writer is halfway through the next picture (odd sequence) for longer than the read
        // waits: the previous picture stays.
        frames.header().sequence.fetch_add(1, Ordering::Release);
        assert_eq!(last.update(&frames).map(|(p, ..)| p[0]), Some(7));
        last.clear();
        assert!(last.update(&frames).is_none());
    }

    /// A reader never sees half of one picture and half of the next.
    #[test]
    fn readers_never_see_a_torn_picture() {
        let (memory, frames) = block();
        frames.init();
        let frames = Arc::new(frames);
        let writer = {
            let frames = frames.clone();
            std::thread::spawn(move || {
                for i in 0..2000u32 {
                    let value = (i % 250) as u8;
                    frames.write(320, 240, &vec![value; 320 * 240 * 2], u64::from(i));
                }
            })
        };
        let mut out = Vec::new();
        let mut seen = 0;
        while !writer.is_finished() {
            if frames.read(&mut out).is_some() {
                assert!(out.iter().all(|&b| b == out[0]), "torn picture");
                seen += 1;
            }
        }
        writer.join().unwrap();
        assert!(seen > 0);
        drop(memory);
    }
}
