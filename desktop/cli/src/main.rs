//! `oculibre` — desktop receiver (Linux).
//!
//! Waits for the phone, speaks the Oculibre protocol 0.3 with it (`docs/protocol.typ`), decodes
//! the H.264 video with OpenH264 and writes the frames to a v4l2loopback device, so the phone
//! shows up as a regular webcam. USB connections (loopback) are plaintext; Wi-Fi connections are
//! TLS with pinned keys, for paired phones only. `oculibre bench` measures every camera × mode.

mod bench;
mod discovery;
mod pairing;
mod session;
mod tls;
mod v4l2;
mod yuv;

use std::error::Error;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use openh264::formats::YUVSource;

const USAGE: &str = "\
Oculibre — use your Android phone as a webcam (Linux receiver)

USAGE:
    oculibre [OPTIONS]
    oculibre bench [OPTIONS] [BENCH OPTIONS]
    oculibre pairings           List the phones paired for Wi-Fi
    oculibre unpair <NAME>      Forget a paired phone (name or fingerprint start, or --all)

OPTIONS:
    -p, --port <PORT>       TCP port to listen on [default: 47100]
    -d, --device <PATH>     v4l2loopback device [default: auto-detect]
        --no-adb            Don't run `adb reverse`
    -w, --wifi              Also accept paired phones over the network (Wi-Fi), encrypted
        --pair              Pair a new phone: shows a QR code to scan (implies --wifi)
        --tls12-only        Testing: Wi-Fi with TLS 1.2 only, as with Android 5-9 phones
    -r, --rotate <ROTATION> auto, 0, 90, 180 or 270 (degrees, clockwise) [default: auto]
                            auto: upright whatever the way you hold the phone.
                            A number: fixed, the picture never turns.
    -m, --mirror            Flip the picture horizontally (left ↔ right)
        --portrait          Portrait virtual camera (default: landscape, like every webcam;
                            a portrait picture then gets black bars on the sides)
    -h, --help              Show this help

BENCH: walks every camera and quality of the phone, measures frame rate, latency, bitrate
and dropped frames, prints a table and writes it into devices/<brand>/<model>/report.md.
        --seconds <N>       Seconds measured per mode [default: 10]
        --warmup <N>        Seconds left out after each switch [default: 3]
        --report <PATH>     Report to update [default: the phone's report in devices/]
        --no-report         Only print the results

Several phones plugged in? Choose one with ANDROID_SERIAL=<serial> oculibre
";

struct Args {
    port: u16,
    device: Option<PathBuf>,
    adb: bool,
    wifi: bool,
    pair: bool,
    /// Testing: offer TLS 1.2 only (the path of Android 5–9 phones).
    tls12_only: bool,
    view: session::View,
    portrait: bool,
    /// `oculibre bench`.
    bench: Option<bench::Settings>,
    report: Option<PathBuf>,
    write_report: bool,
    /// `oculibre pairings`, `oculibre unpair <name>`.
    list_pairings: bool,
    unpair: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        port: oculibre_protocol::HOST_PORT,
        device: None,
        adb: true,
        wifi: false,
        pair: false,
        tls12_only: false,
        view: session::View::default(),
        portrait: false,
        bench: None,
        report: None,
        write_report: true,
        list_pairings: false,
        unpair: None,
    };
    let mut settings = bench::Settings::default();
    let mut bench_only = Vec::new();
    let mut it = std::env::args().skip(1).peekable();
    match it.peek().map(String::as_str) {
        Some("bench") => {
            it.next();
            args.bench = Some(settings);
        }
        Some("pairings") => {
            args.list_pairings = true;
            return Ok(args);
        }
        Some("unpair") => {
            it.next();
            let which = it.next().ok_or("unpair needs a phone name (or --all)")?;
            args.unpair = Some(which);
            return Ok(args);
        }
        _ => {}
    }
    let seconds = |v: Option<String>, name: &str| -> Result<u32, String> {
        let v = v.ok_or(format!("{name} needs a value"))?;
        v.parse()
            .ok()
            .filter(|&n| n <= 600)
            .ok_or(format!("invalid {name}: {v}"))
    };
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--seconds" => {
                settings.seconds = seconds(it.next(), "--seconds")?.max(1);
                bench_only.push(arg);
            }
            "--warmup" => {
                settings.warmup = seconds(it.next(), "--warmup")?.max(1);
                bench_only.push(arg);
            }
            "--report" => {
                args.report = Some(it.next().ok_or("--report needs a value")?.into());
                bench_only.push(arg);
            }
            "--no-report" => {
                args.write_report = false;
                bench_only.push(arg);
            }
            "-p" | "--port" => {
                let v = it.next().ok_or("--port needs a value")?;
                args.port = v.parse().map_err(|_| format!("invalid port: {v}"))?;
            }
            "-d" | "--device" => {
                args.device = Some(it.next().ok_or("--device needs a value")?.into());
            }
            "--no-adb" => args.adb = false,
            "-w" | "--wifi" => args.wifi = true,
            "--tls12-only" => args.tls12_only = true,
            "--pair" => {
                args.pair = true;
                args.wifi = true;
            }
            "--insecure-lan" => {
                return Err(
                    "--insecure-lan is gone: Wi-Fi is now encrypted. Pair the phone once \
                            with `oculibre --pair`, then use `oculibre --wifi`"
                        .into(),
                )
            }
            "-r" | "--rotate" => {
                let v = it.next().ok_or("--rotate needs a value")?;
                args.view.rotate = match v.as_str() {
                    "auto" => session::Rotate::Auto,
                    "0" | "90" | "180" | "270" => session::Rotate::Fixed(v.parse().unwrap()),
                    _ => return Err(format!("invalid rotation: {v} (auto, 0, 90, 180 or 270)")),
                };
            }
            "-m" | "--mirror" => args.view.mirror = true,
            "--portrait" => args.portrait = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown option: {other}\n\n{USAGE}")),
        }
    }
    match (&mut args.bench, bench_only.first()) {
        (Some(b), _) => *b = settings,
        (None, Some(option)) => return Err(format!("{option} only works with `oculibre bench`")),
        (None, None) => {}
    }
    Ok(args)
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// TLS identity and paired phones, when Wi-Fi is on.
struct Wifi {
    config: Arc<rustls::ServerConfig>,
    pairings: pairing::Pairings,
    /// mDNS announcement, as long as the program runs.
    _announcer: Option<discovery::Announcer>,
}

/// A phone that connected, ready for a session.
struct Incoming {
    link: Box<dyn session::Link>,
    peer: SocketAddr,
    /// Wi-Fi: SHA-256 of the phone's TLS certificate.
    fingerprint: Option<String>,
}

/// Waits for a phone: USB (loopback) as is, the network through TLS when `wifi` is on.
fn accept_phone(listener: &TcpListener, wifi: Option<&Wifi>) -> Incoming {
    loop {
        let (stream, peer) = match listener.accept() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("accept failed: {e}");
                continue;
            }
        };
        if peer.ip().is_loopback() {
            let _ = stream.set_nodelay(true);
            println!("Phone connected over USB ({peer})");
            return Incoming {
                link: Box::new(stream),
                peer,
                fingerprint: None,
            };
        }
        // §13: never plaintext on a network.
        let Some(wifi) = wifi else {
            eprintln!(
                "Refused a connection from {peer}: start with --wifi to accept paired phones"
            );
            continue;
        };
        match tls::accept(stream, wifi.config.clone()) {
            Ok(a) => {
                println!(
                    "Phone connected over Wi-Fi ({peer}), encrypted ({})",
                    a.version
                );
                return Incoming {
                    link: Box::new(a.link),
                    peer,
                    fingerprint: Some(a.fingerprint),
                };
            }
            Err(e) => eprintln!("Refused a connection from {peer}: {e}"),
        }
    }
}

/// The pairing check of a Wi-Fi phone (USB phones need none).
fn authorizer<'a>(
    wifi: Option<&'a Wifi>,
    fingerprint: Option<&'a str>,
) -> Option<impl Fn(&oculibre_protocol::Hello) -> Result<(), String> + Sync + 'a> {
    let (wifi, fingerprint) = (wifi?, fingerprint?);
    Some(move |hello: &oculibre_protocol::Hello| {
        wifi.pairings.authorize(fingerprint, hello).map(|new| {
            if let Some(name) = new {
                println!(
                    "Paired with {name}: it can now connect over Wi-Fi with `oculibre --wifi`"
                );
            }
        })
    })
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;

    if args.list_pairings || args.unpair.is_some() {
        let pairings = pairing::Pairings::load(config_dir().join("paired-phones.json"));
        if let Some(which) = &args.unpair {
            let gone = pairings.forget(which)?;
            if gone.is_empty() {
                return Err(
                    format!("no paired phone matches {which:?} (see `oculibre pairings`)").into(),
                );
            }
            for p in gone {
                println!(
                    "Forgot {} ({})",
                    p.name,
                    tls::short_fingerprint(&p.fingerprint)
                );
            }
        } else {
            let phones = pairings.phones();
            if phones.is_empty() {
                println!("No phone paired yet: `oculibre --pair`");
            }
            for p in phones {
                println!(
                    "{}  ·  paired on {}  ·  key {}",
                    p.name,
                    p.paired_on,
                    tls::short_fingerprint(&p.fingerprint)
                );
            }
        }
        return Ok(());
    }

    let device_path = match args.device.clone() {
        Some(d) => d,
        None => v4l2::find_loopback()?,
    };
    v4l2::check_access(&device_path)?;
    println!("Virtual camera: {}", device_path.display());

    if args.adb {
        adb_reverse(args.port);
    }

    // §13: plaintext only on loopback (USB through adb reverse); the network needs --wifi (TLS).
    let bind_ip = if args.wifi {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    } else {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    };
    let listener = TcpListener::bind((bind_ip, args.port)).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            format!(
                "port {} is already used, probably by another `oculibre` still running: \
                     stop it first (or use --port)",
                args.port
            )
        } else {
            format!("cannot listen on port {}: {e}", args.port)
        }
    })?;
    let wifi = if args.wifi {
        let identity = tls::Identity::load_or_create(&config_dir())?;
        let addresses = pairing::lan_addresses();
        let announcer =
            match discovery::announce(&host_name(), &identity.fingerprint(), &addresses, args.port)
            {
                Ok(a) => Some(a),
                Err(e) => {
                    eprintln!(
                        "warning: cannot announce this computer on the network (mDNS: {e}): \
                         phones will use the address they know"
                    );
                    None
                }
            };
        let wifi = Wifi {
            config: tls::server_config(&identity, args.tls12_only)?,
            pairings: pairing::Pairings::load(config_dir().join("paired-phones.json")),
            _announcer: announcer,
        };
        let list: Vec<String> = addresses.iter().map(IpAddr::to_string).collect();
        println!(
            "Wi-Fi: encrypted, paired phones only ({} paired) · this computer: {} · key {}{}",
            wifi.pairings.phones().len(),
            if list.is_empty() {
                "no network address".into()
            } else {
                list.join(", ")
            },
            tls::short_fingerprint(&identity.fingerprint()),
            if wifi._announcer.is_some() {
                " · announced (mDNS)"
            } else {
                ""
            }
        );
        if args.pair {
            show_pairing(&wifi, &identity, &addresses, args.port, args.adb)?;
        }
        Some(wifi)
    } else {
        None
    };
    println!(
        "Listening on {}:{} — press Start in the app (Ctrl+C to quit)",
        bind_ip, args.port
    );

    let hello = session::host_hello(host_id(), host_name(), os_version());

    if let Some(settings) = args.bench {
        return run_bench(
            &listener,
            &args,
            settings,
            &hello,
            &device_path,
            wifi.as_ref(),
        );
    }

    loop {
        let phone = accept_phone(&listener, wifi.as_ref());
        let authorize = authorizer(wifi.as_ref(), phone.fingerprint.as_deref());
        let mut sink = VirtualCamera::new(&device_path, args.portrait, true);
        let result = session::run_driven(
            phone.link,
            &hello,
            args.view,
            &mut sink,
            false,
            None,
            authorize.as_ref().map(|a| a as session::Authorize<'_>),
        );
        match result {
            Ok(session::End::Bye { reason, message }) => println!(
                "\nPhone stopped ({reason}{}) — waiting for it…",
                message.map(|m| format!(": {m}")).unwrap_or_default()
            ),
            Ok(session::End::Closed | session::End::Finished) => {
                println!("\nPhone disconnected — waiting for it…")
            }
            Err(e) => eprintln!("\nConnection ended: {e} — waiting for the phone…"),
        }
    }
}

/// `--pair`: a one-time offer shown as a QR code (and sent over USB when a phone is plugged in).
fn show_pairing(
    wifi: &Wifi,
    identity: &tls::Identity,
    addresses: &[IpAddr],
    port: u16,
    adb: bool,
) -> Result<(), String> {
    if addresses.is_empty() {
        return Err("no network address found: connect this computer to the Wi-Fi first".into());
    }
    let token = wifi.pairings.open_offer(pairing::OFFER_TTL)?;
    let uri = pairing::uri(
        &host_name(),
        &identity.fingerprint(),
        &token,
        addresses,
        port,
    );
    println!("\n{}", pairing::qr(&uri)?);
    println!(
        "Pair a phone: scan this QR code with the phone's camera app (it opens Oculibre and asks \
         to confirm), then press Start. Valid {} minutes, for one phone.\nLink: {uri}",
        pairing::OFFER_TTL.as_secs() / 60
    );
    if adb && adb_open_uri(&uri) {
        println!("Also sent to the phone plugged in over USB: confirm on its screen.");
    }
    println!();
    Ok(())
}

/// `oculibre bench`: waits for one phone, drives it through every mode, writes the report.
fn run_bench(
    listener: &TcpListener,
    args: &Args,
    settings: bench::Settings,
    hello: &oculibre_protocol::Hello,
    device_path: &Path,
    wifi: Option<&Wifi>,
) -> Result<(), Box<dyn Error>> {
    println!("Bench: start the Oculibre app on the phone and press Start (any quality).");
    let phone = accept_phone(listener, wifi);
    let peer = phone.peer;
    let authorize = authorizer(wifi, phone.fingerprint.as_deref());
    let link = if peer.ip().is_loopback() {
        bench::Link::Usb
    } else {
        bench::Link::Wifi
    };
    let mut sink = VirtualCamera::new(device_path, args.portrait, false);
    let outcome = Mutex::new(None);
    let driver: session::Driver<'_> = Box::new(|control: session::Control| {
        let d = control.phone.device.clone().unwrap_or_default();
        println!(
            "Phone: {} {} (Android {}, {} {}) over {}",
            d.manufacturer.unwrap_or_default(),
            d.model.unwrap_or_default(),
            d.os_version.unwrap_or_default(),
            control.phone.app,
            control.phone.app_version,
            match link {
                bench::Link::Usb => "USB",
                bench::Link::Wifi => "Wi-Fi",
            }
        );
        let result = bench::run(&control, settings);
        control.finish("bench finished");
        *outcome.lock().unwrap() = Some(result);
    });
    let end = session::run_driven(
        phone.link,
        hello,
        args.view,
        &mut sink,
        true,
        Some(driver),
        authorize.as_ref().map(|a| a as session::Authorize<'_>),
    );

    let outcome = match outcome.into_inner().unwrap() {
        Some(Ok(o)) => o,
        Some(Err(e)) => return Err(e.into()),
        None => {
            return Err(match end {
                Err(e) => format!("the session ended before the bench could start: {e}"),
                Ok(end) => format!("the phone left before the bench could start ({end:?})"),
            }
            .into())
        }
    };
    if let Err(e) = end {
        eprintln!("Connection: {e}");
    }
    println!("\n{}", bench::table(&outcome.rows));
    if outcome.rows.iter().all(|r| r.result.is_err()) {
        return Err("no mode could be measured: nothing written".into());
    }
    if !args.write_report {
        return Ok(());
    }

    let model = outcome
        .phone
        .device
        .as_ref()
        .and_then(|d| d.model.clone())
        .unwrap_or_default();
    let devices = bench::devices_dir();
    let path = match (&args.report, &devices) {
        (Some(p), _) => p.clone(),
        (None, Some(devices)) => match bench::find_report(devices, &model) {
            Some(p) => p,
            None => {
                let p = PathBuf::from(format!("oculibre-bench-{}.md", bench::slugify(&model)));
                println!(
                    "No report in {} names the model \"{model}\": run \
                     `python3 scripts/device-report.py` (it creates the phone's folder), then the \
                     bench again. Results saved to {} for now.",
                    devices.display(),
                    p.display()
                );
                p
            }
        },
        (None, None) => PathBuf::from(format!("oculibre-bench-{}.md", bench::slugify(&model))),
    };
    let ctx = bench::Context {
        link,
        date: bench::today(),
        host_os: std::env::consts::OS,
        settings,
    };
    let old = fs::read_to_string(&path).unwrap_or_default();
    let new = bench::update_report(&old, &bench::section(&outcome, &ctx), link);
    fs::write(&path, new).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    println!("Results written to {}", path.display());
    if path.starts_with(devices.unwrap_or_default()) {
        println!("Check it, then open a pull request (or attach it to a device report issue).");
    }
    Ok(())
}

/// The v4l2loopback device, opened at the first picture and closed with the session.
///
/// Its shape never follows the phone: landscape by default, like every webcam (apps expect it,
/// and v4l2loopback cannot change the size while an app has the camera open anyway). A portrait
/// picture gets black bars on the sides; `--portrait` gives a portrait virtual camera instead.
struct VirtualCamera<'a> {
    path: &'a Path,
    portrait: bool,
    /// Say when the device is (re)opened.
    verbose: bool,
    device: Option<v4l2::OutputDevice>,
    /// Size asked for when the device was opened.
    requested: (usize, usize),
    frame: Vec<u8>,
}

impl<'a> VirtualCamera<'a> {
    fn new(path: &'a Path, portrait: bool, verbose: bool) -> Self {
        Self {
            path,
            portrait,
            verbose,
            device: None,
            requested: (0, 0),
            frame: Vec::new(),
        }
    }
}

impl session::Sink for VirtualCamera<'_> {
    fn show(
        &mut self,
        picture: &yuv::Picture,
        transform: yuv::Transform,
    ) -> Result<(), Box<dyn Error>> {
        let (pw, ph) = picture.dimensions();
        let (long, short) = (pw.max(ph), pw.min(ph));
        let (w, h) = if self.portrait {
            (short & !1, long)
        } else {
            (long & !1, short)
        };
        if (w, h) != self.requested {
            self.device = None; // close before reopening at the new size
        }
        let device = match self.device.as_mut() {
            Some(d) => d,
            None => {
                let d = v4l2::OutputDevice::open(self.path, w, h)?;
                self.requested = (w, h);
                let (cw, ch) = d.size();
                if (cw, ch) == (w, h) {
                    if self.verbose {
                        println!("Virtual camera set to {cw}x{ch}");
                    }
                } else if self.verbose {
                    println!(
                        "Virtual camera is locked at {cw}x{ch} by an app that has it open \
                         (OBS, ffplay, a browser tab…): fitting the picture into it.\n\
                         Close those apps, then Stop/Start on the phone, to get {w}x{h}."
                    );
                }
                self.device.insert(d)
            }
        };
        let (cw, ch) = device.size();
        yuv::convert(picture, transform, cw, ch, &mut self.frame);
        device.write_frame(&self.frame)?;
        Ok(())
    }
}

/// `~/.config/oculibre` (or `$XDG_CONFIG_HOME/oculibre`).
fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("oculibre")
}

/// Random id of this computer, created once (§7.1: never derived from hardware identifiers).
fn host_id() -> String {
    let dir = Some(config_dir());
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

/// Opens `uri` on the phone plugged in over USB (the app asks the user to confirm).
fn adb_open_uri(uri: &str) -> bool {
    let adb = find_adb();
    let plugged = Command::new(&adb)
        .arg("get-state")
        .output()
        .is_ok_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "device");
    // `adb shell` joins its arguments into one shell command: quote the URI (it has `&`).
    plugged
        && !uri.contains('\'')
        && Command::new(&adb)
            .args([
                "shell",
                "am",
                "start",
                "-a",
                "android.intent.action.VIEW",
                "-d",
            ])
            .arg(format!("'{uri}'"))
            .arg("org.oculibre")
            .output()
            .is_ok_and(|o| o.status.success())
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
