use std::fmt;
use std::io;

/// Everything that can go wrong while reading the protocol.
#[derive(Debug)]
pub enum Error {
    /// Transport error, including an unexpected end of stream in the middle of a message.
    Io(io::Error),
    /// The peer closed the connection cleanly, between two messages.
    ConnectionClosed,
    /// The preamble does not start with `OCUL`: not an Oculibre peer.
    BadMagic([u8; 4]),
    /// The preamble role is neither camera (1) nor host (2).
    BadRole(u8),
    /// Both sides announced the same role.
    SameRole,
    /// Different major versions: the user must update one side.
    IncompatibleMajor { local: u8, peer: u8, peer_minor: u8 },
    /// `length` above the limit of §14.
    TooLarge { length: u32, max: u32 },
    /// A JSON payload above the limit of §14.
    JsonTooLarge { msg_type: u8, length: u32 },
    /// A binary payload shorter than its fixed part.
    Truncated {
        msg_type: u8,
        need: usize,
        got: usize,
    },
    /// A JSON payload that is not valid UTF-8 JSON or misses a required field.
    Json { msg_type: u8, error: String },
}

impl Error {
    /// Stable identifier, used by the shared test vectors (`expect_error`).
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Io(_) => "io",
            Error::ConnectionClosed => "connection_closed",
            Error::BadMagic(_) => "bad_magic",
            Error::BadRole(_) => "bad_role",
            Error::SameRole => "same_role",
            Error::IncompatibleMajor { .. } => "incompatible_major",
            Error::TooLarge { .. } => "too_large",
            Error::JsonTooLarge { .. } => "json_too_large",
            Error::Truncated { .. } => "truncated",
            Error::Json { .. } => "json",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "I/O error: {e}"),
            Error::ConnectionClosed => write!(f, "connection closed by the peer"),
            Error::BadMagic(m) => write!(f, "not an Oculibre peer (preamble starts with {m:02x?})"),
            Error::BadRole(r) => write!(f, "invalid role {r} in preamble"),
            Error::SameRole => write!(f, "both sides announced the same role"),
            Error::IncompatibleMajor {
                local,
                peer,
                peer_minor,
            } => {
                let update = if peer > local {
                    "this side"
                } else {
                    "the other side"
                };
                write!(
                    f,
                    "incompatible protocol versions: peer speaks {peer}.{peer_minor}, we speak {local}.x; update {update}"
                )
            }
            Error::TooLarge { length, max } => write!(f, "message of {length} bytes (max {max})"),
            Error::JsonTooLarge { msg_type, length } => write!(
                f,
                "{} JSON payload of {length} bytes (max {})",
                crate::types::name(*msg_type),
                crate::MAX_JSON_LEN
            ),
            Error::Truncated {
                msg_type,
                need,
                got,
            } => write!(
                f,
                "{} payload too short: {got} bytes, need {need}",
                crate::types::name(*msg_type)
            ),
            Error::Json { msg_type, error } => {
                write!(f, "invalid {} JSON: {error}", crate::types::name(*msg_type))
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}
