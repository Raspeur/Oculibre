//! Oculibre wire protocol, version 0.3.
//!
//! The specification is `docs/protocol.typ`; this crate is its reference implementation on the
//! host side. Section numbers (§) below refer to that document.
//!
//! ```text
//! connection = preamble (8 bytes, each side) + messages
//! message    = header (16 bytes) + payload (`header.length` bytes)
//! ```
//!
//! - [`Preamble`]: magic, version and role, exchanged once (§4).
//! - [`Header`]: fixed 16-byte message header (§5).
//! - [`Message`]: every message type of 0.3 (§7–§9), decoded with [`Message::decode`] and
//!   encoded with [`Message::encode`].
//! - [`MessageReader`]: reads a preamble and messages from any [`std::io::Read`], enforcing the
//!   limits of §14 before allocating anything.

#![forbid(unsafe_code)]

pub mod clock;
mod error;
mod header;
mod message;
pub mod orientation;
mod preamble;
mod reader;
#[doc(hidden)]
pub mod vectors;

pub use error::Error;
pub use header::{Header, HEADER_LEN};
pub use message::{
    Bye, CameraCapabilities, CameraInfo, Capabilities, Color, Device, ErrorBody, Event, Hello,
    Message, ModeCapabilities, Ping, Pong, Request, Response, SetMode, StreamConfig, StreamEnd,
    VideoFrame, VIDEO_SUBHEADER_LEN,
};
pub use preamble::{Preamble, Role, MAGIC, PREAMBLE_LEN};
pub use reader::{write_message, MessageReader};

/// Protocol major version (§15).
pub const MAJOR: u8 = 0;
/// Protocol minor version (§15).
pub const MINOR: u8 = 3;

/// Port a host listens on (§2).
pub const HOST_PORT: u16 = 47100;
/// Port a camera listens on in station mode (§2, reserved).
pub const CAMERA_PORT: u16 = 47101;

/// Maximum `length` of any message (§14).
pub const MAX_MESSAGE_LEN: u32 = 16 * 1024 * 1024;
/// Maximum size of a JSON payload (§14).
pub const MAX_JSON_LEN: usize = 64 * 1024;

/// Message types (§6).
pub mod types {
    pub const HELLO: u8 = 0x01;
    pub const BYE: u8 = 0x02;
    pub const PING: u8 = 0x03;
    pub const PONG: u8 = 0x04;
    pub const STREAM_CONFIG: u8 = 0x20;
    pub const VIDEO_FRAME: u8 = 0x21;
    /// Reserved: decoded as [`crate::Message::Unknown`] in 0.1.
    pub const AUDIO_FRAME: u8 = 0x22;
    pub const STREAM_END: u8 = 0x23;
    pub const REQUEST: u8 = 0x40;
    pub const RESPONSE: u8 = 0x41;
    pub const EVENT: u8 = 0x42;

    /// Human-readable name, for logs and test vectors.
    pub fn name(t: u8) -> &'static str {
        match t {
            HELLO => "HELLO",
            BYE => "BYE",
            PING => "PING",
            PONG => "PONG",
            STREAM_CONFIG => "STREAM_CONFIG",
            VIDEO_FRAME => "VIDEO_FRAME",
            AUDIO_FRAME => "AUDIO_FRAME",
            STREAM_END => "STREAM_END",
            REQUEST => "REQUEST",
            RESPONSE => "RESPONSE",
            EVENT => "EVENT",
            _ => "UNKNOWN",
        }
    }

    /// Types decoded by this version (everything else, including the reserved `AUDIO_FRAME`, is
    /// skipped without being buffered).
    pub fn is_decoded(t: u8) -> bool {
        matches!(
            t,
            HELLO
                | BYE
                | PING
                | PONG
                | STREAM_CONFIG
                | VIDEO_FRAME
                | STREAM_END
                | REQUEST
                | RESPONSE
                | EVENT
        )
    }

    /// Types whose payload is a JSON object.
    pub fn is_json(t: u8) -> bool {
        matches!(
            t,
            HELLO | BYE | STREAM_CONFIG | STREAM_END | REQUEST | RESPONSE | EVENT
        )
    }
}

/// `VIDEO_FRAME` flags (§8.2).
pub mod flags {
    pub const KEY: u8 = 0x01;
    pub const DISCONTINUITY: u8 = 0x02;
}

/// Stream numbers (§5).
pub mod streams {
    pub const SESSION: u8 = 0;
    pub const VIDEO: u8 = 1;
    pub const AUDIO: u8 = 2;
}

/// Feature names (§15).
pub mod features {
    pub const VIDEO_H264: &str = "video.h264";
    pub const CLOCK: &str = "clock";
    pub const ORIENTATION: &str = "orientation";
    pub const STATS: &str = "stats";
    pub const CONTROL_KEYFRAME: &str = "control.keyframe";
    pub const CONTROL_BITRATE: &str = "control.bitrate";
    /// `set_mode` (since 0.2).
    pub const CONTROL_MODE: &str = "control.mode";
}

/// Commands of `REQUEST` (§9.1).
pub mod commands {
    pub const REQUEST_KEYFRAME: &str = "request_keyframe";
    pub const SET_BITRATE: &str = "set_bitrate";
    pub const GET_CAPABILITIES: &str = "get_capabilities";
    /// Since 0.2, feature `control.mode`.
    pub const SET_MODE: &str = "set_mode";
}

/// `STREAM_END` reasons (§8.3).
pub mod stream_end_reasons {
    pub const CAMERA_SWITCH: &str = "camera_switch";
    /// The camera applies a `set_mode` command (since 0.2).
    pub const MODE_CHANGE: &str = "mode_change";
}

/// `BYE` reasons (§7.2).
pub mod bye_reasons {
    pub const USER_STOP: &str = "user_stop";
    pub const BUSY: &str = "busy";
    pub const CAMERA_LOST: &str = "camera_lost";
    pub const THERMAL: &str = "thermal";
    pub const BATTERY: &str = "battery";
    pub const VERSION: &str = "version";
    pub const UNPAIRED: &str = "unpaired";
    pub const PROTOCOL_ERROR: &str = "protocol_error";
    pub const SHUTDOWN: &str = "shutdown";
    pub const OTHER: &str = "other";
}

/// Error codes of `RESPONSE` (§9.1).
pub mod error_codes {
    pub const UNKNOWN_COMMAND: &str = "unknown_command";
    pub const UNSUPPORTED: &str = "unsupported";
    pub const INVALID_ARGS: &str = "invalid_args";
    pub const BUSY: &str = "busy";
    pub const FAILED: &str = "failed";
}
