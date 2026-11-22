//! What the engine tells its front-end (the console of `oculibre`, the desktop app): one [`Event`]
//! per thing that happens, sent to an [`Events`] callback. The engine itself never prints.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use oculibre_protocol::{Hello, StreamConfig};
use serde::Serialize;

use crate::session::{End, Status, Summary};
use crate::yuv::Transform;

/// How a phone reached this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind")]
pub enum Via {
    /// USB cable, through `adb reverse` (plaintext on loopback).
    Usb,
    /// The network, encrypted; `tls`: the TLS version ("TLSv1.3").
    Wifi { tls: String },
}

/// What happens with the phones plugged in, in accessory mode ([`Event::Accessory`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum Accessory {
    /// A phone was asked to switch to accessory mode: Android offers to open Oculibre.
    Switched { phone: String },
    /// A phone is in accessory mode: waiting for the app to start the stream.
    Waiting,
    /// The phone in accessory mode cannot be used (no access to its USB device…).
    Failed(String),
}

/// Why a connection was refused before any session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum Refusal {
    /// It came from the network while Wi-Fi is off (§13: never plaintext on a network).
    WifiOff,
    /// The TLS handshake failed (not a paired-looking phone, old TLS…).
    Tls(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::WifiOff => write!(f, "Wi-Fi is off"),
            Refusal::Tls(e) => write!(f, "{e}"),
        }
    }
}

/// Wi-Fi is on: what the computer shows to phones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WifiInfo {
    /// This computer's network addresses.
    pub addresses: Vec<IpAddr>,
    /// Phones paired so far.
    pub paired: usize,
    /// Start of this computer's key fingerprint ("46B5 EE53"), the code shown when pairing.
    pub key: String,
    /// The computer is announced on the network (mDNS): phones find it without an address.
    pub announced: bool,
}

/// Everything the engine reports. Sent from several threads: receivers must be quick and must not
/// block.
///
/// As JSON (the desktop app): `{"kind": "Status", "data": {…}}`; `Result`s become
/// `{"Ok": …}` / `{"Err": "…"}`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum Event {
    // --- the computer (`Host`)
    /// The virtual camera that will receive the pictures.
    VirtualCamera { path: PathBuf },
    /// `adb reverse` for USB phones: done, or why not (USB then won't work, Wi-Fi still will).
    Usb(Result<(), String>),
    /// USB without developer mode (Android Open Accessory, #21, Linux).
    Accessory(Accessory),
    /// Wi-Fi is on (`--wifi`).
    Wifi(WifiInfo),
    /// The mDNS announcement failed: phones will use the address they remember.
    AnnounceFailed(String),
    /// A phone connected (before its `HELLO`).
    PhoneConnected { peer: SocketAddr, via: Via },
    /// A connection was refused before any session (not on loopback without Wi-Fi, TLS failed…).
    PhoneRefused { peer: SocketAddr, reason: Refusal },
    /// Waiting for connections failed once (the host keeps waiting).
    AcceptFailed(String),
    /// A new phone was paired: it can now connect over Wi-Fi.
    Paired { name: String },
    /// The phone's session is over; the host waits for the next connection.
    SessionEnded(Result<End, String>),

    // --- one phone's session
    /// The phone's `HELLO`, once it is accepted (paired, H.264 offered).
    Phone(Hello),
    /// A new video stream: after Start, or after a mode change (`set_mode`).
    Stream(StreamConfig),
    /// The phone stopped the stream for a while (e.g. `mode_change`); a [`Event::Stream`] follows.
    StreamPaused { reason: String },
    /// Rotation and mirror applied from now on (sent when they change, with a picture).
    /// `auto`: the rotation follows the way the phone is held (`--rotate auto`).
    Transform { transform: Transform, auto: bool },
    /// The video decoder in use, known for sure once a picture came out ("VA-API", "OpenH264"…).
    Decoder { name: &'static str },
    /// The graphics card decoder `from` took `decode_ms` per picture on average, more than the
    /// `frame_ms` the phone leaves between two: decoding moves to the processor for the rest of
    /// the session (problem 505). A [`Event::Decoder`] with the new name follows.
    DecoderTooSlow {
        from: &'static str,
        decode_ms: f64,
        frame_ms: f64,
    },
    /// The virtual camera was (re)opened for pictures of `wanted` size; `size` is what it really
    /// has: smaller or of another shape when an app holds it open (the picture is then fitted).
    VirtualCameraSize {
        wanted: (usize, usize),
        size: (usize, usize),
    },
    /// The phone refused one of our requests that nobody waited for (e.g. `request_keyframe`).
    RequestRefused {
        id: u32,
        code: String,
        message: Option<String>,
    },
    /// Every second while the phone is connected.
    Status(Status),
    /// When the session ends, if it lasted at least one second.
    Summary(Summary),
}

/// Where the engine sends its [`Event`]s. Shared with its threads, hence `Arc` and `Send + Sync`.
pub type Events = Arc<dyn Fn(Event) + Send + Sync>;

/// For a session nobody watches (tests, the bench).
pub fn ignore_events() -> Events {
    Arc::new(|_| {})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Rotate, View};
    use serde_json::json;

    /// The JSON the desktop app receives: its TypeScript types (app/src/engine.ts) follow it.
    #[test]
    fn events_as_json() {
        let json = |e: Event| serde_json::to_value(e).unwrap();
        assert_eq!(
            json(Event::Decoder { name: "VA-API" }),
            json!({"kind": "Decoder", "data": {"name": "VA-API"}})
        );
        assert_eq!(
            json(Event::Usb(Err("no adb".into()))),
            json!({"kind": "Usb", "data": {"Err": "no adb"}})
        );
        assert_eq!(
            json(Event::SessionEnded(Ok(End::Closed))),
            json!({"kind": "SessionEnded", "data": {"Ok": {"kind": "Closed"}}})
        );
        assert_eq!(
            json(Event::PhoneConnected {
                peer: "127.0.0.1:50000".parse().unwrap(),
                via: Via::Wifi {
                    tls: "TLSv1.3".into()
                },
            }),
            json!({"kind": "PhoneConnected", "data": {
                "peer": "127.0.0.1:50000", "via": {"kind": "Wifi", "tls": "TLSv1.3"}}})
        );
        let status = serde_json::to_value(Event::Status(Status::default())).unwrap();
        assert_eq!(status["data"]["shown_fps"], json!(0.0));
        assert_eq!(status["data"]["latency"], json!(null));
        // What the app sends back to change the rotation.
        let view: View = serde_json::from_value(
            json!({"rotate": {"kind": "Fixed", "degrees": 90}, "mirror": true}),
        )
        .unwrap();
        assert_eq!(view.rotate, Rotate::Fixed(90));
        assert_eq!(
            serde_json::to_value(View::default()).unwrap(),
            json!({"rotate": {"kind": "Auto"}, "mirror": false})
        );
    }
}
