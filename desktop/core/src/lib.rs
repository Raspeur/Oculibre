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
//! - [`bench`]: measures every camera × mode of a phone (`oculibre bench`)

pub mod adb;
pub mod bench;
pub mod decoder;
pub mod discovery;
pub mod events;
pub mod host;
pub mod pairing;
pub mod session;
pub mod tls;
pub mod v4l2;
pub mod vcam;
pub mod yuv;

pub use events::{ignore_events, Event, Events};

#[cfg(test)]
mod tests;
