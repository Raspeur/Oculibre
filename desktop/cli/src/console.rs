//! The engine's events as console text: what `oculibre` prints while a phone is connected.

use std::io::{self, Write};
use std::sync::Arc;

use oculibre_core::bench::{self, Progress};
use oculibre_core::events::{Accessory, Refusal, Via, WifiInfo};
use oculibre_core::session::{End, Status, Summary, AVERAGE_WINDOW};
use oculibre_core::{Event, Events};
use oculibre_protocol::{Hello, StreamConfig};

/// Prints every event of the engine (status line rewritten in place every second). `port`: for
/// the `adb reverse` message.
pub fn events(port: u16) -> Events {
    Arc::new(move |event| match event {
        Event::VirtualCamera { path } => println!("Virtual camera: {}", path.display()),
        Event::Usb(Ok(())) => {
            println!("USB: adb reverse tcp:{port} OK — use 127.0.0.1 in the app")
        }
        Event::Usb(Err(e)) => eprintln!("warning: {e} — USB won't work, Wi-Fi still will"),
        Event::Accessory(Accessory::Switched { phone }) => {
            println!("USB: {phone} switched to accessory mode — accept \"Open Oculibre\" on it")
        }
        // The command line switches every phone: nothing is announced.
        Event::Accessory(Accessory::Detected { .. } | Accessory::Gone { .. }) => {}
        Event::Accessory(Accessory::Waiting) => {
            println!("USB: phone in accessory mode — waiting for the app to start")
        }
        Event::Accessory(Accessory::Failed(e)) => eprintln!("warning: USB accessory: {e}"),
        Event::Wifi(info) => println!("{}", wifi_line(&info)),
        Event::AnnounceFailed(e) => eprintln!(
            "warning: cannot announce this computer on the network (mDNS: {e}): \
             phones will use the address they know"
        ),
        Event::PhoneConnected { peer, via } => match via {
            Via::Usb => println!("Phone connected over USB ({peer})"),
            Via::Wifi { tls } => {
                println!("Phone connected over Wi-Fi ({peer}), encrypted ({tls})")
            }
        },
        Event::PhoneRefused { peer, reason } => match reason {
            Refusal::WifiOff => eprintln!(
                "Refused a connection from {peer}: start with --wifi to accept paired phones"
            ),
            Refusal::Tls(e) => eprintln!("Refused a connection from {peer}: {e}"),
        },
        Event::AcceptFailed(e) => eprintln!("accept failed: {e}"),
        Event::Paired { name } => {
            println!("Paired with {name}: it can now connect over Wi-Fi with `oculibre --wifi`")
        }
        Event::SessionEnded(end) => match end {
            Ok(End::Bye { reason, message }) => println!(
                "\nPhone stopped ({reason}{}) — waiting for it…",
                message.map(|m| format!(": {m}")).unwrap_or_default()
            ),
            Ok(End::Closed | End::Finished) => println!("\nPhone disconnected — waiting for it…"),
            Err(e) => eprintln!("\nConnection ended: {e} — waiting for the phone…"),
        },
        Event::VirtualCameraSize {
            wanted: (w, h),
            size: (cw, ch),
        } => {
            if (cw, ch) == (w, h) {
                println!("Virtual camera set to {cw}x{ch}");
            } else {
                println!(
                    "Virtual camera is locked at {cw}x{ch} by an app that has it open \
                     (OBS, ffplay, a browser tab…): fitting the picture into it.\n\
                     Close those apps, then Stop/Start on the phone, to get {w}x{h}."
                );
            }
        }
        Event::Phone(hello) => println!("{}", phone_line(&hello)),
        Event::Stream(config) => println!("\n{}", stream_line(&config)),
        Event::StreamPaused { reason } => println!("\nStream paused ({reason})"),
        Event::Transform { transform, auto } => {
            let mode = if auto {
                " (auto: follows the phone)"
            } else {
                ""
            };
            let mirror = if transform.mirror { ", mirrored" } else { "" };
            println!("\nRotation: {}°{mode}{mirror}", transform.rotation);
        }
        Event::Decoder { name } => println!("\nDecoder: {name}"),
        Event::DecoderTooSlow {
            from,
            decode_ms,
            frame_ms,
        } => println!(
            "\n{from} takes {decode_ms:.1} ms per picture, the phone sends one every \
             {frame_ms:.1} ms: decoding on the processor instead"
        ),
        Event::RequestRefused { id, code, message } => eprintln!(
            "\nphone refused request {id}: {code} {}",
            message.unwrap_or_default()
        ),
        Event::Status(status) => {
            // `\r` goes back to the start of the line, `\x1b[K` clears the rest of the previous
            // one (it may have been longer).
            print!("\r{}\x1b[K", status_line(&status));
            let _ = io::stdout().flush();
        }
        Event::Summary(summary) => println!("\n{}", summary_text(&summary)),
    })
}

/// `oculibre bench` progress, one line per mode.
pub fn bench_progress(progress: Progress<'_>) {
    match progress {
        Progress::Plan {
            cameras,
            modes,
            seconds,
        } => println!(
            "{cameras} cameras, {modes} modes: about {} min {:02} s. Keep the phone still, \
             facing something lit and not completely static.",
            seconds / 60,
            seconds % 60
        ),
        Progress::Mode {
            index,
            total,
            camera,
            mode,
        } => {
            print!(
                "[{index:>2}/{total}] camera {} ({}) · {}×{} @ {} fps ({}) … ",
                camera.id,
                camera.facing,
                mode.width,
                mode.height,
                mode.fps,
                mode.path.as_deref().unwrap_or("normal")
            );
            let _ = io::stdout().flush();
        }
        Progress::Measured(m) => println!("{}", bench::short(m)),
        Progress::Failed(e) => println!("failed: {e}"),
        Progress::Stopped(e) => println!("stopped: {e}"),
    }
}

/// `Wi-Fi: encrypted, paired phones only (2 paired) · this computer: 192.168.1.50 · key 46B5 EE53 · …`
fn wifi_line(info: &WifiInfo) -> String {
    let list: Vec<String> = info.addresses.iter().map(|a| a.to_string()).collect();
    format!(
        "Wi-Fi: encrypted, paired phones only ({} paired) · this computer: {} · key {}{}",
        info.paired,
        if list.is_empty() {
            "no network address".into()
        } else {
            list.join(", ")
        },
        info.key,
        if info.announced {
            " · announced (mDNS)"
        } else {
            ""
        }
    )
}

/// `Phone: Redmi Note 9 Pro (oculibre-android 0.3.0, android 12)`
fn phone_line(phone: &Hello) -> String {
    let device = phone.device.clone().unwrap_or_default();
    let name = if phone.name.is_empty() {
        device.model.unwrap_or_default()
    } else {
        phone.name.clone()
    };
    format!(
        "Phone: {name} ({} {}, {} {})",
        phone.app,
        phone.app_version,
        device.os.unwrap_or_default(),
        device.os_version.unwrap_or_default()
    )
}

/// `Stream: 1920x1080 at 60 fps (unlisted capture, back camera, sensor at 90°)`
fn stream_line(config: &StreamConfig) -> String {
    let camera = config
        .camera
        .as_ref()
        .map(|c| format!(", {} camera, sensor at {}°", c.facing, c.sensor_orientation))
        .unwrap_or_default();
    format!(
        "Stream: {}x{} at {} fps ({} capture{camera})",
        config.width,
        config.height,
        config.fps,
        config.path.as_deref().unwrap_or("normal"),
    )
}

/// `60 fps (camera 60) · 11.8 Mbit/s · latency 48 ms (phone 30 + network 6 + PC 12, max 61) · …`;
/// with a newer app the phone share is split: `(camera 10 + encoder 18 + queue 2 + network 6 …`
fn status_line(s: &Status) -> String {
    let mut line = format!("{:>3.0} fps", s.shown_fps);
    if let Some(c) = s.camera_fps {
        line += &format!(" (camera {c})");
    }
    line += &format!(" · {:>4.1} Mbit/s", s.mbits);
    match s.latency {
        Some(total) => {
            line += &format!(" · latency {total:.0} ms");
            if let (Some(phone), Some(pc)) = (s.phone, s.pc) {
                // The network share is what remains: transfer, waiting in buffers, clock error.
                let network = (total - phone - pc).max(0.0);
                let phone_text = match s.split {
                    Some(split) => split.text(),
                    None => format!("phone {phone:.0}"),
                };
                line += &format!(" ({phone_text} + network {network:.0} + PC {pc:.0}");
                if let Some(max) = s.latency_max {
                    line += &format!(", max {max:.0}");
                }
                line += ")";
            } else if let Some(max) = s.latency_max {
                line += &format!(" (max {max:.0})");
            }
        }
        None if s.clock_mismatch => {
            line += " · latency: unavailable (the phone's frame times don't match its clock)"
        }
        None => line += " · latency: syncing clocks…",
    }
    if let (Some(_), Some(avg)) = (s.latency, s.latency_avg) {
        line += &format!(" · avg {AVERAGE_WINDOW} s: {avg:.0} ms");
    }
    if let Some(rtt) = s.rtt {
        line += &format!(" · rtt {rtt:.1} ms");
    }
    if let Some(d) = s.dropped.filter(|&d| d > 0) {
        line += &format!(" · dropped by phone {d}");
    }
    if s.skipped > 0 {
        line += &format!(" · skipped by this computer {}", s.skipped);
    }
    if s.errors > 0 {
        line += &format!(" · decode errors {}", s.errors);
    }
    if s.behind {
        line += " · this computer is not keeping up (power saving? other programs?)";
    }
    if s.link_slow {
        line += &match s.target_mbits {
            Some(m) => {
                format!(" · connection too slow: the phone lowered the bitrate to {m:.1} Mbit/s")
            }
            None => " · connection too slow: late video dropped".to_string(),
        };
    }
    line
}

/// `Session: 2 min 13 s · 59.4 fps · latency 83 ms on average (max 140 ms) · …`, then the phone
/// and PC shares per frame on their own lines.
fn summary_text(s: &Summary) -> String {
    let (m, sec) = (s.seconds / 60, s.seconds % 60);
    let duration = if m > 0 {
        format!("{m} min {sec} s")
    } else {
        format!("{sec} s")
    };
    let mut text = format!("Session: {duration} · {:.1} fps", s.fps);
    if let (Some(avg), Some(max)) = (s.latency_avg, s.latency_max) {
        text += &format!(" · latency {avg:.0} ms on average (max {max:.0} ms)");
    }
    if let Some(d) = s.dropped {
        text += &format!(" · dropped by phone {d}");
    }
    text += &format!(" · decode errors {}", s.errors);
    if let Some(phone) = s.phone {
        text += &format!("\nPhone per frame: {} ms", phone.text());
    }
    if let (Some(decode), Some(output)) = (s.pc_decode, s.pc_output) {
        text += &format!(
            "\nPC per frame: decode {decode:.1} ms + rotation, conversion and write {output:.1} ms"
        );
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use oculibre_core::session::PhoneSplit;

    #[test]
    fn full_status_line() {
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
            pc_decode: Some(5.2),
            rtt: Some(1.23),
            dropped: Some(0),
            skipped: 0,
            behind: false,
            link_slow: false,
            target_mbits: None,
            errors: 0,
            clock_mismatch: false,
        };
        assert_eq!(
            status_line(&s),
            " 60 fps (camera 60) · 11.8 Mbit/s · latency 48 ms \
             (phone 30 + network 6 + PC 12, max 61) · avg 10 s: 51 ms · rtt 1.2 ms"
        );
    }

    #[test]
    fn status_line_with_the_phone_split() {
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
            status_line(&s),
            " 60 fps · 12.0 Mbit/s · latency 90 ms \
             (camera 31 + encoder 47 + queue 2 + network 3 + PC 7)"
        );
    }

    #[test]
    fn status_line_before_clock_sync() {
        let s = Status {
            shown_fps: 30.0,
            errors: 2,
            ..Default::default()
        };
        assert_eq!(
            status_line(&s),
            " 30 fps ·  0.0 Mbit/s · latency: syncing clocks… · decode errors 2"
        );
    }

    #[test]
    fn session_summary() {
        let s = Summary {
            seconds: 133,
            fps: 60.0,
            latency_avg: Some(80.45),
            latency_max: Some(145.0),
            dropped: Some(3),
            errors: 0,
            ..Default::default()
        };
        assert_eq!(
            summary_text(&s),
            "Session: 2 min 13 s · 60.0 fps · latency 80 ms on average (max 145 ms) \
             · dropped by phone 3 · decode errors 0"
        );
    }

    #[test]
    fn session_summary_with_the_shares() {
        let s = Summary {
            seconds: 42,
            fps: 29.9,
            phone: Some(PhoneSplit {
                camera: 30.0,
                encoder: 21.0,
                queue: 1.0,
            }),
            pc_decode: Some(5.04),
            pc_output: Some(2.0),
            ..Default::default()
        };
        assert_eq!(
            summary_text(&s),
            "Session: 42 s · 29.9 fps · decode errors 0\n\
             Phone per frame: camera 30 + encoder 21 + queue 1 ms\n\
             PC per frame: decode 5.0 ms + rotation, conversion and write 2.0 ms"
        );
    }
}
