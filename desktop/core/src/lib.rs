//! Oculibre desktop engine: everything the receiver does, without a user interface.
//!
//! The command line (`desktop/cli`) and the desktop app (#15) are two front-ends over this crate.
//!
//! - [`session`]: one connection with a phone (protocol, decoding, latency, commands)
//! - [`decoder`]: H.264 decoding (FFmpeg with VA-API / NVDEC, or OpenH264)
//! - [`yuv`]: rotation, mirror, fit and pixel conversion; [`v4l2`]: the Linux virtual camera
//! - [`tls`], [`pairing`], [`discovery`]: encrypted Wi-Fi, paired phones, mDNS announcement
//! - [`bench`]: measures every camera × mode of a phone (`oculibre bench`)

pub mod bench;
pub mod decoder;
pub mod discovery;
pub mod pairing;
pub mod session;
pub mod tls;
pub mod v4l2;
pub mod yuv;

#[cfg(test)]
mod tests;
