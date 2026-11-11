//! `oculibre bench`: walks every camera × mode the phone offers, measures each one, and writes the
//! results into the phone's `devices/<brand>/<model>/report.md`.
//!
//! The phone is driven with the `set_mode` command (protocol 0.2, feature `control.mode`). For each
//! mode: switch, wait for the new stream, let it settle (auto-exposure, clock sync), then average
//! the host's per-second measurements ([`Second`]).

use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use oculibre_protocol::{
    commands, error_codes, features, CameraCapabilities, Capabilities, Hello, ModeCapabilities,
    SetMode,
};

use crate::session::{Control, Second};

/// How long each mode runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// Seconds thrown away after the stream starts (auto-exposure, clock sync). At least 1.
    pub warmup: u32,
    /// Seconds measured.
    pub seconds: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            warmup: 3,
            seconds: 10,
        }
    }
}

/// Time allowed for the phone to switch mode (closing and opening a camera takes ~1 s).
const SET_MODE_TIMEOUT: Duration = Duration::from_secs(15);
/// Seconds to wait for the first picture of a new stream.
const FIRST_PICTURE_SECONDS: u32 = 10;

/// What the bench found.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub phone: Hello,
    pub rows: Vec<Row>,
    /// Number of modes planned (`rows.len()` is smaller when the bench was interrupted).
    pub planned: usize,
    /// Why the bench stopped early.
    pub interrupted: Option<String>,
}

/// One camera × mode.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub camera: String,
    pub facing: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub path: String,
    pub result: Result<Measure, String>,
}

/// Averages over the measured seconds of one mode.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Measure {
    pub seconds: u32,
    pub camera_fps: Option<f64>,
    pub shown_fps: f64,
    pub mbits: f64,
    /// Capture on the phone → written to the virtual camera, ms.
    pub latency_ms: Option<f64>,
    pub latency_max_ms: Option<f64>,
    /// Capture → network, on the phone, ms.
    pub phone_ms: Option<f64>,
    /// Received → virtual camera, on the PC, ms.
    pub pc_ms: Option<f64>,
    pub rtt_ms: Option<f64>,
    /// Frames dropped by the phone during the measurement.
    pub dropped: Option<u64>,
    pub decode_errors: u32,
}

impl Measure {
    /// `base`: the second just before `seconds` (counters are totals since the session start).
    pub fn from_seconds(base: &Second, seconds: &[Second]) -> Measure {
        let n = seconds.len().max(1) as f64;
        let mean = |values: Vec<f64>| {
            (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
        };
        let count: u64 = seconds.iter().map(|s| u64::from(s.latency_count)).sum();
        let sum: i64 = seconds.iter().map(|s| s.latency_sum_us).sum();
        let pc_sum: i64 = seconds.iter().map(|s| s.pc_sum_us).sum();
        let last = seconds.last().unwrap_or(base);
        Measure {
            seconds: seconds.len() as u32,
            camera_fps: mean(seconds.iter().filter_map(|s| s.camera_fps).collect()),
            shown_fps: seconds.iter().map(|s| s.shown_fps).sum::<f64>() / n,
            mbits: seconds.iter().map(|s| s.mbits).sum::<f64>() / n,
            latency_ms: (count > 0).then(|| sum as f64 / count as f64 / 1000.0),
            latency_max_ms: seconds
                .iter()
                .filter(|s| s.latency_count > 0)
                .map(|s| s.latency_max_us)
                .max()
                .map(|us| us as f64 / 1000.0),
            phone_ms: mean(seconds.iter().filter_map(|s| s.phone_latency_ms).collect()),
            pc_ms: (count > 0).then(|| pc_sum as f64 / count as f64 / 1000.0),
            rtt_ms: mean(seconds.iter().filter_map(|s| s.rtt_ms).collect()),
            dropped: match (base.dropped_total, last.dropped_total) {
                (Some(a), Some(b)) => Some(b.saturating_sub(a)),
                _ => None,
            },
            decode_errors: last.errors_total.saturating_sub(base.errors_total),
        }
    }

    /// What remains of the latency once the phone and the PC shares are taken out.
    pub fn network_ms(&self) -> Option<f64> {
        match (self.latency_ms, self.phone_ms, self.pc_ms) {
            (Some(total), Some(phone), Some(pc)) => Some((total - phone - pc).max(0.0)),
            _ => None,
        }
    }
}

/// The capabilities of the phone: from its `HELLO`, else asked with `get_capabilities`.
fn capabilities(control: &Control) -> Result<Capabilities, String> {
    if let Some(value) = &control.phone.capabilities {
        return Capabilities::from_value(value).map_err(|e| format!("bad capabilities: {e}"));
    }
    let response = control.call(
        commands::GET_CAPABILITIES,
        serde_json::Value::Null,
        Duration::from_secs(5),
    )?;
    match (response.ok, response.result) {
        (true, Some(value)) => {
            Capabilities::from_value(&value).map_err(|e| format!("bad capabilities: {e}"))
        }
        _ => Err("the phone did not give its capabilities".into()),
    }
}

/// Runs the whole bench. Progress goes to stdout.
pub fn run(control: &Control, settings: Settings) -> Result<Outcome, String> {
    if !control.phone.has_feature(features::CONTROL_MODE) {
        return Err(format!(
            "the phone app ({} {}) cannot switch mode on request: update it (protocol 0.2)",
            control.phone.app, control.phone.app_version
        ));
    }
    let caps = capabilities(control)?;
    let plan: Vec<(&CameraCapabilities, &ModeCapabilities)> = caps
        .cameras
        .iter()
        .flat_map(|c| c.modes.iter().map(move |m| (c, m)))
        .collect();
    if plan.is_empty() {
        return Err("the phone reports no camera mode".into());
    }
    let per_mode = settings.warmup + settings.seconds + 2;
    let total = per_mode as usize * plan.len();
    println!(
        "{} cameras, {} modes: about {} min {:02} s. Keep the phone still, facing something \
         lit and not completely static.",
        caps.cameras.len(),
        plan.len(),
        total / 60,
        total % 60
    );

    let mut outcome = Outcome {
        phone: control.phone.clone(),
        rows: Vec::new(),
        planned: plan.len(),
        interrupted: None,
    };
    for (i, (camera, mode)) in plan.iter().enumerate() {
        let path = mode.path.clone().unwrap_or_else(|| "normal".into());
        print!(
            "[{:>2}/{}] camera {} ({}) · {}×{} @ {} fps ({path}) … ",
            i + 1,
            plan.len(),
            camera.id,
            camera.facing,
            mode.width,
            mode.height,
            mode.fps
        );
        let _ = std::io::Write::flush(&mut std::io::stdout());
        let result = measure(control, settings, &SetMode::new(camera, mode));
        match &result {
            Ok(m) => println!("{}", short(m)),
            Err(Stop::Mode(e)) => println!("failed: {e}"),
            Err(Stop::Session(e)) => println!("stopped: {e}"),
        }
        let row = |result| Row {
            camera: camera.id.clone(),
            facing: camera.facing.clone(),
            width: mode.width,
            height: mode.height,
            fps: mode.fps,
            path: path.clone(),
            result,
        };
        match result {
            Ok(m) => outcome.rows.push(row(Ok(m))),
            Err(Stop::Mode(e)) => outcome.rows.push(row(Err(e))),
            Err(Stop::Session(e)) => {
                outcome.interrupted = Some(e);
                break;
            }
        }
    }
    Ok(outcome)
}

/// Why a mode has no measurement.
enum Stop {
    /// This mode failed; the bench goes on.
    Mode(String),
    /// The session is over.
    Session(String),
}

fn measure(control: &Control, settings: Settings, set: &SetMode) -> Result<Measure, Stop> {
    let before = control.stream_seq();
    let mut busy_retries = 0;
    let response = loop {
        let r = control
            .call(commands::SET_MODE, set.to_value(), SET_MODE_TIMEOUT)
            .map_err(Stop::Session)?;
        let busy = !r.ok
            && r.error
                .as_ref()
                .is_some_and(|e| e.code == error_codes::BUSY);
        if busy && busy_retries < 5 {
            busy_retries += 1; // e.g. the encoder is still starting
            thread::sleep(Duration::from_secs(1));
            continue;
        }
        break r;
    };
    if !response.ok {
        let e = response.error.unwrap_or_default();
        return Err(Stop::Mode(match e.message {
            Some(m) if !m.is_empty() => format!("{} ({m})", e.code),
            _ => e.code,
        }));
    }

    let gone = || Stop::Session("the phone left".into());
    // Wait for the new stream and its first pictures.
    let mut waited = 0;
    let mut base = loop {
        let s = control.next_second().ok_or_else(gone)?;
        if s.stream_seq > before && s.shown_fps > 0.0 {
            break s;
        }
        waited += 1;
        if waited >= FIRST_PICTURE_SECONDS {
            return Err(Stop::Mode(if control.stream_seq() > before {
                "the stream started but no picture could be decoded".into()
            } else {
                format!("no stream within {FIRST_PICTURE_SECONDS} s")
            }));
        }
    };
    let stream = base.stream_seq;
    for _ in 1..settings.warmup.max(1) {
        base = control.next_second().ok_or_else(gone)?;
    }
    let mut kept = Vec::with_capacity(settings.seconds as usize);
    while kept.len() < settings.seconds.max(1) as usize {
        let s = control.next_second().ok_or_else(gone)?;
        if s.stream_seq != stream {
            return Err(Stop::Mode("the phone restarted the stream".into()));
        }
        kept.push(s);
    }
    if let Some(config) = control.stream() {
        if (config.width, config.height, config.fps) != (set.width, set.height, set.fps) {
            return Err(Stop::Mode(format!(
                "the phone streams {}×{} @ {} instead",
                config.width, config.height, config.fps
            )));
        }
    }
    let m = Measure::from_seconds(&base, &kept);
    if m.shown_fps < 0.5 {
        return Err(Stop::Mode("no picture".into()));
    }
    Ok(m)
}

/// `59.8 fps (camera 60.0) · latency 82 ms (max 120) · 14.1 Mbit/s`
fn short(m: &Measure) -> String {
    let mut s = format!("{:.1} fps", m.shown_fps);
    if let Some(c) = m.camera_fps {
        s += &format!(" (camera {c:.1})");
    }
    match (m.latency_ms, m.latency_max_ms) {
        (Some(l), Some(max)) => s += &format!(" · latency {l:.0} ms (max {max:.0})"),
        _ => s += " · latency unknown",
    }
    s += &format!(" · {:.1} Mbit/s", m.mbits);
    if let Some(d) = m.dropped.filter(|&d| d > 0) {
        s += &format!(" · dropped {d}");
    }
    if m.decode_errors > 0 {
        s += &format!(" · decode errors {}", m.decode_errors);
    }
    s
}

// ------------------------------------------------------------------ report

/// How the phone was connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Usb,
    Wifi,
}

impl Link {
    fn key(self) -> &'static str {
        match self {
            Link::Usb => "usb",
            Link::Wifi => "wifi",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Link::Usb => "USB (`adb reverse`)",
            Link::Wifi => "Wi-Fi (`--insecure-lan`)",
        }
    }
}

/// Context printed above the table.
pub struct Context<'a> {
    pub link: Link,
    pub date: String,
    pub host_os: &'a str,
    pub settings: Settings,
}

fn fmt_opt(v: Option<f64>, decimals: usize) -> String {
    v.map(|v| format!("{v:.decimals$}"))
        .unwrap_or_else(|| "—".into())
}

/// The results as a Markdown table (also printed in the terminal).
pub fn table(rows: &[Row]) -> String {
    let mut t = String::from(
        "| Camera | Mode | Capture | Camera fps | Shown fps | Latency avg / max | \
         Phone + network + PC | Mbit/s | Dropped | Decode errors |\n\
         |---|---|---|---|---|---|---|---|---|---|\n",
    );
    for r in rows {
        let camera = format!("{} {}", r.camera, r.facing);
        let mode = format!("{}×{} @ {}", r.width, r.height, r.fps);
        match &r.result {
            Ok(m) => {
                let latency = match (m.latency_ms, m.latency_max_ms) {
                    (Some(l), Some(max)) => format!("{l:.0} / {max:.0} ms"),
                    _ => "—".into(),
                };
                let split = match (m.phone_ms, m.network_ms(), m.pc_ms) {
                    (Some(p), Some(n), Some(c)) => format!("{p:.0} + {n:.0} + {c:.0}"),
                    _ => "—".into(),
                };
                t += &format!(
                    "| {camera} | {mode} | {} | {} | {:.1} | {latency} | {split} | {:.1} | {} | {} |\n",
                    r.path,
                    fmt_opt(m.camera_fps, 1),
                    m.shown_fps,
                    m.mbits,
                    m.dropped.map(|d| d.to_string()).unwrap_or_else(|| "—".into()),
                    m.decode_errors,
                );
            }
            Err(e) => {
                let e = e.replace('|', "/");
                t += &format!("| {camera} | {mode} | {} | ❌ {e} | | | | | | |\n", r.path);
            }
        }
    }
    t
}

fn markers(link: Link) -> (String, String) {
    (
        format!("<!-- oculibre-bench:{}:start -->", link.key()),
        format!("<!-- oculibre-bench:{}:end -->", link.key()),
    )
}

/// The report section of one link, between markers so that the next run replaces it.
pub fn section(outcome: &Outcome, ctx: &Context<'_>) -> String {
    let (start, end) = markers(ctx.link);
    let p = &outcome.phone;
    let rtt = {
        let v: Vec<f64> = outcome
            .rows
            .iter()
            .filter_map(|r| r.result.as_ref().ok().and_then(|m| m.rtt_ms))
            .collect();
        (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
    };
    let mut s = format!("{start}\n### Automatic bench, {}\n\n", ctx.link.title());
    s += &format!(
        "Generated by `oculibre bench` on {} · {} {} · protocol {}.{} · PC: {} · \
         {} s measured per mode after {} s of warm-up",
        ctx.date,
        p.app,
        p.app_version,
        oculibre_protocol::MAJOR,
        oculibre_protocol::MINOR,
        ctx.host_os,
        ctx.settings.seconds,
        ctx.settings.warmup.max(1),
    );
    if let Some(rtt) = rtt {
        s += &format!(" · round trip {rtt:.1} ms");
    }
    s += ".\n\n";
    if let Some(why) = &outcome.interrupted {
        s += &format!(
            "**Interrupted after {} of {} modes:** {why}.\n\n",
            outcome.rows.len(),
            outcome.planned
        );
    }
    s += &table(&outcome.rows);
    s += "\n- *Latency*: capture on the phone → frame written to the virtual camera, measured by \
          the PC (§11 of the protocol). The app showing the camera and the screen add one or two \
          frames more (glass-to-glass).\n\
          - *Phone + network + PC*: capture → network on the phone (camera, encoder, queue), \
          what remains, and decode → virtual camera on the PC.\n\
          - *Camera fps*: frames the camera really delivers; *shown fps*: frames written to the \
          virtual camera.\n";
    s += &end;
    s += "\n";
    s
}

/// Puts `section` into `report`: replaces the previous run of the same link, or adds it at the
/// end of the "Oculibre results" section (at the end of the file if there is none).
pub fn update_report(report: &str, section: &str, link: Link) -> String {
    let (start, end) = markers(link);
    if let (Some(a), Some(b)) = (report.find(&start), report.find(&end)) {
        if a < b {
            let mut after = b + end.len();
            if report[after..].starts_with('\n') {
                after += 1;
            }
            return format!("{}{section}{}", &report[..a], &report[after..]);
        }
    }
    // Insertion point: after the other link's bench if there is one, else before the heading
    // that follows "## Oculibre results".
    let other = markers(match link {
        Link::Usb => Link::Wifi,
        Link::Wifi => Link::Usb,
    });
    let at = if let Some(b) = report.find(&other.1) {
        let mut at = b + other.1.len();
        if report[at..].starts_with('\n') {
            at += 1;
        }
        at
    } else if let Some(results) = report.find("## Oculibre results") {
        let body = results + "## Oculibre results".len();
        report[body..]
            .match_indices('\n')
            .map(|(i, _)| body + i + 1)
            .find(|&i| report[i..].starts_with("# ") || report[i..].starts_with("## "))
            .unwrap_or(report.len())
    } else {
        report.len()
    };
    let (head, tail) = report.split_at(at);
    let mut out = head.to_string();
    if !out.is_empty() && !out.ends_with("\n\n") {
        out += if out.ends_with('\n') { "\n" } else { "\n\n" };
    }
    out += section;
    if !tail.is_empty() {
        out += "\n";
    }
    out += tail;
    out
}

/// `devices/` of the repository, looked for from the current directory upwards.
pub fn devices_dir() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let d = dir.join("devices");
        if d.join("README.md").is_file() {
            return Some(d);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// The report whose "| Model |" line names this phone (written by `scripts/device-report.py`).
pub fn find_report(devices: &Path, model: &str) -> Option<PathBuf> {
    let model = model.trim().to_lowercase();
    if model.is_empty() {
        return None;
    }
    let brands = fs::read_dir(devices).ok()?;
    for brand in brands.flatten() {
        let Ok(phones) = fs::read_dir(brand.path()) else {
            continue;
        };
        for phone in phones.flatten() {
            let report = phone.path().join("report.md");
            let Ok(text) = fs::read_to_string(&report) else {
                continue;
            };
            // `| Model | SM-S918B (codename dm3q) |`
            let names_it = text.lines().any(|l| {
                l.strip_prefix("| Model |").is_some_and(|cell| {
                    let cell = cell.trim().trim_end_matches('|').trim();
                    let name = cell.split(" (codename").next().unwrap_or(cell);
                    name.trim().to_lowercase() == model
                })
            });
            if names_it {
                return Some(report);
            }
        }
    }
    None
}

/// Folder name as `scripts/device-report.py` makes it.
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-');
    if out.is_empty() {
        "unknown".into()
    } else {
        out.into()
    }
}

/// Today's date (UTC) as `YYYY-MM-DD`.
pub fn today() -> String {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0);
    civil_date(days as i64)
}

/// Days since 1970-01-01 → `YYYY-MM-DD` (Howard Hinnant's `civil_from_days`).
fn civil_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn second(seq: u64, latency_ms: i64, frames: u32, dropped: u64, errors: u32) -> Second {
        Second {
            stream_seq: seq,
            shown_fps: f64::from(frames),
            camera_fps: Some(60.0),
            mbits: 12.0,
            latency_sum_us: latency_ms * 1000 * i64::from(frames),
            latency_max_us: (latency_ms + 10) * 1000,
            latency_count: frames,
            pc_sum_us: 10_000 * i64::from(frames),
            phone_latency_ms: Some(40.0),
            rtt_ms: Some(2.0),
            dropped_total: Some(dropped),
            errors_total: errors,
        }
    }

    #[test]
    fn measure_averages_and_counts_from_the_base() {
        let base = second(2, 500, 60, 7, 1);
        let kept = [second(2, 80, 60, 7, 1), second(2, 90, 60, 9, 2)];
        let m = Measure::from_seconds(&base, &kept);
        assert_eq!(m.seconds, 2);
        assert_eq!(m.shown_fps, 60.0);
        assert_eq!(m.latency_ms, Some(85.0));
        assert_eq!(m.latency_max_ms, Some(100.0));
        assert_eq!(m.pc_ms, Some(10.0));
        assert_eq!(m.network_ms(), Some(35.0));
        assert_eq!(m.dropped, Some(2));
        assert_eq!(m.decode_errors, 1);
    }

    #[test]
    fn measure_without_clock() {
        let mut s = second(1, 0, 30, 0, 0);
        s.latency_count = 0;
        s.phone_latency_ms = None;
        let m = Measure::from_seconds(&s, &[s.clone()]);
        assert_eq!(m.latency_ms, None);
        assert_eq!(m.latency_max_ms, None);
        assert_eq!(m.network_ms(), None);
    }

    fn rows() -> Vec<Row> {
        vec![
            Row {
                camera: "0".into(),
                facing: "back".into(),
                width: 1920,
                height: 1080,
                fps: 60,
                path: "unlisted".into(),
                result: Ok(Measure::from_seconds(
                    &second(1, 82, 60, 0, 0),
                    &[second(1, 82, 60, 0, 0)],
                )),
            },
            Row {
                camera: "1".into(),
                facing: "front".into(),
                width: 3840,
                height: 2160,
                fps: 30,
                path: "normal".into(),
                result: Err("failed (camera | refused)".into()),
            },
        ]
    }

    #[test]
    fn markdown_table() {
        assert_eq!(
            table(&rows()),
            "| Camera | Mode | Capture | Camera fps | Shown fps | Latency avg / max | \
             Phone + network + PC | Mbit/s | Dropped | Decode errors |\n\
             |---|---|---|---|---|---|---|---|---|---|\n\
             | 0 back | 1920×1080 @ 60 | unlisted | 60.0 | 60.0 | 82 / 92 ms | 40 + 32 + 10 | 12.0 | 0 | 0 |\n\
             | 1 front | 3840×2160 @ 30 | normal | ❌ failed (camera / refused) | | | | | | |\n"
        );
    }

    fn outcome() -> Outcome {
        Outcome {
            phone: Hello {
                app: "oculibre-android".into(),
                app_version: "0.2.0".into(),
                ..Default::default()
            },
            rows: rows(),
            planned: 2,
            interrupted: None,
        }
    }

    fn ctx(link: Link) -> Context<'static> {
        Context {
            link,
            date: "2026-11-11".into(),
            host_os: "linux 7.0",
            settings: Settings::default(),
        }
    }

    #[test]
    fn section_is_inserted_then_replaced() {
        let report = "# phone\n\n## Oculibre results\n\n- Notes:\n\n# Labs Result\nS0\n";
        let usb = section(&outcome(), &ctx(Link::Usb));
        assert!(usb.starts_with("<!-- oculibre-bench:usb:start -->\n### Automatic bench, USB"));
        assert!(usb.contains("protocol 0.2 · PC: linux 7.0 · 10 s measured per mode after 3 s"));
        assert!(usb.contains("round trip 2.0 ms"));

        let once = update_report(report, &usb, Link::Usb);
        assert!(once.starts_with("# phone\n\n## Oculibre results\n\n- Notes:\n\n<!-- oculibre"));
        assert!(once.ends_with("<!-- oculibre-bench:usb:end -->\n\n# Labs Result\nS0\n"));

        // A second run replaces the first one, byte for byte the same.
        assert_eq!(update_report(&once, &usb, Link::Usb), once);

        // Wi-Fi goes right after USB.
        let wifi = section(&outcome(), &ctx(Link::Wifi));
        let both = update_report(&once, &wifi, Link::Wifi);
        let u = both.find("usb:end").unwrap();
        let w = both.find("wifi:start").unwrap();
        let labs = both.find("# Labs Result").unwrap();
        assert!(u < w && w < labs);
        assert_eq!(update_report(&both, &wifi, Link::Wifi), both);
    }

    #[test]
    fn section_without_results_heading_goes_at_the_end() {
        let out = update_report("# phone\n", "S\n", Link::Usb);
        assert_eq!(out, "# phone\n\nS\n");
        assert_eq!(update_report("", "S\n", Link::Usb), "S\n");
    }

    #[test]
    fn interrupted_bench_says_so() {
        let mut o = outcome();
        o.planned = 9;
        o.interrupted = Some("the phone left".into());
        assert!(section(&o, &ctx(Link::Usb))
            .contains("**Interrupted after 2 of 9 modes:** the phone left."));
    }

    #[test]
    fn finds_the_report_by_model() {
        let dir = std::env::temp_dir().join(format!("oculibre-bench-test-{}", std::process::id()));
        let phone = dir.join("samsung/galaxy-s23-ultra");
        fs::create_dir_all(&phone).unwrap();
        fs::write(
            phone.join("report.md"),
            "# samsung Galaxy S23 Ultra\n\n| | |\n|---|---|\n| Model | SM-S918B (codename dm3q) |\n",
        )
        .unwrap();
        assert_eq!(find_report(&dir, "SM-S918B"), Some(phone.join("report.md")));
        assert_eq!(
            find_report(&dir, " sm-s918b"),
            Some(phone.join("report.md"))
        );
        assert_eq!(find_report(&dir, "SM-S918"), None);
        assert_eq!(find_report(&dir, "Pixel 3"), None);
        assert_eq!(find_report(&dir, ""), None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn slugs_like_device_report() {
        assert_eq!(slugify("Redmi Note 9 Pro"), "redmi-note-9-pro");
        assert_eq!(slugify("  SM-S918B "), "sm-s918b");
        assert_eq!(slugify("Galaxy S23 Ultra (5G)"), "galaxy-s23-ultra-5g");
        assert_eq!(slugify("?"), "unknown");
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(20_403), "2025-11-11");
        assert_eq!(civil_date(20_768), "2026-11-11");
        assert_eq!(civil_date(11_016), "2000-02-29");
    }
}
