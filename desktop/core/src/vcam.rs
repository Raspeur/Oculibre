//! The virtual camera on Linux: a v4l2loopback device, seen by every app as a webcam.
//!
//! Other systems (#17, #19) get their own [`Sink`] implementation; the session only knows the
//! trait.

use std::error::Error;
use std::path::PathBuf;

use openh264::formats::YUVSource;

use crate::events::{Event, Events};
use crate::preview::Previewer;
use crate::session::Sink;
use crate::{v4l2, yuv};

/// The v4l2loopback device, opened at the first picture and closed with the session.
///
/// Its shape never follows the phone: landscape by default, like every webcam (apps expect it,
/// and v4l2loopback cannot change the size while an app has the camera open anyway). A portrait
/// picture gets black bars on the sides; `portrait` gives a portrait virtual camera instead.
pub struct VirtualCamera {
    path: PathBuf,
    portrait: bool,
    events: Events,
    /// Small copies for a window (the desktop app), if any.
    preview: Option<Previewer>,
    device: Option<v4l2::OutputDevice>,
    /// Size asked for when the device was opened.
    requested: (usize, usize),
    frame: Vec<u8>,
}

impl VirtualCamera {
    pub fn new(path: PathBuf, portrait: bool, events: Events, preview: Option<Previewer>) -> Self {
        Self {
            path,
            portrait,
            events,
            preview,
            device: None,
            requested: (0, 0),
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
        let (pw, ph) = picture.dimensions();
        let (long, short) = (pw.max(ph), pw.min(ph));
        let (w, h) = if self.portrait {
            (short & !1, long)
        } else {
            (long & !1, short)
        };
        if (w, h) != self.requested {
            self.device = None; // close before reopening at the new size
        }
        let device = match self.device.as_mut() {
            Some(d) => d,
            None => {
                let d = v4l2::OutputDevice::open(&self.path, w, h)?;
                self.requested = (w, h);
                (self.events)(Event::VirtualCameraSize {
                    wanted: (w, h),
                    size: d.size(),
                });
                self.device.insert(d)
            }
        };
        let (cw, ch) = device.size();
        yuv::convert(picture, transform, cw, ch, &mut self.frame);
        device.write_frame(&self.frame)?;
        if let Some(preview) = &mut self.preview {
            preview.offer(picture, transform);
        }
        Ok(())
    }
}
