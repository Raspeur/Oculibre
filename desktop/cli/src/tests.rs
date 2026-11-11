//! Host session against a simulated phone, over a real loopback socket: no phone needed.

use std::io::BufReader;
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use oculibre_protocol::{
    features, streams, write_message, Bye, Hello, Message, MessageReader, Preamble, Role,
    StreamConfig, VideoFrame,
};
use openh264::decoder::DecodedYUV;
use openh264::encoder::Encoder;
use openh264::formats::{YUVBuffer, YUVSource};
use openh264::nal_units;
use serde_json::{json, Value};

use crate::bench;
use crate::session::{self, End, Rotate, Sink, View};
use crate::yuv::Transform;

const W: usize = 320;
const H: usize = 240;

#[derive(Default)]
struct Counter {
    pictures: usize,
    yuyv: Vec<u8>,
    transforms: Vec<Transform>,
}

impl Sink for Counter {
    fn show(
        &mut self,
        picture: &DecodedYUV<'_>,
        transform: Transform,
    ) -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(picture.dimensions(), (W, H));
        let (w, h) = transform.output_size(W, H);
        crate::yuv::convert(picture, transform, w, h, &mut self.yuyv);
        assert_eq!(self.yuyv.len(), W * H * 2);
        self.pictures += 1;
        self.transforms.push(transform);
        Ok(())
    }
}

/// `n` access units from OpenH264, like the phone's encoder: SPS + PPS apart (codec_data),
/// then one access unit per frame (the first one is a keyframe).
fn encode(n: u8) -> (Vec<u8>, Vec<Vec<u8>>) {
    let mut encoder = Encoder::new().unwrap();
    let mut codec_data = Vec::new();
    let mut frames = Vec::new();
    for i in 0..n {
        let mut yuv = vec![i.wrapping_mul(20); W * H * 3 / 2];
        yuv[i as usize * 100] = 255;
        let bitstream = encoder
            .encode(&YUVBuffer::from_vec(yuv, W, H))
            .unwrap()
            .to_vec();
        let mut au = Vec::new();
        for nal in nal_units(&bitstream) {
            let start = nal.iter().position(|&b| b == 1).unwrap() + 1;
            match nal[start] & 0x1f {
                7 | 8 => codec_data.extend_from_slice(nal),
                _ => au.extend_from_slice(nal),
            }
        }
        frames.push(au);
    }
    (codec_data, frames)
}

fn phone_hello() -> Hello {
    Hello {
        app: "test-phone".into(),
        id: "00000000-0000-4000-8000-000000000000".into(),
        name: "Simulated".into(),
        features: vec![
            features::VIDEO_H264.into(),
            features::CLOCK.into(),
            features::STATS.into(),
        ],
        ..Default::default()
    }
}

/// Runs the host session on one side of a socket and `phone` on the other.
fn with_phone(phone: impl FnOnce(TcpStream) + Send + 'static) -> (Result<End, String>, usize) {
    let (result, sink) = with_phone_view(View::default(), phone);
    (result, sink.pictures)
}

fn with_phone_view(
    view: View,
    phone: impl FnOnce(TcpStream) + Send + 'static,
) -> (Result<End, String>, Counter) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let phone = thread::spawn(move || phone(TcpStream::connect(addr).unwrap()));
    let (stream, _) = listener.accept().unwrap();
    let hello = session::host_hello("host-id".into(), "test-host".into(), None);
    let mut sink = Counter::default();
    let result = session::run(stream, &hello, view, &mut sink, true).map_err(|e| e.to_string());
    phone.join().unwrap();
    (result, sink)
}

/// Preambles and HELLOs from the phone side; returns a reader of host messages.
fn handshake(s: &mut TcpStream) -> MessageReader<BufReader<TcpStream>> {
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    std::io::Write::write_all(s, &Preamble::new(Role::Camera).encode()).unwrap();
    let mut r = MessageReader::new(BufReader::new(s.try_clone().unwrap()));
    assert_eq!(r.read_preamble().unwrap().role, Role::Host);
    write_message(s, &Message::Hello(phone_hello())).unwrap();
    match r.read_message().unwrap() {
        Message::Hello(h) => assert_eq!(h.app, "oculibre-desktop"),
        other => panic!("expected HELLO, got {other:?}"),
    }
    r
}

fn config(id: u16, codec_data: &[u8]) -> Message {
    let mut c = StreamConfig {
        config_id: id,
        width: W as u32,
        height: H as u32,
        fps: 30,
        ..Default::default()
    };
    c.set_codec_data(codec_data);
    Message::StreamConfig {
        stream: streams::VIDEO,
        config: c,
    }
}

/// STREAM_CONFIG of a back camera whose sensor is mounted at 90°, as on most phones.
fn config_back90(id: u16, codec_data: &[u8]) -> Message {
    let Message::StreamConfig { stream, mut config } = config(id, codec_data) else {
        unreachable!()
    };
    config.camera = Some(oculibre_protocol::CameraInfo {
        id: "0".into(),
        facing: "back".into(),
        sensor_orientation: 90,
        mirrored: false,
    });
    Message::StreamConfig { stream, config }
}

fn rotation_event(device_rotation: serde_json::Value) -> Message {
    Message::Event(oculibre_protocol::Event {
        event: "orientation".into(),
        data: serde_json::json!({ "device_rotation": device_rotation }),
    })
}

fn frame(n: u32, config_id: u16, data: &[u8], key: bool, discontinuity: bool) -> Message {
    Message::VideoFrame(VideoFrame {
        stream: streams::VIDEO,
        timestamp: 1_000 + u64::from(n) * 33_333,
        key,
        discontinuity,
        frame_number: n,
        config_id,
        data: data.to_vec(),
    })
}

/// Reads host messages until a REQUEST arrives (answering PINGs on the way).
fn wait_request(s: &mut TcpStream, r: &mut MessageReader<BufReader<TcpStream>>) -> String {
    loop {
        match r.read_message().unwrap() {
            Message::Request(req) => return req.cmd,
            Message::Ping(p) => write_message(
                s,
                &Message::Pong(oculibre_protocol::Pong {
                    seq: p.seq,
                    t1: p.t1,
                    t2: 5,
                    t3: 6,
                }),
            )
            .unwrap(),
            _ => {}
        }
    }
}

#[test]
fn every_frame_is_shown_without_delay_then_bye() {
    let (codec_data, frames) = encode(10);
    let (result, pictures) = with_phone(move |mut s| {
        let _r = handshake(&mut s);
        write_message(&mut s, &config(1, &codec_data)).unwrap();
        for (i, au) in frames.iter().enumerate() {
            write_message(&mut s, &frame(i as u32, 1, au, i == 0, false)).unwrap();
        }
        write_message(
            &mut s,
            &Message::Bye(Bye {
                reason: "user_stop".into(),
                message: None,
            }),
        )
        .unwrap();
    });
    assert_eq!(
        result.unwrap(),
        End::Bye {
            reason: "user_stop".into(),
            message: None
        }
    );
    // One picture per access unit, including the last one: no frame stays stuck in the host.
    assert_eq!(pictures, 10);
}

#[test]
fn unknown_config_asks_for_a_keyframe() {
    let (codec_data, frames) = encode(2);
    let (result, pictures) = with_phone(move |mut s| {
        let mut r = handshake(&mut s);
        write_message(&mut s, &config(1, &codec_data)).unwrap();
        write_message(&mut s, &frame(0, 7, &frames[0], true, false)).unwrap();
        assert_eq!(wait_request(&mut s, &mut r), "request_keyframe");
        drop(s); // closing without BYE
    });
    assert_eq!(result.unwrap(), End::Closed);
    assert_eq!(pictures, 0);
}

#[test]
fn gap_without_keyframe_asks_for_one() {
    let (codec_data, frames) = encode(3);
    let (result, pictures) = with_phone(move |mut s| {
        let mut r = handshake(&mut s);
        write_message(&mut s, &config(1, &codec_data)).unwrap();
        write_message(&mut s, &frame(0, 1, &frames[0], true, false)).unwrap();
        // Frame 1 was dropped by the phone: frame 2 cannot be decoded correctly.
        write_message(&mut s, &frame(2, 1, &frames[2], false, true)).unwrap();
        assert_eq!(wait_request(&mut s, &mut r), "request_keyframe");
    });
    assert_eq!(result.unwrap(), End::Closed);
    assert_eq!(pictures, 1);
}

#[test]
fn not_an_oculibre_peer() {
    let (result, _) = with_phone(|mut s| {
        std::io::Write::write_all(&mut s, b"GET / HTTP/1.1\r\n\r\n").unwrap();
        let _ = std::io::Read::read(&mut s, &mut [0; 64]);
    });
    assert!(result.unwrap_err().contains("not an Oculibre peer"));
}

/// The phone turns: upright, top edge on the left, flat on the table, top edge on the right.
fn turning_phone(mut s: TcpStream, frames: Vec<Vec<u8>>, codec_data: Vec<u8>) {
    let _r = handshake(&mut s);
    write_message(&mut s, &config_back90(1, &codec_data)).unwrap();
    let rotations = [
        None,
        Some(serde_json::json!(90)),
        Some(serde_json::Value::Null),
        Some(serde_json::json!(270)),
        Some(serde_json::json!(45)), // invalid: ignored
    ];
    for (i, r) in rotations.into_iter().enumerate() {
        if let Some(r) = r {
            write_message(&mut s, &rotation_event(r)).unwrap();
        }
        write_message(&mut s, &frame(i as u32, 1, &frames[i], i == 0, false)).unwrap();
    }
}

#[test]
fn auto_rotation_follows_the_phone() {
    let (codec_data, frames) = encode(5);
    let (result, sink) = with_phone_view(View::default(), move |s| {
        turning_phone(s, frames, codec_data)
    });
    assert_eq!(result.unwrap(), End::Closed);
    let rotations: Vec<u16> = sink.transforms.iter().map(|t| t.rotation).collect();
    // upright → 90, top left → 0, flat → unchanged, top right → 180, invalid → unchanged
    assert_eq!(rotations, vec![90, 0, 0, 180, 180]);
    assert!(sink.transforms.iter().all(|t| !t.mirror));
}

#[test]
fn fixed_rotation_ignores_the_phone() {
    let (codec_data, frames) = encode(5);
    let view = View {
        rotate: Rotate::Fixed(270),
        mirror: true,
    };
    let (result, sink) = with_phone_view(view, move |s| turning_phone(s, frames, codec_data));
    assert_eq!(result.unwrap(), End::Closed);
    let expected = Transform {
        rotation: 270,
        mirror: true,
    };
    assert_eq!(sink.transforms, vec![expected; 5]);
}

/// A phone that obeys `set_mode` (protocol 0.2): one camera, two modes; it streams the first one
/// and refuses the second one, like a camera that cannot open.
fn obedient_phone(s: TcpStream, codec_data: Vec<u8>, frames: Vec<Vec<u8>>) -> Vec<Value> {
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};

    let mut s = s;
    let mut r = {
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        std::io::Write::write_all(&mut s, &Preamble::new(Role::Camera).encode()).unwrap();
        let mut r = MessageReader::new(BufReader::new(s.try_clone().unwrap()));
        r.read_preamble().unwrap();
        let mut hello = phone_hello();
        hello.features.push(features::CONTROL_MODE.into());
        hello.capabilities = Some(json!({
            "cameras": [{"id": "0", "facing": "back", "sensor_orientation": 90, "modes": [
                {"width": W, "height": H, "fps": 30, "path": "normal"},
                {"width": W, "height": H, "fps": 60, "path": "unlisted"},
            ]}],
            "codecs": ["h264"],
        }));
        write_message(&mut s, &Message::Hello(hello)).unwrap();
        assert!(matches!(r.read_message().unwrap(), Message::Hello(_)));
        r
    };
    let writer = Arc::new(Mutex::new(s.try_clone().unwrap()));
    let send = |w: &Arc<Mutex<TcpStream>>, m: &Message| write_message(&mut *w.lock().unwrap(), m);

    // Reader: answers PINGs, hands requests over.
    let (tx, rx) = mpsc::channel();
    let reader = {
        let w = Arc::clone(&writer);
        thread::spawn(move || loop {
            match r.read_message() {
                Ok(Message::Ping(p)) => {
                    let pong = Message::Pong(oculibre_protocol::Pong {
                        seq: p.seq,
                        t1: p.t1,
                        t2: p.t1,
                        t3: p.t1,
                    });
                    let _ = send(&w, &pong);
                }
                Ok(Message::Request(req)) => {
                    if tx.send(req).is_err() {
                        return;
                    }
                }
                Ok(Message::Bye(_)) | Err(_) => return,
                Ok(_) => {}
            }
        })
    };

    let mut asked = Vec::new();
    let mut streaming = false;
    let mut config_id = 0u16;
    let mut n = 0u32;
    loop {
        match rx.try_recv() {
            Ok(req) => {
                assert_eq!(req.cmd, "set_mode");
                asked.push(req.args.clone());
                let response = if req.args["fps"] == 30 {
                    oculibre_protocol::Response::ok(req.id)
                } else {
                    oculibre_protocol::Response::error(
                        req.id,
                        "failed",
                        Some("camera refused".into()),
                    )
                };
                if response.ok {
                    let end = Message::StreamEnd {
                        stream: streams::VIDEO,
                        end: oculibre_protocol::StreamEnd {
                            reason: "mode_change".into(),
                        },
                    };
                    config_id += 1;
                    let ok = send(&writer, &end)
                        .and_then(|_| send(&writer, &config(config_id, &codec_data)));
                    if ok.is_err() {
                        break;
                    }
                    streaming = true;
                    n = 0;
                }
                if send(&writer, &Message::Response(response)).is_err() {
                    break;
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => break, // BYE from the host
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if streaming {
            let i = n as usize % frames.len();
            if send(&writer, &frame(n, config_id, &frames[i], i == 0, false)).is_err() {
                break;
            }
            n += 1;
        }
        thread::sleep(Duration::from_millis(33));
    }
    reader.join().unwrap();
    asked
}

#[test]
fn bench_drives_the_phone_through_every_mode() {
    let (codec_data, frames) = encode(15);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let phone = thread::spawn(move || {
        obedient_phone(TcpStream::connect(addr).unwrap(), codec_data, frames)
    });
    let (stream, _) = listener.accept().unwrap();
    let hello = session::host_hello("host-id".into(), "test-host".into(), None);
    let mut sink = Counter::default();
    let outcome = std::sync::Mutex::new(None);
    let settings = bench::Settings {
        warmup: 1,
        seconds: 2,
    };
    let driver: session::Driver<'_> = Box::new(|control| {
        let o = bench::run(&control, settings);
        control.finish("done");
        *outcome.lock().unwrap() = Some(o);
    });
    let end = session::run_driven(
        stream,
        &hello,
        View::default(),
        &mut sink,
        true,
        Some(driver),
    );
    assert_eq!(end.map_err(|e| e.to_string()).unwrap(), End::Finished);
    let asked = phone.join().unwrap();
    assert_eq!(
        asked,
        vec![
            json!({"camera": "0", "width": W, "height": H, "fps": 30, "path": "normal"}),
            json!({"camera": "0", "width": W, "height": H, "fps": 60, "path": "unlisted"}),
        ]
    );

    let outcome = outcome.into_inner().unwrap().unwrap().unwrap();
    assert_eq!(outcome.planned, 2);
    assert_eq!(outcome.interrupted, None);
    let m = outcome.rows[0].result.as_ref().unwrap();
    assert_eq!(m.seconds, 2);
    assert!(m.shown_fps > 10.0, "{m:?}");
    assert_eq!(m.decode_errors, 0);
    assert_eq!(
        outcome.rows[1].result,
        Err("failed (camera refused)".to_string())
    );
    assert!(sink.pictures > 30);
}

#[test]
fn bench_needs_a_phone_that_switches_modes() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let phone = thread::spawn(move || {
        let mut s = TcpStream::connect(addr).unwrap();
        let mut r = handshake(&mut s); // a 0.1 phone: no control.mode
        while let Ok(m) = r.read_message() {
            if matches!(m, Message::Bye(_)) {
                break;
            }
        }
    });
    let (stream, _) = listener.accept().unwrap();
    let hello = session::host_hello("host-id".into(), "test-host".into(), None);
    let mut sink = Counter::default();
    let outcome = std::sync::Mutex::new(None);
    let driver: session::Driver<'_> = Box::new(|control| {
        let o = bench::run(&control, bench::Settings::default());
        control.finish("done");
        *outcome.lock().unwrap() = Some(o);
    });
    let end = session::run_driven(
        stream,
        &hello,
        View::default(),
        &mut sink,
        true,
        Some(driver),
    );
    assert_eq!(end.map_err(|e| e.to_string()).unwrap(), End::Finished);
    phone.join().unwrap();
    let e = outcome.into_inner().unwrap().unwrap().unwrap_err();
    assert!(e.contains("update it (protocol 0.2)"), "{e}");
}
