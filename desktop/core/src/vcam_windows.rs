//! The virtual camera on Windows (#17): Oculibre writes each picture into shared memory
//! (`oculibre-vcam-shm`), where Oculibre's camera DLLs read it: the DirectShow filter inside the
//! apps (`vcam-dshow`, Windows 10 and 11), and on Windows 11 the Media Foundation source inside
//! the Frame Server service (`vcam-mf`; the installer registers that camera once, for every user,
//! with `oculibre-camera register`). That one has its own, global block (the service cannot see
//! the user's), which its source creates: Oculibre writes there too once it exists.
//!
//! Same rules as on Linux: the camera keeps one shape (landscape 1920×1080, or portrait with
//! `portrait`), a picture of the other shape gets black bars; rotation and mirror are applied
//! here. The memory lives as long as Oculibre: between two phones the apps keep the camera and
//! show "waiting" instead of losing it.

use std::error::Error;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use oculibre_vcam_shm::Shared;

use crate::events::{Event, Events};
use crate::preview::Previewer;
use crate::session::Sink;
use crate::yuv;

/// The camera's size: what apps are offered (vcam-dshow advertises the same).
const LANDSCAPE: (usize, usize) = (1920, 1080);

fn shared() -> Result<&'static (Shared, Instant), String> {
    static SHARED: OnceLock<Result<(Shared, Instant), String>> = OnceLock::new();
    SHARED
        .get_or_init(|| Shared::create().map(|s| (s, Instant::now())))
        .as_ref()
        .map_err(Clone::clone)
}

pub struct VirtualCamera {
    portrait: bool,
    events: Events,
    /// The size last announced (the panel and Settings show the "format sent").
    announced: Option<(usize, usize)>,
    preview: Option<Previewer>,
    frame: Vec<u8>,
}

impl VirtualCamera {
    pub fn new(_path: PathBuf, portrait: bool, events: Events, preview: Option<Previewer>) -> Self {
        Self {
            portrait,
            events,
            announced: None,
            preview,
            frame: Vec::new(),
        }
    }
}

impl Sink for VirtualCamera {
    fn show(
        &mut self,
        picture: &yuv::Picture,
        transform: yuv::Transform,
    ) -> Result<(), Box<dyn Error>> {
        let (shared, started) = shared()?;
        let (w, h) = if self.portrait {
            (LANDSCAPE.1, LANDSCAPE.0)
        } else {
            LANDSCAPE
        };
        if self.announced != Some((w, h)) {
            self.announced = Some((w, h));
            (self.events)(Event::VirtualCameraSize {
                wanted: (w, h),
                size: (w, h),
            });
        }
        yuv::convert(picture, transform, w, h, &mut self.frame);
        let now = started.elapsed().as_millis() as u64;
        shared.frames().write(w, h, &self.frame, now);
        if let Some(global) = global().lock().unwrap().get() {
            global.frames().write(w, h, &self.frame, now);
        }
        if let Some(preview) = &mut self.preview {
            preview.offer(picture, transform);
        }
        Ok(())
    }
}

/// The Media Foundation camera's block, opened once its source made it (an app opened that
/// camera): looked for at most once a second.
struct Global {
    shared: Option<Shared>,
    tried: Option<Instant>,
}

impl Global {
    fn get(&mut self) -> Option<&Shared> {
        if self.shared.is_none()
            && self
                .tried
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(1))
        {
            self.tried = Some(Instant::now());
            self.shared = Shared::open_global();
        }
        self.shared.as_ref()
    }
}

fn global() -> &'static Mutex<Global> {
    static GLOBAL: Mutex<Global> = Mutex::new(Global {
        shared: None,
        tried: None,
    });
    &GLOBAL
}

/// Creates the shared memory now, so that apps find the camera before the first phone.
pub fn prepare() -> Result<(), String> {
    shared().map(drop)
}
