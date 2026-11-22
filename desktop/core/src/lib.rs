//! Oculibre desktop engine: everything the receiver does, without a user interface.
//!
//! The command line (`desktop/cli`) and the desktop app (#15) are two front-ends over this crate.
//!
//! - [`host`]: the receiver as a whole: waits for phones, runs sessions, pairing offers, stop
//! - [`events`]: what the engine reports to its front-end (it never prints)
//! - [`session`]: one connection with a phone (protocol, decoding, latency, commands)
//! - [`decoder`]: H.264 decoding (FFmpeg with VA-API / NVDEC, or OpenH264)
//! - [`yuv`]: rotation, mirror, fit and pixel conversion; [`vcam`] over [`v4l2`]: the Linux
//!   virtual camera; [`adb`]: USB phones
//! - [`tls`], [`pairing`], [`discovery`]: encrypted Wi-Fi, paired phones, mDNS announcement
//! - [`preview`]: small JPEG pictures of the output, for a window
//! - [`profiles`]: each phone's settings, remembered
//! - [`bench`]: measures every camera × mode of a phone (`oculibre bench`)

pub mod adb;
pub mod bench;
pub mod decoder;
pub mod discovery;
pub mod events;
pub mod host;
pub mod pairing;
pub mod power;
pub mod preview;
pub mod profiles;
pub mod session;
pub mod store;
pub mod tls;
#[cfg(target_os = "linux")]
pub mod v4l2;
#[cfg(target_os = "linux")]
pub mod vcam;
#[cfg(windows)]
#[path = "vcam_windows.rs"]
pub mod vcam;
#[cfg(not(any(target_os = "linux", windows)))]
#[path = "vcam_none.rs"]
pub mod vcam;
#[cfg(target_os = "linux")]
pub mod vcam_setup;
#[cfg(windows)]
#[path = "vcam_setup_windows.rs"]
pub mod vcam_setup;
pub mod yuv;

pub use events::{ignore_events, Event, Events};
/// The wire protocol, for front-ends that need its types (capabilities, `SetMode`…).
pub use oculibre_protocol as protocol;

#[cfg(test)]
mod tests;
