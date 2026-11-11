//! One camera session on the host side of the Oculibre protocol 0.2 (`docs/protocol.typ`).
//!
//! - Main thread: reads messages, decodes video, hands pictures to a [`Sink`].
//! - Ticker thread: every second, a `PING` (keep-alive + clock), the host `stats` event and the
//!   status line.
//! - Optional driver thread ([`run_driven`]): sends commands to the phone through a [`Control`]
//!   and reads the per-second measurements (`oculibre bench`).
//!
//! Nothing here touches v4l2, so the tests drive it with a simulated phone over a real socket.

use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::io::{self, Write};
use std::net::{Shutdown, TcpStream};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use oculibre_protocol::clock::{self, ClockSync};
use oculibre_protocol::orientation::auto_rotation;
use oculibre_protocol::{
    bye_reasons, error_codes, features, streams, write_message, Bye, Event, Hello, Message,
    MessageReader, Preamble, Request, Response, Role, StreamConfig,
};
use serde_json::{json, Value};

use crate::decoder::VideoDecoder;
use crate::yuv::{Picture, Transform};

/// The byte stream of a session: a TCP socket (USB, through `adb reverse`) or the plaintext side of
/// a TLS connection (Wi-Fi, see `tls.rs`).
pub trait Link: io::Read + io::Write + Send + Sync + 'static {
    fn try_clone_link(&self) -> io::Result<Box<dyn Link>>;
    fn set_read_timeout_link(&self, timeout: Option<Duration>) -> io::Result<()>;
    fn shutdown_link(&self) -> io::Result<()>;
}

impl Link for TcpStream {
    fn try_clone_link(&self) -> io::Result<Box<dyn Link>> {
        Ok(Box::new(self.try_clone()?))
    }
    fn set_read_timeout_link(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.set_read_timeout(timeout)
    }
    fn shutdown_link(&self) -> io::Result<()> {
        self.shutdown(Shutdown::Both)
    }
}

impl Link for UnixStream {
    fn try_clone_link(&self) -> io::Result<Box<dyn Link>> {
        Ok(Box::new(self.try_clone()?))
    }
    fn set_read_timeout_link(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.set_read_timeout(timeout)
    }
    fn shutdown_link(&self) -> io::Result<()> {
        self.shutdown(Shutdown::Both)
    }
}

impl Link for Box<dyn Link> {
    fn try_clone_link(&self) -> io::Result<Box<dyn Link>> {
        (**self).try_clone_link()
    }
    fn set_read_timeout_link(&self, timeout: Option<Duration>) -> io::Result<()> {
        (**self).set_read_timeout_link(timeout)
    }
    fn shutdown_link(&self) -> io::Result<()> {
        (**self).shutdown_link()
    }
}

/// Decides, from its `HELLO`, whether the phone may stream (pairing, §13). `Err`: why not; the
/// phone gets `BYE unpaired`.
pub type Authorize<'a> = &'a (dyn Fn(&Hello) -> Result<(), String> + Sync);

/// Where decoded pictures go (the virtual camera, or a test).
pub trait Sink {
    /// `transform`: rotation and mirror to apply (it can change from one picture to the next).
    fn show(&mut self, picture: &Picture, transform: Transform) -> Result<(), Box<dyn Error>>;
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
    /// We ended the session on purpose ([`Control::finish`]).
    Finished,
}

/// Latency of the frames shown during the last second, µs.
#[derive(Debug, Default)]
struct Latency {
    /// Capture on the phone → written to the virtual camera.
    total_sum: i64,
    total_max: i64,
    /// Fully received → written to the virtual camera (decode, rotation, conversion, write).
    pc_sum: i64,
    /// The H.264 decoding part of `pc_sum`.
    decode_sum: i64,
    count: u32,
    /// Frames whose latency came out absurd (< 0 or ≥ 10 s): the phone's clocks disagree.
    rejected: u32,
}

/// State shared with the ticker thread.
struct Shared {
    writer: Mutex<Box<dyn Link>>,
    epoch: Instant,
    stop: AtomicBool,
    next_id: AtomicU32,
    clock: Mutex<ClockSync>,
    // Counters since the last tick.
    received: AtomicU32,
    decoded: AtomicU32,
    errors_total: AtomicU32,
    bytes: AtomicU32,
    latency: Mutex<Latency>,
    phone_stats: Mutex<Option<Value>>,
    stats_enabled: AtomicBool,
    quiet: bool,
    /// Our requests waiting for their `RESPONSE`, by id.
    pending: Mutex<HashMap<u32, mpsc::Sender<Response>>>,
    /// Number of video `STREAM_CONFIG` received so far, and the last one.
    stream_seq: AtomicU64,
    stream: Mutex<Option<StreamConfig>>,
    /// Where the ticker sends each [`Second`] (only with a driver).
    seconds: Mutex<Option<mpsc::Sender<Second>>>,
    /// We are closing the session ourselves: a read error is the expected end.
    finished: AtomicBool,
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

/// Where the phone's share of the latency goes (its `stats` event, ms): the camera finishing the
/// picture, the H.264 encoder, the send queue (#12).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PhoneSplit {
    pub camera: f64,
    pub encoder: f64,
    pub queue: f64,
}

impl PhoneSplit {
    /// From the phone's `stats` data; `None` for apps that do not send the split (before 0.3).
    fn from_stats(stats: &Value) -> Option<Self> {
        let get = |k: &str| stats.get(k).and_then(Value::as_f64);
        Some(Self {
            camera: get("camera_ms")?,
            encoder: get("encoder_ms")?,
            queue: get("queue_ms")?,
        })
    }

    /// `camera 30 + encoder 45 + queue 2`
    pub fn text(&self) -> String {
        format!(
            "camera {:.0} + encoder {:.0} + queue {:.0}",
            self.camera, self.encoder, self.queue
        )
    }
}

/// What the host measured during one second (ticker period), for a [`Control`] driver.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Second {
    /// [`Control::stream_seq`] at the end of this second.
    pub stream_seq: u64,
    pub shown_fps: f64,
    /// Last `camera_fps` reported by the phone.
    pub camera_fps: Option<f64>,
    pub mbits: f64,
    /// Capture → virtual camera, µs, over the frames shown during this second.
    pub latency_sum_us: i64,
    pub latency_max_us: i64,
    pub latency_count: u32,
    /// Received → virtual camera, µs (same frames).
    pub pc_sum_us: i64,
    /// Capture → network on the phone (its last `stats`), ms.
    pub phone_latency_ms: Option<f64>,
    /// The same, split (its last `stats`).
    pub phone_split: Option<PhoneSplit>,
    pub rtt_ms: Option<f64>,
    /// Totals since the start of the session.
    pub dropped_total: Option<u64>,
    pub errors_total: u32,
}

/// The driver's handle on a running session (see [`run_driven`]).
pub struct Control {
    shared: Arc<Shared>,
    seconds: mpsc::Receiver<Second>,
    /// The phone's `HELLO`.
    pub phone: Hello,
}

impl Control {
    /// Sends a `REQUEST` and waits for its `RESPONSE` (§9.1).
    pub fn call(&self, cmd: &str, args: Value, timeout: Duration) -> Result<Response, String> {
        let id = self.shared.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        self.shared.pending.lock().unwrap().insert(id, tx);
        let request = Message::Request(Request {
            id,
            cmd: cmd.into(),
            args,
        });
        if let Err(e) = self.shared.send(&request) {
            self.shared.pending.lock().unwrap().remove(&id);
            return Err(format!("cannot send {cmd}: {e}"));
        }
        match rx.recv_timeout(timeout) {
            Ok(response) => Ok(response),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.shared.pending.lock().unwrap().remove(&id);
                Err(format!(
                    "the phone did not answer {cmd} within {} s",
                    timeout.as_secs()
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err("the session ended".into()),
        }
    }

    /// Increases by one at every video `STREAM_CONFIG` (a new stream, e.g. after `set_mode`).
    pub fn stream_seq(&self) -> u64 {
        self.shared.stream_seq.load(Ordering::Relaxed)
    }

    /// The last video `STREAM_CONFIG`.
    pub fn stream(&self) -> Option<StreamConfig> {
        self.shared.stream.lock().unwrap().clone()
    }

    /// Waits for the next second of measurements; `None` once the session is over.
    pub fn next_second(&self) -> Option<Second> {
        self.seconds.recv().ok()
    }

    /// Says goodbye (`BYE user_stop`) and ends the session: [`run_driven`] returns
    /// [`End::Finished`].
    pub fn finish(&self, message: &str) {
        self.shared.finished.store(true, Ordering::Relaxed);
        let _ = self.shared.send(&Message::Bye(Bye {
            reason: bye_reasons::USER_STOP.into(),
            message: Some(message.into()),
        }));
        if let Ok(w) = self.shared.writer.lock() {
            let _ = w.shutdown_link();
        }
    }
}

/// Code that drives a session from its own thread (see [`run_driven`]).
pub type Driver<'a> = Box<dyn FnOnce(Control) + Send + 'a>;

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
        pairing_token: None,
    }
}

/// Runs a session until the phone leaves. `quiet` disables the status line (tests).
#[cfg(test)]
pub fn run(
    stream: impl Link,
    hello: &Hello,
    view: View,
    sink: &mut (dyn Sink + Send),
    quiet: bool,
) -> Result<End, Box<dyn Error>> {
    run_driven(stream, hello, view, sink, quiet, None, None)
}

/// [`run`], plus a `driver` started on its own thread once the phone's `HELLO` is in. The session
/// ends when the phone leaves or when the driver calls [`Control::finish`]; the driver is always
/// finished when this returns.
///
/// `authorize` (Wi-Fi): checked on the phone's `HELLO` before we send ours.
pub fn run_driven(
    stream: impl Link,
    hello: &Hello,
    view: View,
    sink: &mut (dyn Sink + Send),
    quiet: bool,
    driver: Option<Driver<'_>>,
    authorize: Option<Authorize<'_>>,
) -> Result<End, Box<dyn Error>> {
    stream.set_read_timeout_link(Some(SILENCE))?;
    let shared = Arc::new(Shared {
        writer: Mutex::new(stream.try_clone_link()?),
        epoch: Instant::now(),
        stop: AtomicBool::new(false),
        next_id: AtomicU32::new(1),
        clock: Mutex::new(ClockSync::default()),
        received: AtomicU32::new(0),
        decoded: AtomicU32::new(0),
        errors_total: AtomicU32::new(0),
        bytes: AtomicU32::new(0),
        latency: Mutex::new(Latency::default()),
        phone_stats: Mutex::new(None),
        stats_enabled: AtomicBool::new(false),
        quiet,
        pending: Mutex::new(HashMap::new()),
        stream_seq: AtomicU64::new(0),
        stream: Mutex::new(None),
        seconds: Mutex::new(None),
        finished: AtomicBool::new(false),
    });
    let mut reader = MessageReader::new(io::BufReader::with_capacity(
        256 * 1024,
        stream.try_clone_link()?,
    ));

    // --- handshake (§4, §7.1)
    let ours = Preamble::new(Role::Host);
    shared.writer.lock().unwrap().write_all(&ours.encode())?;
    let peer = reader.read_preamble().map_err(|e| match e.kind() {
        "io" => "the phone did not answer within 5 s".into(),
        _ => format!("{e}"),
    })?;
    ours.check_peer(&peer)?;
    // The phone sends its HELLO without waiting for ours: check it first, so that a phone that is
    // not paired learns nothing about this computer.
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
    if let Some(authorize) = authorize {
        if let Err(why) = authorize(&phone) {
            let _ = shared.send(&Message::Bye(Bye {
                reason: bye_reasons::UNPAIRED.into(),
                message: Some(why.clone()),
            }));
            let _ = stream.shutdown_link();
            return Err(why.into());
        }
    }
    shared.send(&Message::Hello(hello.clone()))?;
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

    let control = driver.is_some().then(|| {
        let (tx, rx) = mpsc::channel();
        *shared.seconds.lock().unwrap() = Some(tx);
        Control {
            shared: Arc::clone(&shared),
            seconds: rx,
            phone: phone.clone(),
        }
    });

    let ticker = {
        let shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("oculibre-ticker".into())
            .spawn(move || ticker(&shared))?
    };

    let result = thread::scope(|scope| {
        if let (Some(driver), Some(control)) = (driver, control) {
            thread::Builder::new()
                .name("oculibre-driver".into())
                .spawn_scoped(scope, move || driver(control))?;
        }
        let result = receive(&mut reader, &shared, view, sink, clock_enabled);
        // Unblock the driver before the scope waits for it: no more responses or seconds.
        shared.stop.store(true, Ordering::Relaxed);
        shared.pending.lock().unwrap().clear();
        result
    });

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
    let _ = stream.shutdown_link();
    if let Ok(totals) = ticker.join() {
        if !quiet && totals.seconds > 0 {
            println!("\n{}", totals.summary());
        }
    }
    result
}

/// A decoded picture on its way to the sink, with what the latency measurement needs.
struct Job {
    picture: Picture,
    transform: Transform,
    capture_us: u64,
    arrival_us: u64,
    decode_us: u64,
}

/// Reads and decodes on this thread; rotates, converts and writes on an output thread, so that
/// the next frame is decoded while this one is written: the frame rate is limited by the slower of
/// the two, not by their sum (4K: decoding alone takes most of a 30 fps frame interval). At most
/// one picture waits between them: latency does not pile up.
fn receive(
    reader: &mut MessageReader<io::BufReader<Box<dyn Link>>>,
    shared: &Shared,
    view: View,
    sink: &mut (dyn Sink + Send),
    clock_enabled: bool,
) -> Result<End, Box<dyn Error>> {
    let (jobs, jobs_rx) = mpsc::sync_channel::<Job>(0);
    let (free_tx, free) = mpsc::channel::<Picture>();
    thread::scope(|scope| {
        let output = thread::Builder::new()
            .name("oculibre-output".into())
            .spawn_scoped(scope, move || -> Result<(), String> {
                for job in jobs_rx {
                    sink.show(&job.picture, job.transform)
                        .map_err(|e| e.to_string())?;
                    shared.decoded.fetch_add(1, Ordering::Relaxed);
                    if clock_enabled {
                        record_latency(shared, job.capture_us, job.arrival_us, job.decode_us);
                    }
                    let _ = free_tx.send(job.picture); // reused for a later frame
                }
                Ok(())
            })?;
        let result = receive_loop(reader, shared, view, &jobs, &free);
        drop(jobs);
        match output.join() {
            Ok(Ok(())) => result,
            // The sink's error explains why the loop stopped.
            Ok(Err(e)) => Err(e.into()),
            Err(_) => Err("the output thread panicked".into()),
        }
    })
}

fn receive_loop(
    reader: &mut MessageReader<io::BufReader<Box<dyn Link>>>,
    shared: &Shared,
    view: View,
    jobs: &mpsc::SyncSender<Job>,
    free: &mpsc::Receiver<Picture>,
) -> Result<End, Box<dyn Error>> {
    let mut decoder = VideoDecoder::new()?;
    let mut announced_decoder: Option<&'static str> = None;
    // Picture being filled by the decoder (recycled from the output thread).
    let mut picture = Picture::default();
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
            Err(_) if shared.finished.load(Ordering::Relaxed) => return Ok(End::Finished),
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
                // A new stream may have another size or camera: start from a clean decoder.
                decoder = VideoDecoder::new()?;
                if !codec_data.is_empty() {
                    // SPS + PPS: no picture expected; errors show up on the next frame anyway.
                    let _ = decoder.decode(&codec_data, &mut picture);
                }
                // The phone may now time its frames with another clock (it does not answer PINGs
                // until it knows): forget the previous offsets.
                *shared.clock.lock().unwrap() = ClockSync::default();
                if let Some(c) = &config.camera {
                    orientation.sensor = c.sensor_orientation % 360;
                    orientation.front = c.facing == "front";
                }
                transform = orientation.transform(view);
                config_id = Some(config.config_id);
                need_key = true;
                *shared.stream.lock().unwrap() = Some(config);
                shared.stream_seq.fetch_add(1, Ordering::Relaxed);
            }
            Message::StreamEnd { stream, end } if stream == streams::VIDEO => {
                // The phone stops this stream (e.g. to change mode); a new STREAM_CONFIG follows.
                if !shared.quiet {
                    println!("\nStream paused ({})", end.reason);
                }
                config_id = None;
                need_key = true;
            }
            Message::VideoFrame(frame) if frame.stream == streams::VIDEO => {
                let arrival_us = shared.now_us();
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
                let decode_start = shared.now_us();
                let decoded = decoder.decode(&frame.data, &mut picture);
                let decode_us = shared.now_us() - decode_start;
                match decoded {
                    Ok(true) => {
                        need_key = false;
                        if announced != Some(transform) && !shared.quiet {
                            announced = Some(transform);
                            println!("\n{}", describe(transform, view));
                        }
                        // Known for sure once a picture is out (VA-API may refuse a stream).
                        if announced_decoder != Some(decoder.name()) && !shared.quiet {
                            announced_decoder = Some(decoder.name());
                            println!("\nDecoder: {}", decoder.name());
                        }
                        let next = free.try_recv().unwrap_or_default();
                        let job = Job {
                            picture: std::mem::replace(&mut picture, next),
                            transform,
                            capture_us: frame.timestamp,
                            arrival_us,
                            decode_us,
                        };
                        if jobs.send(job).is_err() {
                            return Err("the virtual camera stopped".into());
                        }
                    }
                    Ok(false) => {}
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
                let waiting = shared.pending.lock().unwrap().remove(&r.id);
                if let Some(tx) = waiting {
                    let _ = tx.send(r);
                } else if !r.ok && !shared.quiet {
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
            // A second HELLO, other streams, unknown types: nothing to do yet.
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
fn record_latency(shared: &Shared, capture_us: u64, arrival_us: u64, decode_us: u64) {
    let Some(best) = shared.clock.lock().unwrap().best() else {
        return; // no PONG yet: the clocks are not synchronised
    };
    let shown_us = shared.now_us() as i64;
    // offset = phone clock - host clock (the phone answered our PING).
    let capture_host = capture_us as i64 - best.offset_us;
    let total = shown_us - capture_host;
    if (0..10_000_000).contains(&total) {
        let mut l = shared.latency.lock().unwrap();
        l.total_sum += total;
        l.total_max = l.total_max.max(total);
        l.pc_sum += shown_us - arrival_us as i64;
        l.decode_sum += decode_us as i64;
        l.count += 1;
    } else {
        shared.latency.lock().unwrap().rejected += 1;
    }
}

/// One line of the console status.
#[derive(Debug, Default, PartialEq)]
struct Status {
    shown_fps: f64,
    camera_fps: Option<u64>,
    mbits: f64,
    /// Averages over the last second, ms.
    latency: Option<f64>,
    latency_max: Option<f64>,
    /// Rolling average over the last [`AVERAGE_WINDOW`] seconds, ms.
    latency_avg: Option<f64>,
    phone: Option<f64>,
    /// The phone share split (newer apps).
    split: Option<PhoneSplit>,
    pc: Option<f64>,
    rtt: Option<f64>,
    dropped: Option<u64>,
    errors: u32,
    /// Latency samples were thrown away as absurd.
    clock_mismatch: bool,
}

impl Status {
    /// `60 fps (camera 60) · 11.8 Mbit/s · latency 48 ms (phone 30 + network 6 + PC 12, max 61) · …`;
    /// with a newer app the phone share is split: `(camera 10 + encoder 18 + queue 2 + network 6 …`
    fn line(&self) -> String {
        let mut line = format!("{:>3.0} fps", self.shown_fps);
        if let Some(c) = self.camera_fps {
            line += &format!(" (camera {c})");
        }
        line += &format!(" · {:>4.1} Mbit/s", self.mbits);
        match self.latency {
            Some(total) => {
                line += &format!(" · latency {total:.0} ms");
                if let (Some(phone), Some(pc)) = (self.phone, self.pc) {
                    // The network share is what remains: transfer, waiting in buffers, clock error.
                    let network = (total - phone - pc).max(0.0);
                    let phone_text = match self.split {
                        Some(split) => split.text(),
                        None => format!("phone {phone:.0}"),
                    };
                    line += &format!(" ({phone_text} + network {network:.0} + PC {pc:.0}");
                    if let Some(max) = self.latency_max {
                        line += &format!(", max {max:.0}");
                    }
                    line += ")";
                } else if let Some(max) = self.latency_max {
                    line += &format!(" (max {max:.0})");
                }
            }
            None if self.clock_mismatch => {
                line += " · latency: unavailable (the phone's frame times don't match its clock)"
            }
            None => line += " · latency: syncing clocks…",
        }
        if let (Some(_), Some(avg)) = (self.latency, self.latency_avg) {
            line += &format!(" · avg {AVERAGE_WINDOW} s: {avg:.0} ms");
        }
        if let Some(rtt) = self.rtt {
            line += &format!(" · rtt {rtt:.1} ms");
        }
        if let Some(d) = self.dropped.filter(|&d| d > 0) {
            line += &format!(" · dropped by phone {d}");
        }
        if self.errors > 0 {
            line += &format!(" · decode errors {}", self.errors);
        }
        line
    }
}

/// Seconds of latency kept for the rolling average of the status line.
const AVERAGE_WINDOW: usize = 10;

/// Whole-session figures, printed when the phone stops.
#[derive(Debug, Default)]
struct Totals {
    seconds: u32,
    frames: u64,
    latency_sum: i64,
    latency_count: u64,
    latency_max: i64,
    /// PC share and its decoding part, µs, summed over the measured frames.
    pc_sum: i64,
    decode_sum: i64,
    /// Sum of the phone's per-second splits, and how many seconds had one.
    phone_split_sum: PhoneSplit,
    phone_split_seconds: u32,
    dropped: Option<u64>,
    errors: u32,
    /// (sum, count) of the last [`AVERAGE_WINDOW`] seconds.
    window: VecDeque<(i64, u32)>,
}

impl Totals {
    fn add_second(&mut self, frames: u32, latency: &Latency) {
        self.seconds += 1;
        self.frames += u64::from(frames);
        self.latency_sum += latency.total_sum;
        self.latency_count += u64::from(latency.count);
        self.pc_sum += latency.pc_sum;
        self.decode_sum += latency.decode_sum;
        if latency.count > 0 {
            self.latency_max = self.latency_max.max(latency.total_max);
        }
        if self.window.len() == AVERAGE_WINDOW {
            self.window.pop_front();
        }
        self.window.push_back((latency.total_sum, latency.count));
    }

    fn add_phone(&mut self, split: Option<PhoneSplit>) {
        if let Some(p) = split {
            self.phone_split_sum.camera += p.camera;
            self.phone_split_sum.encoder += p.encoder;
            self.phone_split_sum.queue += p.queue;
            self.phone_split_seconds += 1;
        }
    }

    /// Average latency of the last seconds, weighted by frames, ms.
    fn rolling_ms(&self) -> Option<f64> {
        let (sum, n) = self.window.iter().fold((0i64, 0u64), |(s, n), &(ws, wn)| {
            (s + ws, n + u64::from(wn))
        });
        (n > 0).then(|| sum as f64 / n as f64 / 1000.0)
    }

    /// `Session: 2 min 13 s · 59.4 fps · latency 83 ms on average (max 140 ms) · …`
    fn summary(&self) -> String {
        let (m, s) = (self.seconds / 60, self.seconds % 60);
        let duration = if m > 0 {
            format!("{m} min {s} s")
        } else {
            format!("{s} s")
        };
        let mut line = format!(
            "Session: {duration} · {:.1} fps",
            self.frames as f64 / f64::from(self.seconds.max(1))
        );
        if self.latency_count > 0 {
            line += &format!(
                " · latency {:.0} ms on average (max {:.0} ms)",
                self.latency_sum as f64 / self.latency_count as f64 / 1000.0,
                self.latency_max as f64 / 1000.0
            );
        }
        if let Some(d) = self.dropped {
            line += &format!(" · dropped by phone {d}");
        }
        line += &format!(" · decode errors {}", self.errors);
        if self.phone_split_seconds > 0 {
            let n = f64::from(self.phone_split_seconds);
            let average = PhoneSplit {
                camera: self.phone_split_sum.camera / n,
                encoder: self.phone_split_sum.encoder / n,
                queue: self.phone_split_sum.queue / n,
            };
            line += &format!("\nPhone per frame: {} ms", average.text());
        }
        if self.latency_count > 0 && self.pc_sum > 0 {
            // Where the PC share goes: if it nears the frame interval, the PC falls behind.
            let per_frame = |us: i64| us as f64 / self.latency_count as f64 / 1000.0;
            line += &format!(
                "\nPC per frame: decode {:.1} ms + rotation, conversion and write {:.1} ms",
                per_frame(self.decode_sum),
                per_frame(self.pc_sum - self.decode_sum)
            );
        }
        line
    }
}

/// Every second: PING, host stats, status line. Stops when the session ends or a write fails.
fn ticker(shared: &Shared) -> Totals {
    // Owned here: the driver's `next_second` returns `None` once the ticker is gone.
    let seconds = shared.seconds.lock().unwrap().take();
    let mut totals = Totals::default();
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
        let latency = std::mem::take(&mut *shared.latency.lock().unwrap());
        let avg_ms =
            |sum: i64| (latency.count > 0).then(|| sum as f64 / latency.count as f64 / 1000.0);
        let latency_ms = avg_ms(latency.total_sum);
        totals.add_second(decoded, &latency);
        totals.errors = errors;
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
            if let Some(l) = latency_ms {
                data["latency_ms"] = json!((l * 10.0).round() / 10.0);
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
                let _ = w.shutdown_link();
            }
            return totals;
        }

        let phone = shared
            .phone_stats
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_default();
        totals.dropped = phone.get("dropped").and_then(Value::as_u64);
        if latency.count > 0 {
            totals.add_phone(PhoneSplit::from_stats(&phone));
        }
        let mbits = bytes as f64 * 8.0 / secs / 1e6;
        if let Some(tx) = &seconds {
            let _ = tx.send(Second {
                stream_seq: shared.stream_seq.load(Ordering::Relaxed),
                shown_fps: decoded as f64 / secs,
                camera_fps: phone.get("camera_fps").and_then(Value::as_f64),
                mbits,
                latency_sum_us: latency.total_sum,
                latency_max_us: latency.total_max,
                latency_count: latency.count,
                pc_sum_us: latency.pc_sum,
                phone_latency_ms: phone.get("latency_ms").and_then(Value::as_f64),
                phone_split: PhoneSplit::from_stats(&phone),
                rtt_ms,
                dropped_total: totals.dropped,
                errors_total: errors,
            });
        }

        if !shared.quiet {
            let status = Status {
                shown_fps: decoded as f64 / secs,
                camera_fps: phone.get("camera_fps").and_then(Value::as_u64),
                mbits,
                latency: latency_ms,
                latency_max: latency_ms.map(|_| latency.total_max as f64 / 1000.0),
                latency_avg: totals.rolling_ms(),
                phone: phone.get("latency_ms").and_then(Value::as_f64),
                split: PhoneSplit::from_stats(&phone),
                pc: avg_ms(latency.pc_sum),
                rtt: rtt_ms,
                dropped: totals.dropped,
                errors,
                clock_mismatch: latency.rejected > 0,
            };
            // Clear the rest of the previous line (it may have been longer).
            let line = format!("\r{}\x1b[K", status.line());
            print!("{line}");
            let _ = io::stdout().flush();
        }
    }
    totals
}

#[cfg(test)]
mod status_tests {
    use super::{PhoneSplit, Status};

    #[test]
    fn full_line() {
        let s = Status {
            shown_fps: 59.6,
            camera_fps: Some(60),
            mbits: 11.84,
            latency: Some(48.2),
            latency_max: Some(61.0),
            latency_avg: Some(50.6),
            phone: Some(30.4),
            split: None,
            pc: Some(11.9),
            rtt: Some(1.23),
            dropped: Some(0),
            errors: 0,
            clock_mismatch: false,
        };
        assert_eq!(
            s.line(),
            " 60 fps (camera 60) · 11.8 Mbit/s · latency 48 ms \
             (phone 30 + network 6 + PC 12, max 61) · avg 10 s: 51 ms · rtt 1.2 ms"
        );
    }

    #[test]
    fn line_with_the_phone_split() {
        let s = Status {
            shown_fps: 60.0,
            mbits: 12.0,
            latency: Some(90.0),
            phone: Some(80.0),
            split: Some(PhoneSplit {
                camera: 31.2,
                encoder: 46.6,
                queue: 2.4,
            }),
            pc: Some(7.0),
            ..Default::default()
        };
        assert_eq!(
            s.line(),
            " 60 fps · 12.0 Mbit/s · latency 90 ms \
             (camera 31 + encoder 47 + queue 2 + network 3 + PC 7)"
        );
    }

    #[test]
    fn before_clock_sync() {
        let s = Status {
            shown_fps: 30.0,
            errors: 2,
            ..Default::default()
        };
        assert_eq!(
            s.line(),
            " 30 fps ·  0.0 Mbit/s · latency: syncing clocks… · decode errors 2"
        );
    }
}

#[cfg(test)]
mod totals_tests {
    use super::{Latency, Totals, AVERAGE_WINDOW};

    fn second(ms: i64, frames: u32) -> Latency {
        Latency {
            total_sum: ms * 1000 * i64::from(frames),
            total_max: ms * 1000 + 5000,
            count: frames,
            ..Default::default()
        }
    }

    #[test]
    fn rolling_average_keeps_the_last_seconds() {
        let mut t = Totals::default();
        t.add_second(60, &second(200, 60)); // pushed out of the window below
        for _ in 0..AVERAGE_WINDOW {
            t.add_second(60, &second(80, 60));
        }
        assert_eq!(t.rolling_ms(), Some(80.0));
        // A second without measurement does not count.
        t.add_second(60, &Latency::default());
        assert_eq!(t.rolling_ms(), Some(80.0));
    }

    #[test]
    fn session_summary() {
        let mut t = Totals::default();
        for i in 0..133 {
            t.add_second(60, &second(if i == 0 { 140 } else { 80 }, 60));
        }
        t.dropped = Some(3);
        assert_eq!(
            t.summary(),
            "Session: 2 min 13 s · 60.0 fps · latency 80 ms on average (max 145 ms) \
             · dropped by phone 3 · decode errors 0"
        );
    }
}
