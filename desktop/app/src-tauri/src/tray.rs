//! The icon in the system tray and its menu (mockup design/mockups/Tray): what the phone does,
//! stop its camera, open the window, add a phone, quit. Oculibre keeps running there when the
//! window is closed, so the virtual camera stays available to the apps.

use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::Receiver;

/// The menu lines that change with the phone.
pub struct TrayMenu {
    status: MenuItem<Wry>,
    stop: MenuItem<Wry>,
}

impl TrayMenu {
    /// "Redmi Note 9 Pro · en direct (USB)" and "Stop the camera" enabled, or waiting.
    pub fn show_phone(&self, phone: Option<(&str, &str)>) {
        let (text, live) = match phone {
            Some((name, via)) => (format!("{name} · en direct ({via})"), true),
            None => ("En attente d'un téléphone".to_string(), false),
        };
        let _ = self.status.set_text(text);
        let _ = self.stop.set_enabled(live);
    }
}

/// Builds the icon and its menu; the lines that change are kept in [`Receiver`].
pub fn build(app: &AppHandle) -> tauri::Result<TrayMenu> {
    let status = MenuItem::with_id(
        app,
        "status",
        "En attente d'un téléphone",
        false,
        None::<&str>,
    )?;
    let stop = MenuItem::with_id(app, "stop", "Arrêter la caméra", false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Ouvrir Oculibre", true, None::<&str>)?;
    let add = MenuItem::with_id(app, "add", "Ajouter un téléphone…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quitter Oculibre", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &status,
            &PredefinedMenuItem::separator(app)?,
            &stop,
            &PredefinedMenuItem::separator(app)?,
            &open,
            &add,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Oculibre")
        .menu(&menu)
        // Linux shows the menu on any click anyway; elsewhere, a left click opens it too.
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu);
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(TrayMenu { status, stop })
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    match event.id.as_ref() {
        "open" => show_window(app),
        "add" => {
            show_window(app);
            // The page opens its "add a phone" screen.
            let _ = app.emit("navigate", "pairing");
        }
        "stop" => {
            if let Ok(remote) = crate::remote(&app.state::<Receiver>()) {
                remote.finish("stopped from the computer");
            }
        }
        "quit" => {
            crate::stop_host(&app.state::<Receiver>());
            app.exit(0);
        }
        _ => {}
    }
}

/// Brings the window back: shown, not minimised, in front.
pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
