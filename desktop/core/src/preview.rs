//! Small JPEG pictures of what the virtual camera shows, for a window to display (the desktop
//! app). The virtual camera itself is never opened for that: an app holding it would lock its
//! size for everyone else.
//!
//! The output thread only copies the picture as it is (a few hundred microseconds), at most every
//! 100 ms with [`Rate::Light`], 30 times a second with [`Rate::Full`], and only when the preview
//! thread is free: that thread makes the thumbnail and the JPEG. The preview may skip pictures, the
//! virtual camera never waits for it (the thumbnail used to be made on the output thread, and a
//! laptop then fell seconds behind with the full preview).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::yuv::{self, Picture, Transform};

/// How often and how big: the user's choice in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Rate {
    /// About 10 pictures a second, 640 pixels: smooth enough to frame yourself, nearly free.
    Light,
    /// Up to 30 pictures a second, 960 pixels: for when nothing else needs the computer.
    Full,
}

impl Rate {
    /// Shortest time between two preview pictures.
    pub fn interval(self) -> Duration {
        match self {
            Rate::Light => Duration::from_millis(100),
            // 30 a second: smooth to the eye, half the work of a 60 fps stream.
            Rate::Full => Duration::from_millis(33),
        }
    }

    /// Largest side of a preview picture, in pixels (the window shows it smaller).
    pub fn max_size(self) -> usize {
        match self {
            Rate::Light => 640,
            Rate::Full => 960,
        }
    }
}

/// JPEG quality (0–100).
const QUALITY: u8 = 75;

/// One preview picture.
#[derive(Debug, Clone)]
pub struct Frame {
    pub width: usize,
    pub height: usize,
    /// A complete JPEG file.
    pub jpeg: Vec<u8>,
}

/// Where preview pictures go.
pub type Viewer = Arc<dyn Fn(Frame) + Send + Sync>;

/// Someone watching, and how much.
#[derive(Clone)]
pub struct Watch {
    pub viewer: Viewer,
    pub rate: Rate,
}

/// `None` in the slot: nobody watches, nothing is made.
pub type ViewerSlot = Arc<Mutex<Option<Watch>>>;

/// A copy of a picture for the preview thread, and how to turn and size it.
struct Job {
    picture: Picture,
    transform: Transform,
    size: usize,
}

/// Makes preview pictures for one session (see the module documentation).
pub struct Previewer {
    viewers: ViewerSlot,
    last: Option<Instant>,
    /// A picture the preview thread gave back, copied into next time.
    spare: Receiver<Picture>,
    /// Set by `offer` when it sends a job, cleared by the preview thread once done.
    busy: Arc<AtomicBool>,
    jobs: SyncSender<Job>,
}

impl Previewer {
    pub fn new(viewers: ViewerSlot) -> Self {
        let (jobs, rx) = mpsc::sync_channel::<Job>(1);
        let (back, spare) = mpsc::channel::<Picture>();
        let busy = Arc::new(AtomicBool::new(false));
        let slot = viewers.clone();
        let done = busy.clone();
        let _ = thread::Builder::new()
            .name("oculibre-preview".into())
            .spawn(move || {
                let mut rgb = Vec::new();
                // Ends when the Previewer (its sender) is dropped, with the session.
                for job in rx {
                    let (w, h) =
                        yuv::thumbnail(&job.picture, job.transform, job.size, job.size, &mut rgb);
                    let _ = back.send(job.picture);
                    let jpeg = (w > 0).then(|| encode(w, h, &rgb)).flatten();
                    if let Some(jpeg) = jpeg {
                        let watch = slot.lock().unwrap().clone();
                        if let Some(Watch { viewer, .. }) = watch {
                            viewer(Frame {
                                width: w,
                                height: h,
                                jpeg,
                            });
                        }
                    }
                    done.store(false, Ordering::Release);
                }
            });
        Self {
            viewers,
            last: None,
            spare,
            busy,
            jobs,
        }
    }

    /// Called by the virtual camera with every picture it writes: cheap, never waits.
    pub fn offer(&mut self, picture: &Picture, transform: Transform) {
        let Some(rate) = self.viewers.lock().unwrap().as_ref().map(|w| w.rate) else {
            return; // nobody watches
        };
        if self.last.is_some_and(|t| t.elapsed() < rate.interval()) {
            return;
        }
        if self.busy.swap(true, Ordering::Acquire) {
            return; // the previous preview picture is not done yet: skip this one
        }
        self.last = Some(Instant::now());
        let mut copy = self.spare.try_recv().unwrap_or_default();
        copy.copy_from(picture);
        let job = Job {
            picture: copy,
            transform,
            size: rate.max_size(),
        };
        if let Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) = self.jobs.try_send(job)
        {
            self.busy.store(false, Ordering::Release);
        }
    }
}

fn encode(width: usize, height: usize, rgb: &[u8]) -> Option<Vec<u8>> {
    let mut jpeg = Vec::with_capacity(32 * 1024);
    let encoder = jpeg_encoder::Encoder::new(&mut jpeg, QUALITY);
    encoder
        .encode(
            rgb,
            u16::try_from(width).ok()?,
            u16::try_from(height).ok()?,
            jpeg_encoder::ColorType::Rgb,
        )
        .ok()?;
    Some(jpeg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_are_jpegs_at_most_every_interval() {
        let frames = Arc::new(Mutex::new(Vec::<Frame>::new()));
        let viewer: Viewer = {
            let frames = frames.clone();
            Arc::new(move |f| frames.lock().unwrap().push(f))
        };
        let slot: ViewerSlot = Arc::new(Mutex::new(Some(Watch {
            viewer: viewer.clone(),
            rate: Rate::Light,
        })));
        let (w, h) = (1280usize, 720usize);
        let mut picture = Picture::default();
        let luma = vec![120u8; w * h];
        let chroma = vec![128u8; (w / 2) * (h / 2)];
        picture.copy_i420(w, h, (&luma, w), (&chroma, w / 2), (&chroma, w / 2));

        let mut previewer = Previewer::new(slot.clone());
        let turned = Transform {
            rotation: 90,
            mirror: false,
        };
        for _ in 0..5 {
            previewer.offer(&picture, turned); // only the first one is taken
        }
        drop(previewer); // ends the encoder thread once it is done
        let deadline = Instant::now() + Duration::from_secs(5);
        while frames.lock().unwrap().is_empty() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        {
            let frames = frames.lock().unwrap();
            assert_eq!(frames.len(), 1);
            // Turned to portrait, fitted into 640 × 640.
            assert_eq!((frames[0].width, frames[0].height), (360, 640));
            assert_eq!(&frames[0].jpeg[..2], &[0xFF, 0xD8]); // JPEG start marker
        }

        // Full rate: up to 30 a second, never while the previous picture is being made.
        *slot.lock().unwrap() = Some(Watch {
            viewer,
            rate: Rate::Full,
        });
        let mut previewer = Previewer::new(slot.clone());
        previewer.offer(&picture, turned);
        let first = previewer.last;
        previewer.offer(&picture, turned);
        assert_eq!(previewer.last, first, "too soon: skipped");
        let deadline = Instant::now() + Duration::from_secs(5);
        while previewer.busy.load(Ordering::Acquire) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        thread::sleep(Rate::Full.interval());
        previewer.offer(&picture, turned);
        assert!(previewer.last > first, "the next picture was taken");

        // Nobody watching: nothing is made.
        *slot.lock().unwrap() = None;
        let mut previewer = Previewer::new(slot);
        previewer.offer(&picture, turned);
        assert!(previewer.last.is_none());
    }
}
