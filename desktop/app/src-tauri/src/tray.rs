//! The icon in the system tray and its menu (mockup design/mockups/Tray): what the phone does,
//! stop its camera, open the window, add a phone, quit. Oculibre keeps running there when the
//! window is closed, so the virtual camera stays available to the apps.

use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::settings::Lang;
use crate::Receiver;

/// The menu's texts in one language.
struct Texts {
    waiting: &'static str,
    /// "{name} · live ({via})"
    live: &'static str,
    stop: &'static str,
    open: &'static str,
    add: &'static str,
    quit: &'static str,
}

fn texts(lang: Lang) -> Texts {
    match lang {
        Lang::En => Texts {
            waiting: "Waiting for a phone",
            live: "{name} · live ({via})",
            stop: "Stop the camera",
            open: "Open Oculibre",
            add: "Add a phone…",
            quit: "Quit Oculibre",
        },
        Lang::Fr => Texts {
            waiting: "En attente d'un téléphone",
            live: "{name} · en direct ({via})",
            stop: "Arrêter la caméra",
            open: "Ouvrir Oculibre",
            add: "Ajouter un téléphone…",
            quit: "Quitter Oculibre",
        },
    }
}

/// The menu lines: kept to follow the phone and the language.
pub struct TrayMenu {
    status: MenuItem<Wry>,
    stop: MenuItem<Wry>,
    open: MenuItem<Wry>,
    add: MenuItem<Wry>,
    quit: MenuItem<Wry>,
    lang: Lang,
    /// The phone streaming now (name, "USB" or "Wi-Fi"), to write it again in another language.
    phone: Option<(String, String)>,
}

impl TrayMenu {
    /// "Redmi Note 9 Pro · live (USB)" and "Stop the camera" enabled, or waiting.
    pub fn show_phone(&mut self, phone: Option<(&str, &str)>) {
        self.phone = phone.map(|(name, via)| (name.to_string(), via.to_string()));
        self.refresh_status();
    }

    /// Writes every line again in `lang` (chosen in the settings).
    pub fn set_language(&mut self, lang: Lang) {
        self.lang = lang;
        let t = texts(lang);
        let _ = self.stop.set_text(t.stop);
        let _ = self.open.set_text(t.open);
        let _ = self.add.set_text(t.add);
        let _ = self.quit.set_text(t.quit);
        self.refresh_status();
    }

    fn refresh_status(&self) {
        let t = texts(self.lang);
        let text = match &self.phone {
            Some((name, via)) => t.live.replace("{name}", name).replace("{via}", via),
            None => t.waiting.to_string(),
        };
        let _ = self.status.set_text(text);
        let _ = self.stop.set_enabled(self.phone.is_some());
    }
}

/// Builds the icon and its menu; the lines that change are kept in [`Receiver`].
pub fn build(app: &AppHandle, lang: Lang) -> tauri::Result<TrayMenu> {
    let t = texts(lang);
    let status = MenuItem::with_id(app, "status", t.waiting, false, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", t.stop, false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", t.open, true, None::<&str>)?;
    let add = MenuItem::with_id(app, "add", t.add, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", t.quit, true, None::<&str>)?;
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
    Ok(TrayMenu {
        status,
        stop,
        open,
        add,
        quit,
        lang,
        phone: None,
    })
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

/// Whether the desktop shows tray icons: someone owns the StatusNotifierWatcher name on the
/// session bus (KDE, Ubuntu's GNOME with its AppIndicator extension, most other desktops). Plain
/// GNOME, as on Debian, has none: a hidden window could then not be found again. Asked each time
/// (the extension can be turned on while Oculibre runs); when D-Bus cannot tell, assume yes.
#[cfg(target_os = "linux")]
pub fn shown() -> bool {
    let ask = || -> zbus::Result<bool> {
        let bus = zbus::blocking::Connection::session()?;
        let dbus = zbus::blocking::fdo::DBusProxy::new(&bus)?;
        Ok(dbus.name_has_owner("org.kde.StatusNotifierWatcher".try_into()?)?)
    };
    ask().unwrap_or(true)
}

#[cfg(not(target_os = "linux"))]
pub fn shown() -> bool {
    true
}

/// Out of the way but still running: hidden in the tray, or minimised when there is no tray
/// (it stays in the dock and Alt+Tab).
pub fn put_away(window: &tauri::WebviewWindow) {
    let _ = if shown() {
        window.hide()
    } else {
        window.minimize()
    };
}

/// Brings the window back: shown, not minimised, in front.
pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
