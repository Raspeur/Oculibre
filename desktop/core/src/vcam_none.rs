//! No virtual camera on this system yet (Windows: #17, macOS: #19): pictures only go to the
//! preview, so the engine and the desktop app run end to end meanwhile. Same API as the Linux
//! `vcam` module, so `host` does not change.

use std::error::Error;
use std::path::PathBuf;

use crate::events::Events;
use crate::preview::Previewer;
use crate::session::Sink;
use crate::yuv;

pub struct VirtualCamera {
    preview: Option<Previewer>,
}

impl VirtualCamera {
    pub fn new(
        _path: PathBuf,
        _portrait: bool,
        _events: Events,
        preview: Option<Previewer>,
    ) -> Self {
        Self { preview }
    }
}

impl Sink for VirtualCamera {
    fn show(
        &mut self,
        picture: &yuv::Picture,
        transform: yuv::Transform,
    ) -> Result<(), Box<dyn Error>> {
        if let Some(preview) = &mut self.preview {
            preview.offer(picture, transform);
        }
        Ok(())
    }
}
