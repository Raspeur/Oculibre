//! What the engine tells its front-end (the console of `oculibre`, the desktop app): one [`Event`]
//! per thing that happens, sent to an [`Events`] callback. The engine itself never prints.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use oculibre_protocol::{Hello, StreamConfig};

use crate::session::{End, Status, Summary};
use crate::yuv::Transform;

/// How a phone reached this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Via {
    /// USB cable, through `adb reverse` (plaintext on loopback).
    Usb,
    /// The network, encrypted; `tls`: the TLS version ("TLSv1.3").
    Wifi { tls: String },
}

/// Why a connection was refused before any session.
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[derive(Debug, Clone)]
pub enum Event {
    // --- the computer (`Host`)
    /// The virtual camera that will receive the pictures.
    VirtualCamera { path: PathBuf },
    /// `adb reverse` for USB phones: done, or why not (USB then won't work, Wi-Fi still will).
    Usb(Result<(), String>),
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
