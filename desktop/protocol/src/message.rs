//! Messages of protocol 0.3 (§7–§9).
//!
//! JSON payloads use serde with `#[serde(default)]`: unknown keys are ignored and missing optional
//! keys take their documented default, as §6 requires.

use base64::Engine;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;

use crate::{flags, streams, types, Error, Header, HEADER_LEN, MAX_JSON_LEN};

/// Size of the fixed part of a `VIDEO_FRAME` payload (§8.2).
pub const VIDEO_SUBHEADER_LEN: usize = 8;
const PING_LEN: usize = 16;
const PONG_LEN: usize = 32;

// ------------------------------------------------------------------ JSON payloads

/// `HELLO` (§7.1).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Hello {
    pub app: String,
    pub app_version: String,
    /// Random UUID created once per installation, never derived from hardware identifiers.
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<Device>,
    pub features: Vec<String>,
    /// Optional capabilities catalogue (§7.4), kept as raw JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Value>,
    /// Camera only, inside the secure channel: the one-time token of the pairing QR code (§13,
    /// since 0.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pairing_token: Option<String>,
}

impl Hello {
    pub fn has_feature(&self, feature: &str) -> bool {
        self.features.iter().any(|f| f == feature)
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Device {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdk: Option<u32>,
}

/// `BYE` (§7.2). `reason` values: see [`crate::bye_reasons`]; unknown ones mean "other".
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Bye {
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Capabilities catalogue (§7.4): `HELLO.capabilities` and the result of `get_capabilities`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Capabilities {
    pub cameras: Vec<CameraCapabilities>,
    pub codecs: Vec<String>,
}

impl Capabilities {
    /// Reads the raw JSON of `HELLO.capabilities` or of a `get_capabilities` result.
    pub fn from_value(value: &Value) -> Result<Self, String> {
        serde_json::from_value(value.clone()).map_err(|e| e.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraCapabilities {
    pub id: String,
    /// `back`, `front` or `external`.
    pub facing: String,
    pub sensor_orientation: u16,
    pub modes: Vec<ModeCapabilities>,
    pub controls: Vec<String>,
}

/// One mode a camera can stream.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ModeCapabilities {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// `normal`, `unlisted` or `high_speed` (absent: `normal`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_fps: Option<u32>,
}

/// Arguments of the `set_mode` command (§9.1, since 0.2).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SetMode {
    pub camera: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl SetMode {
    pub fn new(camera: &CameraCapabilities, mode: &ModeCapabilities) -> Self {
        Self {
            camera: camera.id.clone(),
            width: mode.width,
            height: mode.height,
            fps: mode.fps,
            path: mode.path.clone(),
        }
    }

    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).expect("protocol structs always serialise")
    }
}

/// `STREAM_CONFIG` (§8.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamConfig {
    pub config_id: u16,
    pub kind: String,
    pub codec: String,
    /// Base64 of the decoder initialisation data (Annex B), see [`StreamConfig::codec_data_bytes`].
    pub codec_data: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_fps: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<u32>,
    pub color: Color,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraInfo>,
}

impl Default for StreamConfig {
    fn default() -> Self {
        Self {
            config_id: 0,
            kind: "video".into(),
            codec: "h264".into(),
            codec_data: String::new(),
            width: 0,
            height: 0,
            fps: 0,
            capture_fps: None,
            path: None,
            bitrate: None,
            color: Color::default(),
            camera: None,
        }
    }
}

impl StreamConfig {
    pub fn codec_data_bytes(&self) -> Result<Vec<u8>, Error> {
        base64::engine::general_purpose::STANDARD
            .decode(self.codec_data.trim())
            .map_err(|e| Error::Json {
                msg_type: types::STREAM_CONFIG,
                error: format!("codec_data: {e}"),
            })
    }

    pub fn set_codec_data(&mut self, bytes: &[u8]) {
        self.codec_data = base64::engine::general_purpose::STANDARD.encode(bytes);
    }
}

/// Colour description (§8.1). Defaults when absent: BT.601 / BT.709 / BT.601 / limited.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Color {
    pub primaries: String,
    pub transfer: String,
    pub matrix: String,
    pub range: String,
}

impl Default for Color {
    fn default() -> Self {
        Self {
            primaries: "bt601".into(),
            transfer: "bt709".into(),
            matrix: "bt601".into(),
            range: "limited".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraInfo {
    pub id: String,
    /// `back`, `front` or `external`.
    pub facing: String,
    /// Degrees, clockwise: 0, 90, 180 or 270.
    pub sensor_orientation: u16,
    pub mirrored: bool,
}

/// `STREAM_END` (§8.3).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamEnd {
    pub reason: String,
}

/// `REQUEST` (§9.1).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Request {
    pub id: u32,
    pub cmd: String,
    #[serde(skip_serializing_if = "Value::is_null")]
    pub args: Value,
}

/// `RESPONSE` (§9.1).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Response {
    pub id: u32,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

impl Response {
    pub fn ok(id: u32) -> Self {
        Self {
            id,
            ok: true,
            result: Some(Value::Object(Default::default())),
            error: None,
        }
    }

    pub fn error(id: u32, code: &str, message: Option<String>) -> Self {
        Self {
            id,
            ok: false,
            result: None,
            error: Some(ErrorBody {
                code: code.into(),
                message,
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ErrorBody {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// `EVENT` (§9.2).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Event {
    pub event: String,
    pub data: Value,
}

// ------------------------------------------------------------------ binary payloads

/// `PING` (§7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ping {
    pub seq: u32,
    /// Send time, sender clock, µs.
    pub t1: u64,
}

/// `PONG` (§7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pong {
    pub seq: u32,
    pub t1: u64,
    /// `PING` received, responder clock, µs.
    pub t2: u64,
    /// `PONG` sent, responder clock, µs.
    pub t3: u64,
}

/// `VIDEO_FRAME` (§8.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFrame {
    pub stream: u8,
    /// Capture time, camera clock, µs.
    pub timestamp: u64,
    pub key: bool,
    pub discontinuity: bool,
    pub frame_number: u32,
    pub config_id: u16,
    /// One complete access unit, Annex B.
    pub data: Vec<u8>,
}

// ------------------------------------------------------------------ message

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Hello(Hello),
    Bye(Bye),
    Ping(Ping),
    Pong(Pong),
    StreamConfig {
        stream: u8,
        config: StreamConfig,
    },
    VideoFrame(VideoFrame),
    StreamEnd {
        stream: u8,
        end: StreamEnd,
    },
    Request(Request),
    Response(Response),
    Event(Event),
    /// A type this version does not know: skipped, as §6 requires.
    Unknown {
        msg_type: u8,
        stream: u8,
        length: u32,
    },
}

impl Message {
    pub fn msg_type(&self) -> u8 {
        match self {
            Message::Hello(_) => types::HELLO,
            Message::Bye(_) => types::BYE,
            Message::Ping(_) => types::PING,
            Message::Pong(_) => types::PONG,
            Message::StreamConfig { .. } => types::STREAM_CONFIG,
            Message::VideoFrame(_) => types::VIDEO_FRAME,
            Message::StreamEnd { .. } => types::STREAM_END,
            Message::Request(_) => types::REQUEST,
            Message::Response(_) => types::RESPONSE,
            Message::Event(_) => types::EVENT,
            Message::Unknown { msg_type, .. } => *msg_type,
        }
    }

    /// Encodes header + payload. `Unknown` encodes as a header with an empty payload.
    pub fn encode(&self) -> Vec<u8> {
        let mut header = Header {
            msg_type: self.msg_type(),
            stream: streams::SESSION,
            ..Default::default()
        };
        let payload: Vec<u8> = match self {
            Message::Hello(m) => json(m),
            Message::Bye(m) => json(m),
            Message::Request(m) => json(m),
            Message::Response(m) => json(m),
            Message::Event(m) => json(m),
            Message::StreamConfig { stream, config } => {
                header.stream = *stream;
                json(config)
            }
            Message::StreamEnd { stream, end } => {
                header.stream = *stream;
                json(end)
            }
            Message::Ping(p) => {
                let mut b = Vec::with_capacity(PING_LEN);
                b.extend_from_slice(&p.seq.to_be_bytes());
                b.extend_from_slice(&[0; 4]);
                b.extend_from_slice(&p.t1.to_be_bytes());
                b
            }
            Message::Pong(p) => {
                let mut b = Vec::with_capacity(PONG_LEN);
                b.extend_from_slice(&p.seq.to_be_bytes());
                b.extend_from_slice(&[0; 4]);
                b.extend_from_slice(&p.t1.to_be_bytes());
                b.extend_from_slice(&p.t2.to_be_bytes());
                b.extend_from_slice(&p.t3.to_be_bytes());
                b
            }
            Message::VideoFrame(v) => {
                header.stream = v.stream;
                header.timestamp = v.timestamp;
                header.flags = (if v.key { flags::KEY } else { 0 })
                    | (if v.discontinuity {
                        flags::DISCONTINUITY
                    } else {
                        0
                    });
                let mut b = Vec::with_capacity(VIDEO_SUBHEADER_LEN + v.data.len());
                b.extend_from_slice(&v.frame_number.to_be_bytes());
                b.extend_from_slice(&v.config_id.to_be_bytes());
                b.extend_from_slice(&[0; 2]);
                b.extend_from_slice(&v.data);
                b
            }
            Message::Unknown { stream, .. } => {
                header.stream = *stream;
                Vec::new()
            }
        };
        header.length = payload.len() as u32;
        let mut out = Vec::with_capacity(HEADER_LEN + payload.len());
        out.extend_from_slice(&header.encode());
        out.extend_from_slice(&payload);
        out
    }

    /// Decodes a message from its header and complete payload (`payload.len() == header.length`).
    pub fn decode(header: &Header, payload: &[u8]) -> Result<Message, Error> {
        let t = header.msg_type;
        if types::is_json(t) && payload.len() > MAX_JSON_LEN {
            return Err(Error::JsonTooLarge {
                msg_type: t,
                length: payload.len() as u32,
            });
        }
        Ok(match t {
            types::HELLO => Message::Hello(from_json(t, payload)?),
            types::BYE => Message::Bye(from_json(t, payload)?),
            types::REQUEST => Message::Request(from_json(t, payload)?),
            types::RESPONSE => Message::Response(from_json(t, payload)?),
            types::EVENT => Message::Event(from_json(t, payload)?),
            types::STREAM_CONFIG => Message::StreamConfig {
                stream: header.stream,
                config: from_json(t, payload)?,
            },
            types::STREAM_END => Message::StreamEnd {
                stream: header.stream,
                end: from_json(t, payload)?,
            },
            types::PING => {
                need(t, payload, PING_LEN)?;
                Message::Ping(Ping {
                    seq: be32(payload, 0),
                    t1: be64(payload, 8),
                })
            }
            types::PONG => {
                need(t, payload, PONG_LEN)?;
                Message::Pong(Pong {
                    seq: be32(payload, 0),
                    t1: be64(payload, 8),
                    t2: be64(payload, 16),
                    t3: be64(payload, 24),
                })
            }
            types::VIDEO_FRAME => {
                need(t, payload, VIDEO_SUBHEADER_LEN)?;
                Message::VideoFrame(VideoFrame {
                    stream: header.stream,
                    timestamp: header.timestamp,
                    key: header.flags & flags::KEY != 0,
                    discontinuity: header.flags & flags::DISCONTINUITY != 0,
                    frame_number: be32(payload, 0),
                    config_id: u16::from_be_bytes([payload[4], payload[5]]),
                    data: payload[VIDEO_SUBHEADER_LEN..].to_vec(),
                })
            }
            // Unknown and reserved types (e.g. AUDIO_FRAME) are skipped.
            other => Message::Unknown {
                msg_type: other,
                stream: header.stream,
                length: header.length,
            },
        })
    }
}

fn json<T: Serialize>(value: &T) -> Vec<u8> {
    serde_json::to_vec(value).expect("protocol structs always serialise")
}

fn from_json<T: DeserializeOwned>(msg_type: u8, payload: &[u8]) -> Result<T, Error> {
    // Must be a JSON object (§6); serde would also accept e.g. `null` for all-default structs.
    let value: Value = serde_json::from_slice(payload).map_err(|e| Error::Json {
        msg_type,
        error: e.to_string(),
    })?;
    if !value.is_object() {
        return Err(Error::Json {
            msg_type,
            error: "payload is not a JSON object".into(),
        });
    }
    serde_json::from_value(value).map_err(|e| Error::Json {
        msg_type,
        error: e.to_string(),
    })
}

fn need(msg_type: u8, payload: &[u8], len: usize) -> Result<(), Error> {
    if payload.len() < len {
        return Err(Error::Truncated {
            msg_type,
            need: len,
            got: payload.len(),
        });
    }
    Ok(())
}

fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn be64(b: &[u8], at: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[at..at + 8]);
    u64::from_be_bytes(a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn roundtrip(m: Message) {
        let bytes = m.encode();
        let mut h = [0u8; HEADER_LEN];
        h.copy_from_slice(&bytes[..HEADER_LEN]);
        let header = Header::decode(&h).unwrap();
        assert_eq!(header.length as usize, bytes.len() - HEADER_LEN);
        assert_eq!(Message::decode(&header, &bytes[HEADER_LEN..]).unwrap(), m);
    }

    #[test]
    fn every_type_roundtrips() {
        roundtrip(Message::Hello(Hello {
            app: "a".into(),
            features: vec!["clock".into()],
            ..Default::default()
        }));
        roundtrip(Message::Bye(Bye {
            reason: "user_stop".into(),
            message: None,
        }));
        roundtrip(Message::Ping(Ping {
            seq: 7,
            t1: 123_456,
        }));
        roundtrip(Message::Pong(Pong {
            seq: 7,
            t1: 1,
            t2: 2,
            t3: 3,
        }));
        roundtrip(Message::StreamConfig {
            stream: 1,
            config: StreamConfig {
                config_id: 3,
                width: 1920,
                height: 1080,
                fps: 60,
                ..Default::default()
            },
        });
        roundtrip(Message::VideoFrame(VideoFrame {
            stream: 1,
            timestamp: 99,
            key: true,
            discontinuity: true,
            frame_number: 5,
            config_id: 3,
            data: vec![0, 0, 0, 1, 0x65],
        }));
        roundtrip(Message::StreamEnd {
            stream: 1,
            end: StreamEnd {
                reason: "camera_switch".into(),
            },
        });
        roundtrip(Message::Request(Request {
            id: 1,
            cmd: "request_keyframe".into(),
            args: Value::Null,
        }));
        roundtrip(Message::Response(Response::error(
            1,
            "unsupported",
            Some("no torch".into()),
        )));
        roundtrip(Message::Event(Event {
            event: "orientation".into(),
            data: json!({"device_rotation": 90}),
        }));
    }

    #[test]
    fn unknown_keys_ignored_and_defaults_applied() {
        let h = Header {
            msg_type: types::STREAM_CONFIG,
            stream: 1,
            length: 0,
            ..Default::default()
        };
        let payload = br#"{"config_id":2,"width":640,"height":480,"fps":30,"future_key":{"x":1}}"#;
        let Message::StreamConfig { config, .. } = Message::decode(&h, payload).unwrap() else {
            panic!()
        };
        assert_eq!(config.codec, "h264");
        assert_eq!(config.color, Color::default());
        assert_eq!(config.width, 640);
    }

    #[test]
    fn rejects_non_objects_bad_json_and_short_binaries() {
        let json_h = Header {
            msg_type: types::HELLO,
            ..Default::default()
        };
        assert_eq!(
            Message::decode(&json_h, b"null").unwrap_err().kind(),
            "json"
        );
        assert_eq!(
            Message::decode(&json_h, b"{\"app\": 3}")
                .unwrap_err()
                .kind(),
            "json"
        );
        assert_eq!(
            Message::decode(&json_h, b"\xff").unwrap_err().kind(),
            "json"
        );
        let ping_h = Header {
            msg_type: types::PING,
            ..Default::default()
        };
        assert_eq!(
            Message::decode(&ping_h, &[0; 15]).unwrap_err().kind(),
            "truncated"
        );
        let big = vec![b' '; MAX_JSON_LEN + 1];
        assert_eq!(
            Message::decode(&json_h, &big).unwrap_err().kind(),
            "json_too_large"
        );
    }

    #[test]
    fn unknown_type_is_skipped() {
        let h = Header {
            msg_type: 0x60,
            stream: 4,
            length: 3,
            ..Default::default()
        };
        assert_eq!(
            Message::decode(&h, &[1, 2, 3]).unwrap(),
            Message::Unknown {
                msg_type: 0x60,
                stream: 4,
                length: 3
            }
        );
    }

    #[test]
    fn longer_binary_payloads_are_accepted() {
        // A future minor version may append fields to PING: extra bytes are ignored.
        let h = Header {
            msg_type: types::PING,
            ..Default::default()
        };
        let mut p = Message::Ping(Ping { seq: 1, t1: 2 }).encode()[HEADER_LEN..].to_vec();
        p.extend_from_slice(&[9; 8]);
        assert_eq!(
            Message::decode(&h, &p).unwrap(),
            Message::Ping(Ping { seq: 1, t1: 2 })
        );
    }

    #[test]
    fn capabilities_and_set_mode() {
        let caps = Capabilities::from_value(&json!({
            "cameras": [{"id": "1", "facing": "front", "sensor_orientation": 270, "modes": [
                {"width": 1920, "height": 1080, "fps": 60, "path": "high_speed", "capture_fps": 120},
                {"width": 1280, "height": 720, "fps": 30, "future": true}
            ]}],
            "codecs": ["h264"]
        }))
        .unwrap();
        let cam = &caps.cameras[0];
        assert_eq!(cam.modes[1].path, None);
        assert_eq!(
            SetMode::new(cam, &cam.modes[0]).to_value(),
            json!({"camera": "1", "width": 1920, "height": 1080, "fps": 60, "path": "high_speed"})
        );
        assert!(Capabilities::from_value(&json!({"cameras": 3})).is_err());
    }

    #[test]
    fn codec_data_base64() {
        let mut c = StreamConfig::default();
        c.set_codec_data(&[0, 0, 0, 1, 0x67]);
        assert_eq!(c.codec_data, "AAAAAWc=");
        assert_eq!(c.codec_data_bytes().unwrap(), vec![0, 0, 0, 1, 0x67]);
    }
}
