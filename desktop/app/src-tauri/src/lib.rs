//! The desktop app's Rust side: the bridge between the window (React, `app/src`) and the engine
//! (`oculibre-core`).
//!
//! - Page → Rust: the `#[tauri::command]` functions below, called from TypeScript with
//!   `invoke("start", { wifi: true })`. Their arguments and results travel as JSON.
//! - Rust → page: every engine [`Event`] is sent to the page as the `engine` event
//!   (`listen("engine", …)` in TypeScript).

use std::sync::{Arc, Mutex};
use std::thread;

use oculibre_core::host::{Config, Host};
use oculibre_core::session::View;
use oculibre_core::{Event, Events};
use tauri::{AppHandle, Emitter, State};

/// What the app keeps between two commands: the receiver, while it runs. Tauri gives every
/// command access to it (`State<Receiver>`); the `Mutex` because commands may run at the same
/// time on different threads.
#[derive(Default)]
struct Receiver {
    host: Mutex<Option<Arc<Host>>>,
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
        .invoke_handler(tauri::generate_handler![start, stop, set_view])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
