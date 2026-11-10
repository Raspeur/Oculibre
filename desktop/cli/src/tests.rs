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

use crate::session::{self, End, Sink};

const W: usize = 320;
const H: usize = 240;

#[derive(Default)]
struct Counter {
    pictures: usize,
    yuyv: Vec<u8>,
}

impl Sink for Counter {
    fn show(&mut self, picture: &DecodedYUV<'_>) -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(picture.dimensions(), (W, H));
        crate::yuv::i420_to_yuyv(picture, W, H, &mut self.yuyv);
        assert_eq!(self.yuyv.len(), W * H * 2);
        self.pictures += 1;
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
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let phone = thread::spawn(move || phone(TcpStream::connect(addr).unwrap()));
    let (stream, _) = listener.accept().unwrap();
    let hello = session::host_hello("host-id".into(), "test-host".into(), None);
    let mut sink = Counter::default();
    let result = session::run(stream, &hello, &mut sink, true).map_err(|e| e.to_string());
    phone.join().unwrap();
    (result, sink.pictures)
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
