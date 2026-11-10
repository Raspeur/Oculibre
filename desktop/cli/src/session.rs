//! One camera session on the host side of the Oculibre protocol 0.1 (`docs/protocol.typ`).
//!
//! - Main thread: reads messages, decodes video, hands pictures to a [`Sink`].
//! - Ticker thread: every second, a `PING` (keep-alive + clock), the host `stats` event and the
//!   status line.
//!
//! Nothing here touches v4l2, so the tests drive it with a simulated phone over a real socket.

use std::error::Error;
use std::io::{self, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use oculibre_protocol::clock::{self, ClockSync};
use oculibre_protocol::orientation::auto_rotation;
use oculibre_protocol::{
    bye_reasons, error_codes, features, streams, write_message, Bye, Event, Hello, Message,
    MessageReader, Preamble, Request, Response, Role,
};
use openh264::decoder::{DecodedYUV, Decoder};
use serde_json::{json, Value};

use crate::yuv::Transform;

/// Where decoded pictures go (the virtual camera, or a test).
pub trait Sink {
    /// `transform`: rotation and mirror to apply (it can change from one picture to the next).
    fn show(
        &mut self,
        picture: &DecodedYUV<'_>,
        transform: Transform,
    ) -> Result<(), Box<dyn Error>>;
}

/// `--rotate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rotate {
    /// Always this clockwise rotation (0, 90, 180, 270): the picture never turns.
    Fixed(u16),
    /// From the camera's sensor orientation and the phone's rotation (§10).
    Auto,
}

/// How the user wants the picture: `--rotate` and `--mirror`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    pub rotate: Rotate,
    pub mirror: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            rotate: Rotate::Auto,
            mirror: false,
        }
    }
}

/// What `auto` knows about the camera and the phone.
#[derive(Debug, Default)]
struct Orientation {
    sensor: u16,
    front: bool,
    /// Last known phone rotation (counter-clockwise); kept while the phone lies flat (§10).
    device: u16,
}

impl Orientation {
    fn transform(&self, view: View) -> Transform {
        let rotation = match view.rotate {
            Rotate::Fixed(r) => r % 360,
            Rotate::Auto => auto_rotation(self.sensor, self.front, self.device),
        };
        Transform {
            rotation,
            mirror: view.mirror,
        }
    }
}

/// Silence after which the phone is considered gone (§7.3, §14).
const SILENCE: Duration = Duration::from_secs(5);
/// Minimum time between two `request_keyframe` (the phone needs a moment to produce one).
const KEYFRAME_RETRY: Duration = Duration::from_millis(500);

/// Features this host understands (§15).
pub const HOST_FEATURES: [&str; 4] = [
    features::VIDEO_H264,
    features::CLOCK,
    features::ORIENTATION,
    features::STATS,
];

/// Why a session ended normally.
#[derive(Debug, PartialEq, Eq)]
pub enum End {
    /// The phone sent `BYE`.
    Bye {
        reason: String,
        message: Option<String>,
    },
    /// The phone closed the connection without `BYE`.
    Closed,
}

/// State shared with the ticker thread.
struct Shared {
    writer: Mutex<TcpStream>,
    epoch: Instant,
    stop: AtomicBool,
    next_id: AtomicU32,
    clock: Mutex<ClockSync>,
    // Counters since the last tick.
    received: AtomicU32,
    decoded: AtomicU32,
    errors_total: AtomicU32,
    bytes: AtomicU32,
    latency_sum_us: Mutex<(i64, u32)>,
    phone_stats: Mutex<Option<Value>>,
    stats_enabled: AtomicBool,
    quiet: bool,
}

impl Shared {
    fn now_us(&self) -> u64 {
        self.epoch.elapsed().as_micros() as u64
    }

    fn send(&self, m: &Message) -> io::Result<()> {
        let mut w = self.writer.lock().unwrap_or_else(|e| e.into_inner());
        write_message(&mut *w, m)
    }

    fn request(&self, cmd: &str) -> io::Result<u32> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.send(&Message::Request(Request {
            id,
            cmd: cmd.into(),
            args: Value::Null,
        }))?;
        Ok(id)
    }
}

/// Our `HELLO`.
pub fn host_hello(id: String, name: String, os_version: Option<String>) -> Hello {
    Hello {
        app: "oculibre-desktop".into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        id,
        name,
        device: Some(oculibre_protocol::Device {
            os: Some(std::env::consts::OS.into()),
            os_version,
            ..Default::default()
        }),
        features: HOST_FEATURES.iter().map(|s| s.to_string()).collect(),
        capabilities: None,
    }
}

/// Runs a session until the phone leaves. `quiet` disables the status line (tests).
pub fn run(
    stream: TcpStream,
    hello: &Hello,
    view: View,
    sink: &mut dyn Sink,
    quiet: bool,
) -> Result<End, Box<dyn Error>> {
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(SILENCE))?;
    let shared = Arc::new(Shared {
        writer: Mutex::new(stream.try_clone()?),
        epoch: Instant::now(),
        stop: AtomicBool::new(false),
        next_id: AtomicU32::new(1),
        clock: Mutex::new(ClockSync::default()),
        received: AtomicU32::new(0),
        decoded: AtomicU32::new(0),
        errors_total: AtomicU32::new(0),
        bytes: AtomicU32::new(0),
        latency_sum_us: Mutex::new((0, 0)),
        phone_stats: Mutex::new(None),
        stats_enabled: AtomicBool::new(false),
        quiet,
    });
    let mut reader = MessageReader::new(io::BufReader::with_capacity(
        256 * 1024,
        stream.try_clone()?,
    ));

    // --- handshake (§4, §7.1)
    let ours = Preamble::new(Role::Host);
    shared.writer.lock().unwrap().write_all(&ours.encode())?;
    let peer = reader.read_preamble().map_err(|e| match e.kind() {
        "io" => "the phone did not answer within 5 s".into(),
        _ => format!("{e}"),
    })?;
    ours.check_peer(&peer)?;
    shared.send(&Message::Hello(hello.clone()))?;
    let phone = loop {
        match reader.read_message()? {
            Message::Hello(h) => break h,
            Message::Bye(b) => {
                return Ok(End::Bye {
                    reason: b.reason,
                    message: b.message,
                })
            }
            _ => {} // nothing else may come before HELLO: ignore
        }
    };
    let shared_features: Vec<&str> = HOST_FEATURES
        .iter()
        .copied()
        .filter(|f| phone.has_feature(f))
        .collect();
    if !phone.has_feature(features::VIDEO_H264) {
        let _ = shared.send(&Message::Bye(Bye {
            reason: bye_reasons::OTHER.into(),
            message: Some("H.264 is required".into()),
        }));
        return Err("the phone does not offer H.264 video".into());
    }
    shared.stats_enabled.store(
        shared_features.contains(&features::STATS),
        Ordering::Relaxed,
    );
    let clock_enabled = shared_features.contains(&features::CLOCK);
    if !quiet {
        let model = phone
            .device
            .as_ref()
            .and_then(|d| d.model.clone())
            .unwrap_or_default();
        println!(
            "Phone: {} ({} {}, {} {})",
            if phone.name.is_empty() {
                &model
            } else {
                &phone.name
            },
            phone.app,
            phone.app_version,
            phone
                .device
                .as_ref()
                .and_then(|d| d.os.clone())
                .unwrap_or_default(),
            phone
                .device
                .as_ref()
                .and_then(|d| d.os_version.clone())
                .unwrap_or_default(),
        );
    }

    let ticker = {
        let shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("oculibre-ticker".into())
            .spawn(move || ticker(&shared))?
    };

    let result = receive(&mut reader, &shared, view, sink, clock_enabled);

    // Polite goodbye on our own errors; the phone may already be gone.
    if let Err(e) = &result {
        if let Some(pe) = e.downcast_ref::<oculibre_protocol::Error>() {
            if !matches!(pe.kind(), "io" | "connection_closed") {
                let _ = shared.send(&Message::Bye(Bye {
                    reason: bye_reasons::PROTOCOL_ERROR.into(),
                    message: Some(pe.to_string()),
                }));
            }
        }
    }
    shared.stop.store(true, Ordering::Relaxed);
    let _ = stream.shutdown(Shutdown::Both);
    let _ = ticker.join();
    result
}

fn receive(
    reader: &mut MessageReader<io::BufReader<TcpStream>>,
    shared: &Shared,
    view: View,
    sink: &mut dyn Sink,
    clock_enabled: bool,
) -> Result<End, Box<dyn Error>> {
    let mut decoder = Decoder::new()?;
    let mut orientation = Orientation::default();
    let mut transform = orientation.transform(view);
    let mut announced: Option<Transform> = None;
    let mut config_id: Option<u16> = None;
    let mut need_key = true;
    let mut last_key_request: Option<Instant> = None;

    let ask_keyframe = |last: &mut Option<Instant>| -> io::Result<()> {
        if last.is_none_or(|t| t.elapsed() >= KEYFRAME_RETRY) {
            *last = Some(Instant::now());
            shared.request("request_keyframe")?;
        }
        Ok(())
    };

    loop {
        let message = match reader.read_message() {
            Ok(m) => m,
            Err(e) if e.kind() == "connection_closed" => return Ok(End::Closed),
            Err(oculibre_protocol::Error::Io(e))
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err("the phone stopped sending (nothing for 5 s)".into())
            }
            Err(e) => return Err(Box::new(e)),
        };
        match message {
            Message::StreamConfig { stream, config } if stream == streams::VIDEO => {
                if config.codec != "h264" {
                    return Err(format!("unsupported codec {:?}", config.codec).into());
                }
                let codec_data = config.codec_data_bytes()?;
                if !shared.quiet {
                    let camera = config
                        .camera
                        .as_ref()
                        .map(|c| {
                            format!(", {} camera, sensor at {}°", c.facing, c.sensor_orientation)
                        })
                        .unwrap_or_default();
                    println!(
                        "\nStream: {}x{} at {} fps ({} capture{camera})",
                        config.width,
                        config.height,
                        config.fps,
                        config.path.as_deref().unwrap_or("normal"),
                    );
                }
                if !codec_data.is_empty() {
                    // SPS + PPS: no picture expected; errors show up on the next frame anyway.
                    let _ = decoder.decode(&codec_data);
                }
                if let Some(c) = &config.camera {
                    orientation.sensor = c.sensor_orientation % 360;
                    orientation.front = c.facing == "front";
                }
                transform = orientation.transform(view);
                config_id = Some(config.config_id);
                need_key = true;
            }
            Message::VideoFrame(frame) if frame.stream == streams::VIDEO => {
                shared.received.fetch_add(1, Ordering::Relaxed);
                shared
                    .bytes
                    .fetch_add(frame.data.len() as u32, Ordering::Relaxed);
                if config_id != Some(frame.config_id) {
                    // §8.2: a frame of a config we don't have.
                    ask_keyframe(&mut last_key_request)?;
                    continue;
                }
                if frame.discontinuity && !frame.key {
                    need_key = true;
                }
                if need_key && !frame.key {
                    ask_keyframe(&mut last_key_request)?;
                    continue;
                }
                match decoder.decode(&frame.data) {
                    Ok(Some(picture)) => {
                        need_key = false;
                        if announced != Some(transform) && !shared.quiet {
                            announced = Some(transform);
                            println!("\n{}", describe(transform, view));
                        }
                        sink.show(&picture, transform)?;
                        shared.decoded.fetch_add(1, Ordering::Relaxed);
                        if clock_enabled {
                            record_latency(shared, frame.timestamp);
                        }
                    }
                    Ok(None) => {}
                    Err(_) => {
                        shared.errors_total.fetch_add(1, Ordering::Relaxed);
                        need_key = true;
                        ask_keyframe(&mut last_key_request)?;
                    }
                }
            }
            Message::Ping(p) => {
                let t2 = shared.now_us();
                shared.send(&Message::Pong(oculibre_protocol::Pong {
                    seq: p.seq,
                    t1: p.t1,
                    t2,
                    t3: shared.now_us(),
                }))?;
            }
            Message::Pong(p) => {
                let s = clock::sample(p.t1, p.t2, p.t3, shared.now_us());
                shared.clock.lock().unwrap().add(s);
            }
            Message::Event(Event { event, data }) => match event.as_str() {
                "stats" => *shared.phone_stats.lock().unwrap() = Some(data),
                "orientation" => {
                    // null (flat, unknown) or an invalid value: keep the previous one (§10).
                    if let Some(r) = data
                        .get("device_rotation")
                        .and_then(Value::as_u64)
                        .filter(|r| [0, 90, 180, 270].contains(r))
                    {
                        orientation.device = r as u16;
                        transform = orientation.transform(view);
                    }
                }
                _ => {}
            },
            Message::Request(r) => {
                // The host handles no command in 0.1 (§9.1).
                shared.send(&Message::Response(Response::error(
                    r.id,
                    error_codes::UNKNOWN_COMMAND,
                    Some(r.cmd),
                )))?;
            }
            Message::Response(r) => {
                if !r.ok && !shared.quiet {
                    let e = r.error.unwrap_or_default();
                    eprintln!(
                        "\nphone refused request {}: {} {}",
                        r.id,
                        e.code,
                        e.message.unwrap_or_default()
                    );
                }
            }
            Message::Bye(b) => {
                return Ok(End::Bye {
                    reason: b.reason,
                    message: b.message,
                })
            }
            // A second HELLO, other streams, STREAM_END, unknown types: nothing to do yet.
            _ => {}
        }
    }
}

/// "Rotation: 90° (auto)" for the console.
fn describe(t: Transform, view: View) -> String {
    let mode = match view.rotate {
        Rotate::Auto => " (auto: follows the phone)",
        Rotate::Fixed(_) => "",
    };
    let mirror = if t.mirror { ", mirrored" } else { "" };
    format!("Rotation: {}°{mode}{mirror}", t.rotation)
}

/// Capture-to-display latency (§11): the phone's capture time brought to our clock.
fn record_latency(shared: &Shared, capture_us: u64) {
    let Some(best) = shared.clock.lock().unwrap().best() else {
        return;
    };
    // offset = phone clock - host clock (the phone answered our PING).
    let capture_host = capture_us as i64 - best.offset_us;
    let latency = shared.now_us() as i64 - capture_host;
    if (0..10_000_000).contains(&latency) {
        let mut l = shared.latency_sum_us.lock().unwrap();
        l.0 += latency;
        l.1 += 1;
    }
}

/// Every second: PING, host stats, status line. Stops when the session ends or a write fails.
fn ticker(shared: &Shared) {
    let mut seq = 0u32;
    let mut last = Instant::now();
    while !shared.stop.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(100));
        if last.elapsed() < Duration::from_secs(1) {
            continue;
        }
        let secs = last.elapsed().as_secs_f64();
        last = Instant::now();
        seq = seq.wrapping_add(1);

        let received = shared.received.swap(0, Ordering::Relaxed);
        let decoded = shared.decoded.swap(0, Ordering::Relaxed);
        let bytes = shared.bytes.swap(0, Ordering::Relaxed);
        let errors = shared.errors_total.load(Ordering::Relaxed);
        let (lat_sum, lat_n) = std::mem::take(&mut *shared.latency_sum_us.lock().unwrap());
        let rtt_ms = shared
            .clock
            .lock()
            .unwrap()
            .best()
            .map(|s| s.rtt_us as f64 / 1000.0);

        let ping = Message::Ping(oculibre_protocol::Ping {
            seq,
            t1: shared.now_us(),
        });
        let mut ok = shared.send(&ping).is_ok();
        if ok && shared.stats_enabled.load(Ordering::Relaxed) {
            let mut data = json!({
                "received_fps": (received as f64 / secs).round() as u64,
                "decoded_fps": (decoded as f64 / secs).round() as u64,
                "decode_errors": errors,
            });
            if let Some(rtt) = rtt_ms {
                data["rtt_ms"] = json!((rtt * 10.0).round() / 10.0);
            }
            ok = shared
                .send(&Message::Event(Event {
                    event: "stats".into(),
                    data,
                }))
                .is_ok();
        }
        if !ok {
            // Unblock the reader: the session is over.
            if let Ok(w) = shared.writer.lock() {
                let _ = w.shutdown(Shutdown::Both);
            }
            return;
        }

        if !shared.quiet {
            let mut line = format!(
                "\r{:>3.0} fps · {:>4.1} Mbit/s",
                decoded as f64 / secs,
                bytes as f64 * 8.0 / secs / 1e6
            );
            if lat_n > 0 {
                line += &format!(
                    " · latency {:>3.0} ms",
                    lat_sum as f64 / lat_n as f64 / 1000.0
                );
            }
            if let Some(rtt) = rtt_ms {
                line += &format!(" · rtt {rtt:.1} ms");
            }
            if let Some(p) = shared.phone_stats.lock().unwrap().as_ref() {
                if let Some(d) = p.get("dropped").and_then(Value::as_u64) {
                    line += &format!(" · dropped by phone {d}");
                }
            }
            line += &format!(" · decode errors {errors}   ");
            print!("{line}");
            let _ = io::stdout().flush();
        }
    }
}
