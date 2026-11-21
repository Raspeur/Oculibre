//! The desktop app's Rust side: the bridge between the window (React, `app/src`) and the engine
//! (`oculibre-core`).
//!
//! - Page → Rust: the `#[tauri::command]` functions below, called from TypeScript with
//!   `invoke("start", { wifi: true })`. Their arguments and results travel as JSON.
//! - Rust → page: every engine [`Event`] is sent to the page as the `engine` event
//!   (`listen("engine", …)` in TypeScript); `receiver` says when the receiver (re)started or
//!   could not, `phones` when the paired phones changed.

mod settings;
mod tray;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use oculibre_core::decoder::{self, Choice};
use oculibre_core::host::{self, Config, Host, KnownPhone};
use oculibre_core::pairing::{self, Pairings};
use oculibre_core::preview::{Rate, Viewer, Watch};
use oculibre_core::protocol::{Capabilities, SetMode};
use oculibre_core::session::{ModeError, Remote, View};
use oculibre_core::vcam_setup::{self, FixError, VcamStatus};
use oculibre_core::{Event, Events};
use serde::{Deserialize, Serialize};
use settings::AppSettings;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

/// What the app keeps between two commands: the receiver, while it runs. Tauri gives every
/// command access to it (`State<Receiver>`); the `Mutex` because commands may run at the same
/// time on different threads.
#[derive(Default)]
struct Receiver {
    host: Mutex<Option<Arc<Host>>>,
    /// The thread running the host: waited for when stopping, so that the port is free again.
    runner: Mutex<Option<JoinHandle<()>>>,
    /// This computer's settings (loaded at start, saved at each change).
    settings: Mutex<AppSettings>,
    /// Settings that need a new receiver were changed while a phone streamed: restart when its
    /// session ends.
    restart_pending: AtomicBool,
    /// The tray menu lines that follow the phone, and how the phone came ("USB", "Wi-Fi").
    tray: Mutex<Option<tray::TrayMenu>>,
    via: Mutex<&'static str>,
    /// Where preview pictures go (the page's channel), kept to give it to every new host.
    viewer: Mutex<Option<Viewer>>,
    /// The preview chosen in the window.
    preview: Mutex<PreviewMode>,
}

/// The preview control of the window.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
enum PreviewMode {
    /// Nothing is computed.
    Off,
    /// About 10 pictures a second: never slows the source down.
    #[default]
    Light,
    /// Every picture: when nothing else needs the computer.
    Full,
}

impl Receiver {
    /// What the host should do with previews now: the page's channel at the chosen rate.
    fn watch(&self) -> Option<Watch> {
        let rate = match *self.preview.lock().unwrap() {
            PreviewMode::Off => return None,
            PreviewMode::Light => Rate::Light,
            PreviewMode::Full => Rate::Full,
        };
        let viewer = self.viewer.lock().unwrap().clone()?;
        Some(Watch { viewer, rate })
    }

    /// Gives the host (if running) the current preview choice.
    fn apply_preview(&self) {
        if let Some(host) = self.host.lock().unwrap().as_ref() {
            host.set_preview(self.watch());
        }
    }
}

/// Starts the receiver with the current settings; returns where phones connect. Nothing happens
/// if it already runs.
fn start_host(app: &AppHandle, receiver: &Receiver) -> Result<String, String> {
    let mut slot = receiver.host.lock().unwrap();
    if let Some(host) = slot.as_ref() {
        return Ok(host.address().to_string());
    }
    let settings = receiver.settings.lock().unwrap().clone();
    decoder::set_choice(Choice::parse(&settings.decoder).unwrap_or(Choice::Auto));
    // Every engine event goes to the page, as JSON.
    let handle = app.clone();
    let events: Events = Arc::new(move |event: Event| {
        let ended = matches!(event, Event::SessionEnded(_));
        let _ = handle.emit("engine", &event);
        follow_in_tray(&handle, &event);
        if ended
            && handle
                .state::<Receiver>()
                .restart_pending
                .swap(false, Ordering::Relaxed)
        {
            // On a thread of its own: this runs on the host's thread, which a restart waits for.
            let handle = handle.clone();
            thread::spawn(move || restart(&handle));
        }
    });
    let config = Config {
        wifi: settings.wifi,
        portrait: settings.portrait,
        // Each phone's rotation, mirror, camera and quality, applied again when it comes back.
        settings_file: Some(host::config_dir().join("phone-settings.json")),
        ..Config::default()
    };
    let host = Arc::new(Host::start(config, events)?);
    host.set_preview(receiver.watch());
    // `run` blocks while it serves phones: on a thread of its own, so the window stays free.
    let runner = host.clone();
    let thread = thread::Builder::new()
        .name("oculibre-host".into())
        .spawn(move || runner.run())
        .map_err(|e| e.to_string())?;
    *receiver.runner.lock().unwrap() = Some(thread);
    let address = host.address().to_string();
    *slot = Some(host);
    Ok(address)
}

/// The tray menu says which phone streams.
fn follow_in_tray(app: &AppHandle, event: &Event) {
    let receiver = app.state::<Receiver>();
    match event {
        Event::PhoneConnected { via, .. } => {
            *receiver.via.lock().unwrap() = match via {
                oculibre_core::events::Via::Usb => "USB",
                oculibre_core::events::Via::Wifi { .. } => "Wi-Fi",
            };
        }
        Event::Phone(hello) => {
            let name = if hello.name.is_empty() {
                hello
                    .device
                    .as_ref()
                    .and_then(|d| d.model.clone())
                    .unwrap_or_default()
            } else {
                hello.name.clone()
            };
            let via = *receiver.via.lock().unwrap();
            if let Some(menu) = receiver.tray.lock().unwrap().as_mut() {
                menu.show_phone(Some((&name, via)));
            }
        }
        Event::SessionEnded(_) => {
            if let Some(menu) = receiver.tray.lock().unwrap().as_mut() {
                menu.show_phone(None);
            }
        }
        _ => {}
    }
}

/// Stops the receiver and waits until its thread is done (the port is then free).
fn stop_host(receiver: &Receiver) {
    let host = receiver.host.lock().unwrap().take();
    if let Some(host) = host {
        host.stop();
    }
    let runner = receiver.runner.lock().unwrap().take();
    if let Some(runner) = runner {
        let _ = runner.join();
    }
}

/// A new receiver with the current settings; the page learns the outcome (`receiver` event:
/// `{Ok: address}` or `{Err: why}`).
fn restart(app: &AppHandle) {
    let receiver = app.state::<Receiver>();
    stop_host(&receiver);
    let result = start_host(app, &receiver);
    let _ = app.emit("receiver", &result);
}

/// Starts waiting for phones (USB, and the network when Wi-Fi is on in the settings). Returns
/// where phones connect ("127.0.0.1:47100").
///
/// `async`: run on a worker thread, not on the main thread that draws the window (`adb reverse`
/// and the mDNS announcement can take a second).
#[tauri::command(async)]
fn start(app: AppHandle, receiver: State<Receiver>) -> Result<String, String> {
    start_host(&app, &receiver)
}

/// Why each decoding library is missing (`None`: it is there). Without FFmpeg OpenH264 decodes
/// (problem 502); without either, nothing does (503).
/// With what was found, for About: FFmpeg's version and licence (as the distribution built it),
/// OpenH264's version.
#[derive(Serialize)]
struct Decoders {
    ffmpeg: Option<String>,
    openh264: Option<String>,
    ffmpeg_info: Option<decoder::FfmpegInfo>,
    openh264_version: Option<String>,
}

#[tauri::command(async)]
fn decoders() -> Decoders {
    // Windows: the main decoder is Windows' own (Media Foundation), in FFmpeg's place: the page
    // shows it with Windows' words (problems 502/503: the Media Feature Pack).
    #[cfg(windows)]
    let main = decoder::media_foundation_available().err();
    #[cfg(not(windows))]
    let main = decoder::ffmpeg_available().err();
    Decoders {
        ffmpeg: main,
        openh264: decoder::openh264_available().err(),
        ffmpeg_info: decoder::ffmpeg_info().ok(),
        openh264_version: decoder::openh264_version().ok(),
    }
}

/// Whether the virtual camera is there (problem 401 when not, 404 when its module is missing).
#[tauri::command(async)]
fn vcam_status() -> VcamStatus {
    vcam_setup::status()
}

/// Sets the virtual camera up as root (the desktop asks for the password), then starts the
/// receiver again: it could not start without the camera.
#[tauri::command(async)]
fn fix_vcam(app: AppHandle) -> Result<(), FixError> {
    // While developing (debug build, nothing installed): the script of the source tree. Windows:
    // the DLL next to the program, installed or not.
    let dev = (cfg!(debug_assertions) && cfg!(target_os = "linux"))
        .then(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("linux/vcam-setup.sh"));
    let helper = vcam_setup::helper_path(dev).ok_or_else(|| {
        FixError::Failed(format!(
            "{} is missing: reinstall Oculibre",
            vcam_setup::HELPER
        ))
    })?;
    vcam_setup::fix(&helper)?;
    restart(&app);
    Ok(())
}

/// Stops the receiver: the phone being served (if any) is let go, the port is closed.
#[tauri::command(async)]
fn stop(receiver: State<Receiver>) {
    stop_host(&receiver);
}

/// This computer's settings.
#[tauri::command]
fn settings(receiver: State<Receiver>) -> AppSettings {
    receiver.settings.lock().unwrap().clone()
}

/// When changed settings take effect.
#[derive(Serialize)]
enum Applied {
    /// Now.
    Now,
    /// When the phone streaming now leaves (Wi-Fi, shape of the virtual camera).
    AfterThisPhone,
}

/// Saves the settings and applies them: the decoder from the next stream; Wi-Fi and the shape of
/// the virtual camera with a new receiver, at once or, while a phone streams, after it — unless
/// `now` (turning Wi-Fi on to pair the phone streaming: its video stops, it must start again).
#[tauri::command(async)]
fn set_settings(
    app: AppHandle,
    receiver: State<Receiver>,
    settings: AppSettings,
    now: Option<bool>,
) -> Result<Applied, String> {
    let before = std::mem::replace(&mut *receiver.settings.lock().unwrap(), settings.clone());
    settings.save()?;
    decoder::set_choice(Choice::parse(&settings.decoder).unwrap_or(Choice::Auto));
    if before.language != settings.language {
        if let Some(menu) = receiver.tray.lock().unwrap().as_mut() {
            menu.set_language(settings.lang());
        }
    }
    if !before.needs_restart(&settings) || receiver.host.lock().unwrap().is_none() {
        return Ok(Applied::Now);
    }
    if running(&receiver)?.remote().is_some() && now != Some(true) {
        receiver.restart_pending.store(true, Ordering::Relaxed);
        return Ok(Applied::AfterThisPhone);
    }
    restart(&app);
    Ok(Applied::Now)
}

/// The receiver, if running; the lock is only held to copy the handle.
fn running(receiver: &Receiver) -> Result<Arc<Host>, String> {
    receiver
        .host
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "the receiver is not running".into())
}

/// The phone being served, if any. The lock is only held to copy the handle: a slow command to
/// the phone never blocks the other commands.
fn remote(receiver: &Receiver) -> Result<Remote, String> {
    let slot = receiver.host.lock().unwrap();
    let host = slot.as_ref().ok_or("the receiver is not running")?;
    host.remote().ok_or_else(|| "no phone is connected".into())
}

/// Whether Oculibre starts with the computer (hidden, in the tray).
#[tauri::command]
fn autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let launcher = app.autolaunch();
    if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    }
    .map_err(|e| e.to_string())
}

/// Opens the licences of everything the app ships (generated at build time by
/// `app/scripts/licences.mjs`) with the computer's browser. Installed: next to the program on
/// Windows, in the package's documentation folder on Linux. Development builds also look in
/// `desktop/target/licences/`, after `npm run licences`.
#[tauri::command]
fn open_licences() -> Result<(), String> {
    const NAME: &str = "THIRD-PARTY-LICENSES.html";
    let mut places = Vec::new();
    if cfg!(windows) {
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.join(NAME)))
        {
            places.push(dir);
        }
    } else {
        places.push(std::path::PathBuf::from("/usr/share/doc/oculibre").join(NAME));
    }
    if cfg!(debug_assertions) {
        places.push(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/licences")
                .join(NAME),
        );
    }
    let file = places
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| format!("{NAME} not found (generated when the app is packaged)"))?;
    let opener = if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(&file)
        .spawn()
        .map(drop)
        .map_err(|e| format!("{opener}: {e}"))
}

/// The cameras of the phone being served and their modes (size, frame rate, capture path).
#[tauri::command(async)]
fn capabilities(receiver: State<Receiver>) -> Result<Capabilities, String> {
    remote(&receiver)?.capabilities()
}

/// Switches the phone to another camera or quality; resolves once it streams in it, and the
/// choice is remembered for this phone. A refused mode (`{kind: "Refused"}`) leaves the phone in
/// its previous mode.
#[tauri::command(async)]
fn set_mode(receiver: State<Receiver>, mode: SetMode) -> Result<(), ModeError> {
    running(&receiver)
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

/// Every phone this computer knows: paired for Wi-Fi, or seen over USB.
#[tauri::command]
fn known_phones(receiver: State<Receiver>) -> Vec<KnownPhone> {
    match running(&receiver) {
        Ok(host) => host.known_phones(),
        // Not running (it could not start): the paired ones at least.
        Err(_) => Pairings::load(host::paired_phones_file())
            .phones()
            .into_iter()
            .map(|p| KnownPhone {
                id: p.id,
                name: p.name,
                fingerprint: Some(p.fingerprint),
                paired_on: Some(p.paired_on),
                last_seen: None,
                last_via: None,
            })
            .collect(),
    }
}

/// Forgets a phone: its settings, and its Wi-Fi pairing if it has one (then, streaming over
/// Wi-Fi now, it is disconnected). The page hears `phones`.
#[tauri::command(async)]
fn forget_phone(
    app: AppHandle,
    receiver: State<Receiver>,
    id: String,
    fingerprint: Option<String>,
) -> Result<(), String> {
    match running(&receiver) {
        Ok(host) => host.forget_phone(&id, fingerprint.as_deref())?,
        Err(_) => {
            if let Some(fingerprint) = &fingerprint {
                Pairings::load(host::paired_phones_file()).forget(fingerprint)?;
            }
        }
    }
    let _ = app.emit("phones", ());
    Ok(())
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
    let offer = running(&receiver)?.pairing_offer()?;
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
    *receiver.viewer.lock().unwrap() = Some(viewer);
    receiver.apply_preview();
}

/// Off, light (about 10 pictures a second) or full (every picture).
#[tauri::command]
fn set_preview_mode(receiver: State<Receiver>, mode: PreviewMode) {
    *receiver.preview.lock().unwrap() = mode;
    receiver.apply_preview();
}

/// Rotation and mirror, for the phone being served (remembered for it) and the next ones.
#[tauri::command]
fn set_view(receiver: State<Receiver>, view: View) -> Result<(), String> {
    running(&receiver)?.set_view(view);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Before any thread exists: it may set environment variables (VA-API drivers).
    oculibre_core::decoder::prepare_environment();
    let receiver = Receiver {
        settings: Mutex::new(AppSettings::load()),
        ..Receiver::default()
    };
    tauri::Builder::default()
        // First plugin, as its documentation asks: a second launch (menu, autostart while the
        // window is in the tray) shows the running Oculibre's window and exits.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_window(app);
        }))
        // Started with the computer: `--hidden`, straight to the tray (minimised without one).
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .manage(receiver)
        .setup(|app| {
            let handle = app.handle().clone();
            let lang = app.state::<Receiver>().settings.lock().unwrap().lang();
            *app.state::<Receiver>().tray.lock().unwrap() = Some(tray::build(&handle, lang)?);
            if std::env::args().any(|a| a == "--hidden") {
                if let Some(window) = app.get_webview_window("main") {
                    tray::put_away(&window);
                }
            }
            // The receiver starts now, without waiting for the page (the window may be hidden).
            std::thread::spawn(move || {
                let result = start_host(&handle, &handle.state::<Receiver>());
                let _ = handle.emit("receiver", &result);
            });
            Ok(())
        })
        // Closing the window keeps Oculibre running (the setting), so the camera stays: in the
        // tray, or minimised on a desktop without one.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.state::<Receiver>().settings.lock().unwrap().tray {
                    api.prevent_close();
                    if let Some(window) = window.app_handle().get_webview_window(window.label()) {
                        tray::put_away(&window);
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            start,
            stop,
            set_view,
            capabilities,
            set_mode,
            finish,
            known_phones,
            pairing_offer,
            preview,
            set_preview_mode,
            settings,
            set_settings,
            autostart,
            set_autostart,
            forget_phone,
            vcam_status,
            fix_vcam,
            decoders,
            open_licences
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
