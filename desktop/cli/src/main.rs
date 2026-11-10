//! `oculibre` — phase-1 desktop receiver.
//!
//! Listens for the phone (raw H.264 Annex B over TCP), decodes it with OpenH264 and
//! writes the frames to a v4l2loopback device, so the phone shows up as a regular webcam.

mod annexb;
mod v4l2;
mod yuv;

use std::error::Error;
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use openh264::decoder::Decoder;
use openh264::formats::YUVSource;

const DEFAULT_PORT: u16 = 47100;

const USAGE: &str = "\
Oculibre — use your Android phone as a webcam (phase-1 receiver, Linux)

USAGE:
    oculibre [OPTIONS]

OPTIONS:
    -p, --port <PORT>       TCP port to listen on [default: 47100]
    -d, --device <PATH>     v4l2loopback device [default: auto-detect]
        --no-adb            Don't run `adb reverse` (Wi-Fi only)
    -h, --help              Show this help

Several phones plugged in? Choose one with ANDROID_SERIAL=<serial> oculibre
";

struct Args {
    port: u16,
    device: Option<PathBuf>,
    adb: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        port: DEFAULT_PORT,
        device: None,
        adb: true,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-p" | "--port" => {
                let v = it.next().ok_or("--port needs a value")?;
                args.port = v.parse().map_err(|_| format!("invalid port: {v}"))?;
            }
            "-d" | "--device" => {
                args.device = Some(it.next().ok_or("--device needs a value")?.into());
            }
            "--no-adb" => args.adb = false,
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown option: {other}\n\n{USAGE}")),
        }
    }
    Ok(args)
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;

    let device_path = match args.device {
        Some(d) => d,
        None => v4l2::find_loopback()?,
    };
    v4l2::check_access(&device_path)?;
    println!("Virtual camera: {}", device_path.display());

    if args.adb {
        adb_reverse(args.port);
    }

    let listener = TcpListener::bind(("0.0.0.0", args.port))
        .map_err(|e| format!("cannot listen on port {}: {e}", args.port))?;
    println!(
        "Listening on port {} — press Start in the app (Ctrl+C to quit)",
        args.port
    );

    for conn in listener.incoming() {
        let stream = match conn {
            Ok(s) => s,
            Err(e) => {
                eprintln!("accept failed: {e}");
                continue;
            }
        };
        let peer = stream
            .peer_addr()
            .map(|a| a.to_string())
            .unwrap_or_default();
        println!("Phone connected ({peer})");
        match receive(stream, &device_path) {
            Ok(()) => println!("\nPhone disconnected — waiting for it to reconnect…"),
            Err(e) => eprintln!("\nConnection ended: {e} — waiting for the phone…"),
        }
    }
    Ok(())
}

/// Runs `adb reverse` so the phone's 127.0.0.1:<port> reaches this PC over USB.
fn adb_reverse(port: u16) {
    let adb = find_adb();
    let spec = format!("tcp:{port}");
    match Command::new(&adb).args(["reverse", &spec, &spec]).output() {
        Ok(out) if out.status.success() => {
            println!("USB: adb reverse {spec} OK — use 127.0.0.1 in the app");
        }
        Ok(out) => eprintln!(
            "warning: adb reverse failed ({}) — USB won't work, Wi-Fi still will",
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(e) => eprintln!(
            "warning: cannot run {} ({e}) — USB won't work, Wi-Fi still will",
            adb.display()
        ),
    }
}

fn find_adb() -> PathBuf {
    let sdk_dirs = ["ANDROID_HOME", "ANDROID_SDK_ROOT"]
        .iter()
        .filter_map(|v| std::env::var_os(v).map(PathBuf::from))
        .chain(std::env::var_os("HOME").map(|h| Path::new(&h).join("Android/Sdk")));
    for sdk in sdk_dirs {
        let adb = sdk.join("platform-tools/adb");
        if adb.is_file() {
            return adb;
        }
    }
    PathBuf::from("adb") // rely on PATH
}

/// One phone session. The virtual camera is opened on the first decoded frame (when the
/// size is known) and closed when the phone disconnects, so the next session can use
/// another resolution.
fn receive(mut stream: TcpStream, device_path: &Path) -> Result<(), Box<dyn Error>> {
    stream.set_nodelay(true)?;
    let mut camera: Option<v4l2::OutputDevice> = None;
    let mut decoder = Decoder::new()?;
    let mut splitter = annexb::Splitter::new();
    let mut chunk = vec![0u8; 256 * 1024];
    let mut frame = Vec::new();

    let mut frames = 0u32;
    let mut errors = 0u32;
    let mut bytes = 0usize;
    let mut size = (0, 0);
    let mut last_report = Instant::now();

    loop {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Ok(());
        }
        bytes += n;
        splitter.push(&chunk[..n]);

        while let Some(nal) = splitter.next_nal() {
            match decoder.decode(&nal) {
                Ok(Some(picture)) => {
                    let (w, h) = picture.dimensions();
                    if (w, h) != size {
                        println!("\nStream: {w}x{h}");
                        size = (w, h);
                        camera = None; // close before reopening at the new size
                    }
                    let cam = match camera.as_mut() {
                        Some(cam) => cam,
                        None => {
                            let cam = v4l2::OutputDevice::open(device_path, w, h)?;
                            let (cw, ch) = cam.size();
                            if (cw, ch) == (w, h) {
                                println!("Virtual camera set to {cw}x{ch}");
                            } else {
                                println!(
                                    "Virtual camera is locked at {cw}x{ch} by an app reading it: \
                                     scaling {w}x{h} down/up to it.\n\
                                     Close the apps using the camera, then Stop/Start, to get {w}x{h}."
                                );
                            }
                            camera.insert(cam)
                        }
                    };
                    let (cw, ch) = cam.size();
                    yuv::i420_to_yuyv(&picture, cw, ch, &mut frame);
                    cam.write_frame(&frame)?;
                    frames += 1;
                }
                Ok(None) => {}
                Err(_) => errors += 1,
            }
        }

        let elapsed = last_report.elapsed();
        if elapsed >= Duration::from_secs(1) {
            let secs = elapsed.as_secs_f64();
            print!(
                "\r{:>3.0} fps · {:>4.1} Mbit/s · decode errors: {errors}   ",
                f64::from(frames) / secs,
                bytes as f64 * 8.0 / secs / 1e6,
            );
            let _ = std::io::Write::flush(&mut std::io::stdout());
            frames = 0;
            bytes = 0;
            last_report = Instant::now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openh264::encoder::Encoder;
    use openh264::formats::YUVBuffer;

    /// Encodes a few frames with OpenH264, feeds the bytes in awkward chunk sizes through
    /// the splitter and decoder, and checks every frame comes out — no phone needed.
    #[test]
    fn encode_split_decode_roundtrip() {
        let (w, h) = (320usize, 240usize);
        let mut encoder = Encoder::new().unwrap();
        let mut stream = Vec::new();
        for i in 0..10u8 {
            let mut yuv = vec![i.wrapping_mul(20); w * h * 3 / 2];
            yuv[i as usize * 100] = 255;
            let buffer = YUVBuffer::from_vec(yuv, w, h);
            stream.extend(encoder.encode(&buffer).unwrap().to_vec());
        }
        // A trailing start code flushes the last NAL out of the splitter.
        stream.extend([0, 0, 0, 1]);

        let mut decoder = Decoder::new().unwrap();
        let mut splitter = annexb::Splitter::new();
        let mut out = Vec::new();
        let mut decoded = 0;
        for chunk in stream.chunks(777) {
            splitter.push(chunk);
            while let Some(nal) = splitter.next_nal() {
                if let Some(pic) = decoder.decode(&nal).unwrap() {
                    assert_eq!(pic.dimensions(), (w, h));
                    yuv::i420_to_yuyv(&pic, w, h, &mut out);
                    assert_eq!(out.len(), w * h * 2);
                    decoded += 1;
                }
            }
        }
        assert_eq!(decoded, 10);
    }
}
