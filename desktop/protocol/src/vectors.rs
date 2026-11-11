//! Shared test vectors (§16), written to `tests/protocol/vectors/` at the repository root.
//!
//! - `<name>.hex`: the exact bytes, lowercase hex on one line;
//! - `<name>.json`: what a decoder must find in them.
//!
//! Regenerate after a deliberate format change:
//! `cargo run -p oculibre-protocol --example gen_vectors`. The `vectors` test fails if the files
//! on disk differ from this list, and decodes every file independently of the generator.

use serde_json::{json, Map, Value};

use crate::{
    types, Bye, CameraInfo, Color, Device, Event, Header, Hello, Message, Ping, Pong, Preamble,
    Request, Response, Role, StreamConfig, StreamEnd, VideoFrame, HEADER_LEN, MAJOR, MAX_JSON_LEN,
    MAX_MESSAGE_LEN, MINOR,
};

pub enum Expect {
    Preamble(Preamble),
    Message(Box<Message>),
    /// Decoding must fail. `stage` is `preamble` or `message`; `kind` is [`crate::Error::kind`].
    Error {
        stage: &'static str,
        kind: &'static str,
    },
}

pub struct Vector {
    pub name: &'static str,
    pub description: &'static str,
    pub bytes: Vec<u8>,
    pub expect: Expect,
}

fn msg(name: &'static str, description: &'static str, m: Message) -> Vector {
    Vector {
        name,
        description,
        bytes: m.encode(),
        expect: Expect::Message(Box::new(m)),
    }
}

fn invalid(
    name: &'static str,
    description: &'static str,
    stage: &'static str,
    kind: &'static str,
    bytes: Vec<u8>,
) -> Vector {
    Vector {
        name,
        description,
        bytes,
        expect: Expect::Error { stage, kind },
    }
}

/// Header + payload, with `length` set to the payload size.
fn raw(mut header: Header, payload: &[u8]) -> Vec<u8> {
    header.length = payload.len() as u32;
    let mut b = header.encode().to_vec();
    b.extend_from_slice(payload);
    b
}

/// A plausible H.264 SPS + PPS (Annex B) and a tiny IDR slice, so the bytes look real.
const SPS_PPS: &[u8] = &[
    0, 0, 0, 1, 0x67, 0x42, 0xC0, 0x2A, 0x95, 0xA0, 0x1E, 0x00, 0x89, 0xF9, 0x50, // SPS
    0, 0, 0, 1, 0x68, 0xCE, 0x3C, 0x80, // PPS
];
const IDR: &[u8] = &[0, 0, 0, 1, 0x65, 0x88, 0x84, 0x00, 0x33, 0xFF];
const P_SLICE: &[u8] = &[0, 0, 0, 1, 0x41, 0x9A, 0x02, 0x04];

pub fn all() -> Vec<Vector> {
    let mut config = StreamConfig {
        config_id: 1,
        width: 1920,
        height: 1080,
        fps: 60,
        capture_fps: Some(60),
        path: Some("unlisted".into()),
        bitrate: Some(12_000_000),
        color: Color {
            primaries: "bt709".into(),
            transfer: "bt709".into(),
            matrix: "bt709".into(),
            range: "limited".into(),
        },
        camera: Some(CameraInfo {
            id: "0".into(),
            facing: "back".into(),
            sensor_orientation: 90,
            mirrored: false,
        }),
        ..Default::default()
    };
    config.set_codec_data(SPS_PPS);

    vec![
        Vector {
            name: "preamble_camera",
            description: "Preamble sent by a camera speaking 0.3",
            bytes: Preamble::new(Role::Camera).encode().to_vec(),
            expect: Expect::Preamble(Preamble::new(Role::Camera)),
        },
        Vector {
            name: "preamble_host",
            description: "Preamble sent by a host speaking 0.3",
            bytes: Preamble::new(Role::Host).encode().to_vec(),
            expect: Expect::Preamble(Preamble::new(Role::Host)),
        },
        msg(
            "hello_camera",
            "HELLO from a camera",
            Message::Hello(Hello {
                app: "oculibre-android".into(),
                app_version: "0.2.0".into(),
                id: "5b0d7f0e-3f1c-4a8e-9f7d-0c2b2f6f1a11".into(),
                name: "Redmi Note 9 Pro".into(),
                device: Some(Device {
                    manufacturer: Some("Xiaomi".into()),
                    model: Some("Redmi Note 9 Pro".into()),
                    os: Some("android".into()),
                    os_version: Some("12".into()),
                    sdk: Some(31),
                }),
                features: [
                    "video.h264",
                    "clock",
                    "orientation",
                    "stats",
                    "control.keyframe",
                    "control.bitrate",
                ]
                .map(String::from)
                .to_vec(),
                capabilities: None,
                pairing_token: None,
            }),
        ),
        msg(
            "hello_camera_pairing",
            "HELLO from a camera pairing with a computer: one-time token of the QR code (0.3)",
            Message::Hello(Hello {
                app: "oculibre-android".into(),
                app_version: "0.3.0".into(),
                id: "5b0d7f0e-3f1c-4a8e-9f7d-0c2b2f6f1a11".into(),
                name: "Redmi Note 9 Pro".into(),
                device: None,
                features: ["video.h264", "clock", "control.mode"]
                    .map(String::from)
                    .to_vec(),
                capabilities: None,
                pairing_token: Some("3f0c9a2b7d1e4c5f8a6b0e2d9c7f1a3b".into()),
            }),
        ),
        msg(
            "hello_host",
            "HELLO from a host",
            Message::Hello(Hello {
                app: "oculibre-desktop".into(),
                app_version: "0.2.0".into(),
                id: "c1f04a7e-9b2d-4f60-8a1e-2d9c5b7e3f40".into(),
                name: "my-computer".into(),
                device: Some(Device {
                    os: Some("linux".into()),
                    os_version: Some("6.8".into()),
                    ..Default::default()
                }),
                features: ["video.h264", "clock", "orientation", "stats"]
                    .map(String::from)
                    .to_vec(),
                capabilities: None,
                pairing_token: None,
            }),
        ),
        msg(
            "bye_user_stop",
            "BYE after the user stopped streaming",
            Message::Bye(Bye {
                reason: "user_stop".into(),
                message: None,
            }),
        ),
        msg(
            "ping",
            "PING, seq 1, t1 = 1 000 000 µs",
            Message::Ping(Ping {
                seq: 1,
                t1: 1_000_000,
            }),
        ),
        msg(
            "pong",
            "PONG answering ping, responder clock 5 s ahead",
            Message::Pong(Pong {
                seq: 1,
                t1: 1_000_000,
                t2: 6_000_400,
                t3: 6_000_450,
            }),
        ),
        msg(
            "stream_config_1080p60",
            "STREAM_CONFIG for 1080p60 H.264 from the back camera",
            Message::StreamConfig { stream: 1, config },
        ),
        msg(
            "video_frame_key",
            "First VIDEO_FRAME: keyframe of config 1",
            Message::VideoFrame(VideoFrame {
                stream: 1,
                timestamp: 123_456_789,
                key: true,
                discontinuity: false,
                frame_number: 0,
                config_id: 1,
                data: IDR.to_vec(),
            }),
        ),
        msg(
            "video_frame_discontinuity",
            "VIDEO_FRAME after dropped frames (frame numbers 1-4 missing)",
            Message::VideoFrame(VideoFrame {
                stream: 1,
                timestamp: 123_540_122,
                key: false,
                discontinuity: true,
                frame_number: 5,
                config_id: 1,
                data: P_SLICE.to_vec(),
            }),
        ),
        msg(
            "stream_end",
            "STREAM_END while switching camera",
            Message::StreamEnd {
                stream: 1,
                end: StreamEnd {
                    reason: "camera_switch".into(),
                },
            },
        ),
        msg(
            "request_keyframe",
            "REQUEST request_keyframe (no args)",
            Message::Request(Request {
                id: 1,
                cmd: "request_keyframe".into(),
                args: Value::Null,
            }),
        ),
        msg(
            "request_set_bitrate",
            "REQUEST set_bitrate",
            Message::Request(Request {
                id: 2,
                cmd: "set_bitrate".into(),
                args: json!({"bitrate": 8_000_000}),
            }),
        ),
        msg(
            "request_set_mode",
            "REQUEST set_mode: back camera 0, 1080p60 through the unlisted AE range (0.2)",
            Message::Request(Request {
                id: 4,
                cmd: "set_mode".into(),
                args: json!({"camera": "0", "fps": 60, "height": 1080, "path": "unlisted", "width": 1920}),
            }),
        ),
        msg(
            "response_ok",
            "RESPONSE ok to request 1",
            Message::Response(Response::ok(1)),
        ),
        msg(
            "response_error",
            "RESPONSE error unsupported",
            Message::Response(Response::error(
                3,
                "unsupported",
                Some("no torch on this camera".into()),
            )),
        ),
        msg(
            "event_orientation",
            "EVENT orientation, phone turned 90° clockwise",
            Message::Event(Event {
                event: "orientation".into(),
                data: json!({"device_rotation": 90}),
            }),
        ),
        msg(
            "event_orientation_flat",
            "EVENT orientation, phone lying flat",
            Message::Event(Event {
                event: "orientation".into(),
                data: json!({"device_rotation": null}),
            }),
        ),
        msg(
            "event_stats_camera",
            "EVENT stats from the camera",
            Message::Event(Event {
                event: "stats".into(),
                data: json!({"camera_fps": 60, "sent_fps": 60, "bitrate": 11_800_000, "dropped": 0, "queue": 1}),
            }),
        ),
        msg(
            "event_stats_host",
            "EVENT stats from the host",
            Message::Event(Event {
                event: "stats".into(),
                data: json!({"received_fps": 60, "decoded_fps": 60, "decode_errors": 0, "rtt_ms": 3}),
            }),
        ),
        Vector {
            name: "unknown_type",
            description:
                "Type 0x60 (reserved for future versions) with a 4-byte payload: must be skipped",
            bytes: raw(
                Header {
                    msg_type: 0x60,
                    stream: 0,
                    length: 4,
                    ..Default::default()
                },
                &[0xDE, 0xAD, 0xBE, 0xEF],
            ),
            expect: Expect::Message(Box::new(Message::Unknown {
                msg_type: 0x60,
                stream: 0,
                length: 4,
            })),
        },
        Vector {
            name: "hello_future_keys",
            description: "HELLO with keys unknown to 0.1: they must be ignored",
            bytes: raw(
                Header {
                    msg_type: types::HELLO,
                    ..Default::default()
                },
                br#"{"app":"x","features":["clock"],"future":{"a":1},"zzz":[1,2]}"#,
            ),
            expect: Expect::Message(Box::new(Message::Hello(Hello {
                app: "x".into(),
                features: vec!["clock".into()],
                ..Default::default()
            }))),
        },
        // ---------------------------------------------------------------- invalid
        invalid(
            "invalid_preamble_bad_magic",
            "Not an Oculibre peer (an HTTP request)",
            "preamble",
            "bad_magic",
            b"GET / HT".to_vec(),
        ),
        invalid(
            "invalid_preamble_bad_role",
            "Role 3 does not exist",
            "preamble",
            "bad_role",
            vec![b'O', b'C', b'U', b'L', MAJOR, MINOR, 3, 0],
        ),
        invalid(
            "invalid_too_large",
            "length = 16 MiB + 1: rejected from the header, before reading the payload",
            "message",
            "too_large",
            Header {
                msg_type: types::VIDEO_FRAME,
                stream: 1,
                length: MAX_MESSAGE_LEN + 1,
                ..Default::default()
            }
            .encode()
            .to_vec(),
        ),
        invalid(
            "invalid_json_too_large",
            "HELLO announcing 64 KiB + 1: rejected from the header, before reading the payload",
            "message",
            "json_too_large",
            Header {
                msg_type: types::HELLO,
                length: (MAX_JSON_LEN + 1) as u32,
                ..Default::default()
            }
            .encode()
            .to_vec(),
        ),
        invalid(
            "invalid_ping_truncated",
            "PING with a 12-byte payload (needs 16)",
            "message",
            "truncated",
            raw(
                Header {
                    msg_type: types::PING,
                    length: 12,
                    ..Default::default()
                },
                &[0; 12],
            ),
        ),
        invalid(
            "invalid_video_truncated",
            "VIDEO_FRAME shorter than its 8-byte sub-header",
            "message",
            "truncated",
            raw(
                Header {
                    msg_type: types::VIDEO_FRAME,
                    stream: 1,
                    length: 5,
                    ..Default::default()
                },
                &[0; 5],
            ),
        ),
        invalid(
            "invalid_hello_not_object",
            "HELLO whose payload is a JSON array",
            "message",
            "json",
            raw(
                Header {
                    msg_type: types::HELLO,
                    length: 2,
                    ..Default::default()
                },
                b"[]",
            ),
        ),
        invalid(
            "invalid_hello_bad_json",
            "HELLO whose payload is not JSON",
            "message",
            "json",
            raw(
                Header {
                    msg_type: types::HELLO,
                    length: 5,
                    ..Default::default()
                },
                b"{oops",
            ),
        ),
        invalid(
            "invalid_request_bad_field",
            "REQUEST whose id is a string",
            "message",
            "json",
            raw(
                Header {
                    msg_type: types::REQUEST,
                    length: 30,
                    ..Default::default()
                },
                br#"{"id":"7","cmd":"set_bitrate"}"#,
            ),
        ),
    ]
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    let s: String = s.split_whitespace().collect();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// The `.json` description of a vector.
pub fn describe(v: &Vector) -> Value {
    let mut d = Map::new();
    d.insert("description".into(), v.description.into());
    match &v.expect {
        Expect::Preamble(p) => {
            d.insert("kind".into(), "preamble".into());
            d.insert("major".into(), p.major.into());
            d.insert("minor".into(), p.minor.into());
            d.insert("role".into(), p.role.name().into());
        }
        Expect::Error { stage, kind } => {
            d.insert("kind".into(), "error".into());
            d.insert("stage".into(), (*stage).into());
            d.insert("expect_error".into(), (*kind).into());
        }
        Expect::Message(m) => {
            let mut hb = [0u8; HEADER_LEN];
            hb.copy_from_slice(&v.bytes[..HEADER_LEN]);
            let h = Header::decode(&hb).expect("valid vector header");
            d.insert("kind".into(), "message".into());
            d.insert("type".into(), types::name(h.msg_type).into());
            d.insert(
                "header".into(),
                json!({"type": h.msg_type, "flags": h.flags, "stream": h.stream, "length": h.length, "timestamp": h.timestamp}),
            );
            d.insert("fields".into(), fields(m));
        }
    }
    Value::Object(d)
}

/// Decoded content of a message: the JSON object itself for JSON types, named fields otherwise.
pub fn fields(m: &Message) -> Value {
    let to = |x: Result<Value, serde_json::Error>| x.expect("serialisable");
    match m {
        Message::Hello(x) => to(serde_json::to_value(x)),
        Message::Bye(x) => to(serde_json::to_value(x)),
        Message::Request(x) => to(serde_json::to_value(x)),
        Message::Response(x) => to(serde_json::to_value(x)),
        Message::Event(x) => to(serde_json::to_value(x)),
        Message::StreamConfig { config, .. } => to(serde_json::to_value(config)),
        Message::StreamEnd { end, .. } => to(serde_json::to_value(end)),
        Message::Ping(p) => json!({"seq": p.seq, "t1": p.t1}),
        Message::Pong(p) => json!({"seq": p.seq, "t1": p.t1, "t2": p.t2, "t3": p.t3}),
        Message::VideoFrame(f) => json!({
            "frame_number": f.frame_number,
            "config_id": f.config_id,
            "key": f.key,
            "discontinuity": f.discontinuity,
            "data_hex": to_hex(&f.data),
        }),
        Message::Unknown { length, .. } => json!({"skipped_length": length}),
    }
}
