//! `oculibre` — desktop receiver (Linux).
//!
//! Waits for the phone, speaks the Oculibre protocol 0.1 with it (`docs/protocol.typ`), decodes
//! the H.264 video with OpenH264 and writes the frames to a v4l2loopback device, so the phone
//! shows up as a regular webcam.

mod session;
mod v4l2;
mod yuv;

use std::error::Error;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Command;

use openh264::decoder::DecodedYUV;
use openh264::formats::YUVSource;

const USAGE: &str = "\
Oculibre — use your Android phone as a webcam (Linux receiver)

USAGE:
    oculibre [OPTIONS]

OPTIONS:
    -p, --port <PORT>       TCP port to listen on [default: 47100]
    -d, --device <PATH>     v4l2loopback device [default: auto-detect]
        --no-adb            Don't run `adb reverse`
        --insecure-lan      Also accept phones over the network (Wi-Fi), UNENCRYPTED:
                            anyone on the same network could watch or inject video
    -h, --help              Show this help

Several phones plugged in? Choose one with ANDROID_SERIAL=<serial> oculibre
";

struct Args {
    port: u16,
    device: Option<PathBuf>,
    adb: bool,
    insecure_lan: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        port: oculibre_protocol::HOST_PORT,
        device: None,
        adb: true,
        insecure_lan: false,
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
            "--insecure-lan" => args.insecure_lan = true,
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

    // §13: plaintext is only acceptable on loopback (USB through adb reverse).
    let bind_ip = if args.insecure_lan {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    } else {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    };
    let listener = TcpListener::bind((bind_ip, args.port))
        .map_err(|e| format!("cannot listen on port {}: {e}", args.port))?;
    if args.insecure_lan {
        eprintln!(
            "WARNING: --insecure-lan: network connections are accepted WITHOUT encryption or \
             pairing. Anyone on this network can watch the video or send fake video to this \
             virtual camera. Use it only on a network you trust."
        );
    }
    println!(
        "Listening on {}:{} — press Start in the app (Ctrl+C to quit)",
        bind_ip, args.port
    );

    let hello = session::host_hello(host_id(), host_name(), os_version());

    for conn in listener.incoming() {
        let stream = match conn {
            Ok(s) => s,
            Err(e) => {
                eprintln!("accept failed: {e}");
                continue;
            }
        };
        let peer = match stream.peer_addr() {
            Ok(a) => a,
            Err(_) => continue,
        };
        if !peer.ip().is_loopback() && !args.insecure_lan {
            eprintln!("Refused a connection from {peer}: only USB (adb) is allowed without --insecure-lan");
            continue;
        }
        println!("Phone connected ({peer})");
        let mut sink = VirtualCamera::new(&device_path);
        match session::run(stream, &hello, &mut sink, false) {
            Ok(session::End::Bye { reason, message }) => println!(
                "\nPhone stopped ({reason}{}) — waiting for it…",
                message.map(|m| format!(": {m}")).unwrap_or_default()
            ),
            Ok(session::End::Closed) => println!("\nPhone disconnected — waiting for it…"),
            Err(e) => eprintln!("\nConnection ended: {e} — waiting for the phone…"),
        }
    }
    Ok(())
}

/// The v4l2loopback device, opened at the first picture (when the size is known) and closed
/// with the session, so the next session can use another resolution.
struct VirtualCamera<'a> {
    path: &'a Path,
    device: Option<v4l2::OutputDevice>,
    size: (usize, usize),
    frame: Vec<u8>,
}

impl<'a> VirtualCamera<'a> {
    fn new(path: &'a Path) -> Self {
        Self {
            path,
            device: None,
            size: (0, 0),
            frame: Vec::new(),
        }
    }
}

impl session::Sink for VirtualCamera<'_> {
    fn show(&mut self, picture: &DecodedYUV<'_>) -> Result<(), Box<dyn Error>> {
        let (w, h) = picture.dimensions();
        if (w, h) != self.size {
            self.size = (w, h);
            self.device = None; // close before reopening at the new size
        }
        let device = match self.device.as_mut() {
            Some(d) => d,
            None => {
                let d = v4l2::OutputDevice::open(self.path, w, h)?;
                let (cw, ch) = d.size();
                if (cw, ch) == (w, h) {
                    println!("Virtual camera set to {cw}x{ch}");
                } else {
                    println!(
                        "Virtual camera is locked at {cw}x{ch} by an app reading it: \
                         scaling {w}x{h} to it.\n\
                         Close the apps using the camera, then Stop/Start, to get {w}x{h}."
                    );
                }
                self.device.insert(d)
            }
        };
        let (cw, ch) = device.size();
        yuv::i420_to_yuyv(picture, cw, ch, &mut self.frame);
        device.write_frame(&self.frame)?;
        Ok(())
    }
}

/// Random id of this computer, created once (§7.1: never derived from hardware identifiers).
fn host_id() -> String {
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")))
        .map(|d| d.join("oculibre"));
    let file = dir.as_ref().map(|d| d.join("host-id"));
    if let Some(id) = file.as_ref().and_then(|f| fs::read_to_string(f).ok()) {
        let id = id.trim().to_string();
        if !id.is_empty() {
            return id;
        }
    }
    let mut b = [0u8; 16];
    if let Ok(mut f) = fs::File::open("/dev/urandom") {
        let _ = std::io::Read::read_exact(&mut f, &mut b);
    }
    b[6] = (b[6] & 0x0f) | 0x40; // UUID version 4
    b[8] = (b[8] & 0x3f) | 0x80; // RFC 4122 variant
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    let id = format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    );
    if let (Some(dir), Some(file)) = (dir, file) {
        let _ = fs::create_dir_all(dir).and_then(|_| fs::write(file, &id));
    }
    id
}

fn host_name() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname")
        .or_else(|_| fs::read_to_string("/etc/hostname"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "computer".into())
}

fn os_version() -> Option<String> {
    fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|s| s.trim().to_string())
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

#[cfg(test)]
mod tests;
