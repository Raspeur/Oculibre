//! `oculibre` — desktop receiver (Linux).
//!
//! Waits for the phone, speaks the Oculibre protocol 0.3 with it (`docs/protocol.typ`), decodes
//! the H.264 video with OpenH264 and writes the frames to a v4l2loopback device, so the phone
//! shows up as a regular webcam. USB connections (loopback) are plaintext; Wi-Fi connections are
//! TLS with pinned keys, for paired phones only. `oculibre bench` measures every camera × mode.

mod console;

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use oculibre_core::host::{self, Host};
use oculibre_core::{bench, decoder, pairing, session, tls};

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
        --usb-accessory     Experimental: USB without developer mode (Android Open
                            Accessory): switches the phones plugged in
    -w, --wifi              Also accept paired phones over the network (Wi-Fi), encrypted
        --pair              Pair a new phone: shows a QR code to scan (implies --wifi)
        --tls12-only        Testing: Wi-Fi with TLS 1.2 only, as with Android 5-9 phones
    -r, --rotate <ROTATION> auto, 0, 90, 180 or 270 (degrees, clockwise) [default: auto]
                            auto: upright whatever the way you hold the phone.
                            A number: fixed, the picture never turns.
    -m, --mirror            Flip the picture horizontally (left ↔ right)
        --decoder <NAME>    auto, vaapi, nvdec, software or openh264 [default: auto]
                            auto: hardware decoding when the computer can
                            (VA-API: AMD, Intel; NVDEC: NVIDIA)
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
    /// What the receiver does: port, virtual camera, USB, Wi-Fi, rotation…
    config: host::Config,
    /// `--pair`: show a pairing QR code at start.
    pair: bool,
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
        config: host::Config::default(),
        pair: false,
        bench: None,
        report: None,
        write_report: true,
        list_pairings: false,
        unpair: None,
    };
    let c = &mut args.config;
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
                c.port = v.parse().map_err(|_| format!("invalid port: {v}"))?;
            }
            "-d" | "--device" => {
                c.device = Some(it.next().ok_or("--device needs a value")?.into());
            }
            "--no-adb" => c.adb = false,
            "--usb-accessory" => c.usb_accessory = true,
            "-w" | "--wifi" => c.wifi = true,
            "--tls12-only" => c.tls12_only = true,
            "--pair" => {
                args.pair = true;
                c.wifi = true;
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
                c.view.rotate = match v.as_str() {
                    "auto" => session::Rotate::Auto,
                    "0" | "90" | "180" | "270" => session::Rotate::Fixed(v.parse().unwrap()),
                    _ => return Err(format!("invalid rotation: {v} (auto, 0, 90, 180 or 270)")),
                };
            }
            "-m" | "--mirror" => c.view.mirror = true,
            "--portrait" => c.portrait = true,
            "--decoder" => {
                let v = it.next().ok_or("--decoder needs a value")?;
                let choice = decoder::Choice::parse(&v).ok_or(format!(
                    "invalid decoder: {v} (auto, vaapi, nvdec, software or openh264)"
                ))?;
                decoder::set_choice(choice);
            }
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
    // Before any thread exists: it may set environment variables.
    decoder::prepare_environment();
    oculibre_core::power::keep_full_speed();
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = parse_args()?;
    if args.list_pairings || args.unpair.is_some() {
        return pairings(&args);
    }

    let host = Host::start(args.config.clone(), console::events(args.config.port))?;
    if args.pair {
        show_pairing(&host)?;
    }
    println!(
        "Listening on {} — press Start in the app (Ctrl+C to quit)",
        host.address()
    );
    match args.bench {
        Some(settings) => run_bench(&host, &args, settings),
        None => {
            host.run();
            Ok(())
        }
    }
}

/// `oculibre pairings` and `oculibre unpair <name>`.
fn pairings(args: &Args) -> Result<(), Box<dyn Error>> {
    let pairings = pairing::Pairings::load(host::config_dir().join("paired-phones.json"));
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
    Ok(())
}

/// `--pair`: a one-time offer shown as a QR code (and sent over USB when a phone is plugged in).
fn show_pairing(host: &Host) -> Result<(), String> {
    let offer = host.pairing_offer()?;
    println!("\n{}", pairing::qr(&offer.uri)?);
    println!(
        "Pair a phone: scan this QR code with the phone's camera app (it opens Oculibre and asks \
         to confirm), then press Start. Valid {} minutes, for one phone.\nLink: {}",
        offer.ttl.as_secs() / 60,
        offer.uri
    );
    if offer.sent_over_usb {
        println!("Also sent to the phone plugged in over USB: confirm on its screen.");
    }
    println!();
    Ok(())
}

/// `oculibre bench`: waits for one phone, drives it through every mode, writes the report.
fn run_bench(host: &Host, args: &Args, settings: bench::Settings) -> Result<(), Box<dyn Error>> {
    println!("Bench: start the Oculibre app on the phone and press Start (any quality).");
    let phone = host.accept().ok_or("stopped before a phone came")?;
    let link = if phone.peer.ip().is_loopback() {
        bench::Link::Usb
    } else {
        bench::Link::Wifi
    };
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
        let result = bench::run(&control, settings, &console::bench_progress);
        control.finish("bench finished");
        *outcome.lock().unwrap() = Some(result);
    });
    // The bench prints its own progress: no status line, no stream messages.
    let end = host.session(phone, oculibre_core::ignore_events(), Some(driver));

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
