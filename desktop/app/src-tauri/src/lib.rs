//! The desktop app's Rust side: the bridge between the window (React, `app/src`) and the engine
//! (`oculibre-core`).
//!
//! - Page → Rust: the `#[tauri::command]` functions below, called from TypeScript with
//!   `invoke("start", { wifi: true })`. Their arguments and results travel as JSON.
//! - Rust → page: every engine [`Event`] is sent to the page as the `engine` event
//!   (`listen("engine", …)` in TypeScript).

use std::sync::{Arc, Mutex};
use std::thread;

use oculibre_core::host::{self, Config, Host};
use oculibre_core::pairing::{self, Pairings};
use oculibre_core::preview::Viewer;
use oculibre_core::protocol::{Capabilities, SetMode};
use oculibre_core::session::{ModeError, Remote, View};
use oculibre_core::{Event, Events};
use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, State};

/// What the app keeps between two commands: the receiver, while it runs. Tauri gives every
/// command access to it (`State<Receiver>`); the `Mutex` because commands may run at the same
/// time on different threads.
#[derive(Default)]
struct Receiver {
    host: Mutex<Option<Arc<Host>>>,
    /// Where preview pictures go (the page's channel), kept to give it to every new host.
    viewer: Mutex<Option<Viewer>>,
}

/// Starts waiting for phones (USB, and the network when `wifi` is on). Returns where phones
/// connect ("127.0.0.1:47100").
///
/// `async`: run on a worker thread, not on the main thread that draws the window (`adb reverse`
/// and the mDNS announcement can take a second).
#[tauri::command(async)]
fn start(app: AppHandle, receiver: State<Receiver>, wifi: bool) -> Result<String, String> {
    let mut slot = receiver.host.lock().unwrap();
    if let Some(host) = slot.as_ref() {
        return Ok(host.address().to_string()); // already running
    }
    // Every engine event goes to the page, as JSON.
    let events: Events = Arc::new(move |event: Event| {
        let _ = app.emit("engine", &event);
    });
    let config = Config {
        wifi,
        ..Config::default()
    };
    let host = Arc::new(Host::start(config, events)?);
    host.set_preview(receiver.viewer.lock().unwrap().clone());
    // `run` blocks while it serves phones: on a thread of its own, so the window stays free.
    let runner = host.clone();
    thread::Builder::new()
        .name("oculibre-host".into())
        .spawn(move || runner.run())
        .map_err(|e| e.to_string())?;
    let address = host.address().to_string();
    *slot = Some(host);
    Ok(address)
}

/// Stops the receiver: the phone being served (if any) is let go, the port is closed.
#[tauri::command(async)]
fn stop(receiver: State<Receiver>) {
    if let Some(host) = receiver.host.lock().unwrap().take() {
        host.stop();
    }
}

/// The phone being served, if any. The lock is only held to copy the handle: a slow command to
/// the phone never blocks the other commands.
fn remote(receiver: &Receiver) -> Result<Remote, String> {
    let slot = receiver.host.lock().unwrap();
    let host = slot.as_ref().ok_or("the receiver is not running")?;
    host.remote().ok_or_else(|| "no phone is connected".into())
}

/// The cameras of the phone being served and their modes (size, frame rate, capture path).
#[tauri::command(async)]
fn capabilities(receiver: State<Receiver>) -> Result<Capabilities, String> {
    remote(&receiver)?.capabilities()
}

/// Switches the phone to another camera or quality; resolves once it streams in it. A refused
/// mode (`{kind: "Refused"}`) leaves the phone in its previous mode.
#[tauri::command(async)]
fn set_mode(receiver: State<Receiver>, mode: SetMode) -> Result<(), ModeError> {
    remote(&receiver)
        .map_err(ModeError::Session)?
        .set_mode(&mode)
}

/// Stops the phone's camera (`BYE user_stop`); the receiver keeps waiting for phones.
#[tauri::command(async)]
fn finish(receiver: State<Receiver>) {
    if let Ok(remote) = remote(&receiver) {
        remote.finish("stopped from the computer");
    }
}

/// The phones paired for Wi-Fi, as stored on this computer.
#[tauri::command]
fn paired_phones() -> Vec<pairing::Phone> {
    Pairings::load(host::config_dir().join("paired-phones.json")).phones()
}

/// A one-time pairing offer, ready to show (see `Host::pairing_offer`).
#[derive(Serialize)]
struct PairingOffer {
    /// `oculibre://pair?…`, what the QR code holds.
    uri: String,
    qr: pairing::QrGrid,
    /// Valid this long, for one phone.
    ttl_seconds: u64,
    /// Also opened on the phone plugged in over USB.
    sent_over_usb: bool,
}

/// Opens a pairing offer (Wi-Fi must be on): the window shows its QR code; the phone that scans
/// it and confirms is paired (`Paired` event).
#[tauri::command(async)]
fn pairing_offer(receiver: State<Receiver>) -> Result<PairingOffer, String> {
    let host = receiver
        .host
        .lock()
        .unwrap()
        .clone()
        .ok_or("the receiver is not running")?;
    let offer = host.pairing_offer()?;
    Ok(PairingOffer {
        qr: pairing::qr_grid(&offer.uri)?,
        uri: offer.uri,
        ttl_seconds: offer.ttl.as_secs(),
        sent_over_usb: offer.sent_over_usb,
    })
}

/// Sends the page a small JPEG of the output about 10 times a second, through `on_frame` (raw
/// bytes: an `ArrayBuffer` in the page). A new call replaces the previous channel (the page was
/// reloaded).
#[tauri::command]
fn preview(receiver: State<Receiver>, on_frame: Channel<InvokeResponseBody>) {
    let viewer: Viewer = Arc::new(move |frame| {
        // The page may be gone (reloaded): then nothing listens, and that is fine.
        let _ = on_frame.send(InvokeResponseBody::Raw(frame.jpeg));
    });
    *receiver.viewer.lock().unwrap() = Some(viewer.clone());
    if let Some(host) = receiver.host.lock().unwrap().as_ref() {
        host.set_preview(Some(viewer));
    }
}

/// Rotation and mirror, for the phone being served and the next ones.
#[tauri::command]
fn set_view(receiver: State<Receiver>, view: View) -> Result<(), String> {
    let slot = receiver.host.lock().unwrap();
    let host = slot.as_ref().ok_or("the receiver is not running")?;
    host.set_view(view);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Before any thread exists: it may set environment variables (VA-API drivers).
    oculibre_core::decoder::prepare_environment();
    tauri::Builder::default()
        .manage(Receiver::default())
        .invoke_handler(tauri::generate_handler![
            start,
            stop,
            set_view,
            capabilities,
            set_mode,
            finish,
            paired_phones,
            pairing_offer,
            preview
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
