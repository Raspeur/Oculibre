// The window's texts in English and French, without a library: a dictionary per language, `t()`
// to read one, `useLang()` so a component draws again when the language changes.
//
// English is the reference: `fr` is typed `Record<Key, string>`, so TypeScript refuses to build
// when a French text is missing. `{name}` in a text is replaced by `t(key, { name })`.
// The language follows the system unless the settings choose one (AppSettings.language).

import { useSyncExternalStore } from "react";

export type Lang = "en" | "fr";

const en = {
  // App
  browser_title: "Preview in a browser",
  browser_text: "Only the layout shows here: to control the phone, open Oculibre's window (npm run tauri dev).",
  start_error_title: "Oculibre cannot wait for phones",
  code: "Code {code}",
  phone: "Phone",
  at_time: "{day} at {time}",

  // Sidebar
  phones: "Phones",
  live: "Live",
  connected: "Connected",
  offline: "Offline",
  no_phone_yet: "No phone yet.",
  not_waiting: "Oculibre is not waiting for phones yet",
  add_phone: "Add a phone",
  settings: "Settings",
  tour: "Guided tour",
  about: "About",

  // Stage
  waiting: "Waiting",
  waiting_title: "Waiting for a phone",
  waiting_text:
    "Open Oculibre on the phone and press “Start”. Over Wi-Fi, it must be paired with this computer and on the same network; over the USB cable, USB debugging must be on.",
  computer_code: "This computer's code: {key}",
  preview: "Preview",
  changing_mode: "Changing mode…",
  starting: "Starting…",
  wifi_encrypted: "Encrypted Wi-Fi",
  usb_cable: "USB cable",
  preview_off: "Preview off",
  preview_off_text: "Apps receive the picture as usual.",
  preview_alt: "Preview of what the apps receive",
  picture_coming: "The picture is coming…",
  preview_mode_off: "Off",
  preview_mode_off_hint: "No work for the preview",
  preview_mode_light: "Light",
  preview_mode_light_hint: "About 10 pictures a second: never slows the camera down",
  preview_mode_full: "Smooth",
  preview_mode_full_hint: "Every picture: when nothing else needs the computer",
  latency: "Latency",
  frames_per_second: "Frames/s",
  bitrate: "Bitrate",
  connection: "Connection",
  quality: "{size} · {fps} fps",

  // Panel
  camera_settings: "Camera settings",
  source: "Source",
  phone_camera: "Phone camera",
  reading_cameras: "Reading the cameras…",
  no_phone_connected: "No phone connected.",
  video: "Video",
  resolution: "Resolution",
  switching: "Changing…",
  mode_failed_title: "This mode did not work",
  mode_failed_text: "{mode}: {why}. The phone keeps the previous mode.",
  quality_at: "{size} at {fps} fps",
  picture: "Picture",
  rotation: "Rotation",
  automatic: "Automatic",
  mirror: "Mirror",
  output: "Output",
  name_in_apps: "Name in the apps",
  format_sent: "Format sent",
  decoding: "Decoding",
  stop_camera: "Stop the camera",
  camera_front: "Front",
  camera_back: "Back",
  camera_external: "External",

  // Pairing
  back: "Back",
  pair_ready: "{name} is ready",
  pair_ready_text: "Press “Start” on the phone. Next time, opening the app is enough.",
  done: "Done",
  usb_nothing: "USB cable: USB debugging needed",
  usb_nothing_text:
    "Turn on USB debugging on the phone (Settings › Developer options), plug it in, allow this computer on the phone, then press “Start”. USB debugging gives the computer wide access to the phone: only allow your own computer, never a public one, and turn it off when you do not use it. This is temporary: a future version of Oculibre will connect over USB without it. Wi-Fi does not need it.",
  wifi_once: "To use it over Wi-Fi too, pair it once. Then it reconnects by itself.",
  sent_usb: "Also sent to the phone plugged in by USB: confirm on its screen.",
  usb_hint: "Phone plugged in by USB? The invitation is sent to it directly.",
  qr_label: "Pairing QR code",
  qr_alt: "QR code to scan with the phone",
  wifi_off: "Wi-Fi is off",
  wifi_off_text: "Oculibre only accepts the cable for now. Turn Wi-Fi on to pair this phone.",
  wifi_off_live: "The live phone's video will stop: then press “Start” again on the phone.",
  turning_on: "Turning on…",
  turn_wifi_on: "Turn Wi-Fi on",
  qr_failed: "Cannot create a QR code",
  retry: "Try again",
  qr_expired: "This QR code has expired",
  qr_expired_text: "For your safety, a QR code only works for 10 minutes and for one phone.",
  new_qr: "New QR code",
  preparing: "Preparing…",
  this_computer_code: "This computer's code",
  valid_for: "Valid for {time} more · for one phone only",
  pair_step1_title: "Install Oculibre on the phone",
  pair_step1_text: "Android 5 or newer.",
  pair_step2_title: "Scan the QR code with the phone",
  pair_step2_text:
    "In the Oculibre app (“Scan the QR code”), or with the camera app. The phone and the computer must be on the same Wi-Fi.",
  pair_step3_title: "Check the code and confirm on the phone",
  pair_step3_text:
    "The phone shows a code: it must be the same as the one under the QR code. That is what makes sure nobody slipped in between.",

  // Tour
  tour1_title: "Connect your phone",
  tour1_text:
    "Over Wi-Fi, pair it here once, with a QR code. Over the USB cable, turn on USB debugging on the phone, plug it in and press “Start” in the phone's app.",
  tour2_title: "Your phones",
  tour2_text: "Green dot: live. Grey circle: stopped or too far. Oculibre remembers each one's settings.",
  tour3_title: "Start from the phone",
  tour3_text: "The phone films when you press “Start” in its app. Here you see what the apps receive.",
  tour4_title: "Camera, quality and picture",
  tour4_text:
    "Back or front, resolution, frames per second, rotation, mirror: everything is set here, without touching the phone. Oculibre remembers it for this phone.",
  tour5_title: "In your apps",
  tour5_text: "In Zoom, Teams, Discord or OBS, choose the “Oculibre” camera. Closing this window keeps it available.",
  tour_welcome_title: "Welcome to Oculibre",
  tour_welcome_text: "Your phone becomes this computer's webcam. A one-minute tour to know where to click?",
  tour_end_title: "That's all!",
  tour_end_text: "First thing to do: plug your phone in, or pair it over Wi-Fi. The tour stays available in the left bar.",
  tour_step: "Step {n} of {total}",
  tour_skip: "Skip the tour",
  tour_start: "Start the tour",
  later: "Later",
  close: "Close",
  next: "Next",

  // Settings
  applied_later: "Saved: it will apply when the live phone disconnects.",
  not_saved: "Setting not saved",
  general: "General",
  language: "Language",
  language_system: "System language",
  autostart: "Start with the computer",
  autostart_hint: "Oculibre waits for your phones as soon as you log in, in the taskbar.",
  stay_in_tray: "Stay in the taskbar",
  stay_in_tray_hint: "Closing the window does not quit Oculibre: the camera stays available.",
  virtual_camera: "Virtual camera",
  status: "Status",
  installed: "Installed",
  unavailable: "Unavailable",
  shape: "Picture shape",
  shape_hint: "Apps already open keep the shape they saw.",
  landscape: "Landscape (like a webcam)",
  portrait: "Portrait",
  phones_wifi: "Phones and Wi-Fi",
  accept_wifi: "Accept phones over Wi-Fi",
  accept_wifi_hint: "Only paired phones, encrypted connection.",
  known_phones: "Known phones",
  none: "None",
  computer_code_hint: "The phone shows the same one when you pair it.",
  performance: "Performance",
  video_decoding: "Video decoding",
  decoder_used: "In use: {name}. ",
  decoder_next: "A change applies to the next stream.",
  decoder_gpu: "Graphics card (AMD, Intel)",
  decoder_nvidia: "Graphics card (NVIDIA)",
  decoder_cpu: "Processor",
  decoder_openh264: "Processor (OpenH264)",
  decoder_missing: "(not installed)",
  about_text: "Free and open source, MIT licence. No data leaves your network.",
  licences: "Component licences",
  paired_wifi: "Paired over Wi-Fi",
  paired_wifi_on: "Paired over Wi-Fi on {date}",
  seen_on: " · seen on {date}",
  forget_warning_wifi: "Its settings will be erased, and it will need a new pairing for Wi-Fi.",
  forget_warning_usb: "Its settings will be erased. With the cable, it keeps working.",
  cancel: "Cancel",
  forget: "Forget",
  forget_ask: "Forget…",
  notice_ffmpeg: "libavcodec {libavcodec}: your system's library.",
  notice_openh264: "© Cisco Systems. Your system's library.",
  notice_not_installed: "Not installed on this computer.",
  notice_jpeg: "This software is based in part on the work of the Independent JPEG Group.",
  notice_fonts: "Fonts included in the app.",

  // Virtual camera (problems 401, 403, 404, 405)
  vcam_missing_title: "The virtual camera is not set up",
  vcam_missing_text: "Without it, Zoom, Teams or OBS cannot see Oculibre. Setting it up asks for the computer's password.",
  vcam_install: "Set up the virtual camera",
  vcam_installing: "Setting up…",
  vcam_module_title: "A package is missing for the virtual camera",
  vcam_module_text:
    "The v4l2loopback module is not installed for this Linux kernel. Install the package linux-modules-v4l2loopback-generic (Ubuntu), or v4l2loopback-dkms with the kernel headers (linux-headers-amd64 on Debian), then try again.",
  vcam_secureboot_title: "Secure Boot blocks the virtual camera",
  vcam_secureboot_text:
    "Its module was built on this computer and its key is not accepted yet. In a terminal: sudo update-secureboot-policy --enroll-key (Ubuntu) or sudo mokutil --import /var/lib/dkms/mok.pub (Debian), choose a password, restart, choose \u201cEnroll MOK\u201d on the blue screen and type that password.",
  vcam_inuse_title: "Another virtual camera is in the way",
  vcam_inuse_text: "Another app already created a virtual camera with other settings. Restart the computer: the Oculibre camera will then be ready.",
  vcam_failed_title: "The virtual camera could not be set up",
  vcam_other: "Another app's camera ({device})",
  no_decoder_title: "No video decoder on this computer",
  no_decoder_text:
    "Oculibre cannot show the phone's video without FFmpeg or OpenH264. Install the package libavcodec (recommended) or libopenh264, then restart Oculibre.",
  ffmpeg_missing_title: "Decoding by the processor",
  ffmpeg_missing_text:
    "FFmpeg is not installed on this computer: Oculibre decodes the video with OpenH264, slower in high resolution. For decoding by the graphics card, install the package libavcodec.",
  // Windows (#17)
  decoder_windows: "Processor (Windows)",
  decoder_gpu_windows: "Graphics card",
  notice_media_foundation: "Windows' own decoder.",
  notice_adb: "© The Android Open Source Project, Google's build. Licences and sources: the adb folder of Oculibre.",
  no_decoder_text_windows:
    "Windows has no H.264 decoder: on an \u201cN\u201d edition of Windows, install the Media Feature Pack (Settings \u203a Apps \u203a Optional features), then restart Oculibre.",
  mf_missing_text:
    "Windows' video decoder is missing (an \u201cN\u201d edition of Windows?): Oculibre decodes with OpenH264, slower in high resolution. Install the Media Feature Pack for decoding by the graphics card.",
  vcam_missing_text_windows: "Without it, Zoom, Teams or OBS cannot see Oculibre. Setting it up asks Windows for permission.",
  vcam_module_text_windows: "The camera's file is missing from Oculibre's folder. Install Oculibre again.",
};

export type Key = keyof typeof en;

const fr: Record<Key, string> = {
  browser_title: "Aperçu dans un navigateur",
  browser_text:
    "Seule la mise en page s'affiche ici : pour piloter le téléphone, ouvrez la fenêtre d'Oculibre (npm run tauri dev).",
  start_error_title: "Oculibre ne peut pas attendre les téléphones",
  code: "Code {code}",
  phone: "Téléphone",
  at_time: "{day} à {time}",

  phones: "Téléphones",
  live: "En direct",
  connected: "Connecté",
  offline: "Hors ligne",
  no_phone_yet: "Aucun téléphone pour l'instant.",
  not_waiting: "Oculibre n'attend pas encore les téléphones",
  add_phone: "Ajouter un téléphone",
  settings: "Réglages",
  tour: "Visite guidée",
  about: "À propos",

  waiting: "En attente",
  waiting_title: "En attente d'un téléphone",
  waiting_text:
    "Ouvrez Oculibre sur le téléphone et appuyez sur « Démarrer ». En Wi-Fi, il doit être relié à cet ordinateur et sur le même réseau ; par câble USB, le débogage USB doit être activé.",
  computer_code: "Code de cet ordinateur : {key}",
  preview: "Aperçu",
  changing_mode: "Changement de mode…",
  starting: "Démarrage…",
  wifi_encrypted: "Wi-Fi chiffré",
  usb_cable: "Câble USB",
  preview_off: "Aperçu arrêté",
  preview_off_text: "Les applications reçoivent l'image normalement.",
  preview_alt: "Aperçu de ce que reçoivent les applications",
  picture_coming: "L'image arrive…",
  preview_mode_off: "Arrêté",
  preview_mode_off_hint: "Aucun calcul pour l'aperçu",
  preview_mode_light: "Léger",
  preview_mode_light_hint: "Environ 10 images/s : ne ralentit jamais la caméra",
  preview_mode_full: "Fluide",
  preview_mode_full_hint: "Toutes les images : quand rien d'autre ne demande l'ordinateur",
  latency: "Latence",
  frames_per_second: "Images/s",
  bitrate: "Débit",
  connection: "Connexion",
  quality: "{size} · {fps} i/s",

  camera_settings: "Réglages de la caméra",
  source: "Source",
  phone_camera: "Caméra du téléphone",
  reading_cameras: "Lecture des caméras…",
  no_phone_connected: "Aucun téléphone connecté.",
  video: "Vidéo",
  resolution: "Résolution",
  switching: "Changement en cours…",
  mode_failed_title: "Ce mode n'a pas fonctionné",
  mode_failed_text: "{mode} : {why}. Le téléphone garde le mode précédent.",
  quality_at: "{size} à {fps} i/s",
  picture: "Image",
  rotation: "Rotation",
  automatic: "Automatique",
  mirror: "Miroir",
  output: "Sortie",
  name_in_apps: "Nom dans les applications",
  format_sent: "Format envoyé",
  decoding: "Décodage",
  stop_camera: "Arrêter la caméra",
  camera_front: "Avant",
  camera_back: "Arrière",
  camera_external: "Externe",

  back: "Retour",
  pair_ready: "{name} est prêt",
  pair_ready_text: "Appuyez sur « Démarrer » sur le téléphone. La prochaine fois, il suffira d'ouvrir l'app.",
  done: "Terminer",
  usb_nothing: "Par câble USB : débogage USB nécessaire",
  usb_nothing_text:
    "Activez le débogage USB sur le téléphone (Paramètres › Options pour les développeurs), branchez-le, autorisez cet ordinateur sur le téléphone, puis appuyez sur « Démarrer ». Le débogage USB donne à l'ordinateur un large accès au téléphone : n'autorisez que votre propre ordinateur, jamais un ordinateur public, et désactivez-le quand vous ne l'utilisez pas. C'est temporaire : une prochaine version d'Oculibre se connectera par USB sans lui. Le Wi-Fi n'en a pas besoin.",
  wifi_once: "Pour l'utiliser aussi en Wi-Fi, reliez-le une seule fois. Ensuite, il se reconnecte tout seul.",
  sent_usb: "Envoyé aussi au téléphone branché en USB : confirmez sur son écran.",
  usb_hint: "Téléphone branché en USB ? L'invitation lui est envoyée directement.",
  qr_label: "QR code d'appairage",
  qr_alt: "QR code à scanner avec le téléphone",
  wifi_off: "Le Wi-Fi est désactivé",
  wifi_off_text: "Oculibre n'accepte pour l'instant que le câble. Activez le Wi-Fi pour relier ce téléphone.",
  wifi_off_live:
    "La vidéo du téléphone en direct va s'interrompre : appuyez ensuite à nouveau sur « Démarrer » sur le téléphone.",
  turning_on: "Activation…",
  turn_wifi_on: "Activer le Wi-Fi",
  qr_failed: "Impossible de créer un QR code",
  retry: "Réessayer",
  qr_expired: "Ce QR code a expiré",
  qr_expired_text: "Pour votre sécurité, un QR code ne sert que 10 minutes et pour un seul téléphone.",
  new_qr: "Nouveau QR code",
  preparing: "Préparation…",
  this_computer_code: "Code de cet ordinateur",
  valid_for: "Valable encore {time} · pour un seul téléphone",
  pair_step1_title: "Installez Oculibre sur le téléphone",
  pair_step1_text: "Android 5 ou plus récent.",
  pair_step2_title: "Scannez le QR code avec le téléphone",
  pair_step2_text:
    "Dans l'app Oculibre (« Scanner le QR code »), ou avec l'appareil photo. Le téléphone et l'ordinateur doivent être sur le même Wi-Fi.",
  pair_step3_title: "Vérifiez le code et confirmez sur le téléphone",
  pair_step3_text:
    "Le téléphone affiche un code : il doit être identique à celui sous le QR code. C'est ce qui garantit que personne d'autre ne s'est glissé entre les deux.",

  tour1_title: "Reliez votre téléphone",
  tour1_text:
    "En Wi-Fi, reliez-le une seule fois ici, avec un QR code. Par câble USB, activez le débogage USB sur le téléphone, branchez-le et appuyez sur « Démarrer » dans l'app du téléphone.",
  tour2_title: "Vos téléphones",
  tour2_text: "Point vert : en direct. Cercle gris : arrêté ou trop loin. Oculibre se souvient des réglages de chacun.",
  tour3_title: "Démarrer depuis le téléphone",
  tour3_text:
    "Le téléphone filme quand vous appuyez sur « Démarrer » dans son app. Ici, vous voyez ce que reçoivent les applications.",
  tour4_title: "Caméra, qualité et image",
  tour4_text:
    "Arrière ou avant, résolution, images par seconde, rotation, miroir : tout se règle ici, sans toucher au téléphone. Oculibre s'en souvient pour ce téléphone.",
  tour5_title: "Dans vos applications",
  tour5_text:
    "Dans Zoom, Teams, Discord ou OBS, choisissez la caméra « Oculibre ». Fermer cette fenêtre la garde disponible.",
  tour_welcome_title: "Bienvenue dans Oculibre",
  tour_welcome_text:
    "Votre téléphone devient la webcam de cet ordinateur. Une visite d'une minute pour savoir où cliquer ?",
  tour_end_title: "C'est tout !",
  tour_end_text:
    "Première chose à faire : brancher votre téléphone, ou le relier en Wi-Fi. La visite reste disponible dans la barre de gauche.",
  tour_step: "Étape {n} sur {total}",
  tour_skip: "Passer la visite",
  tour_start: "Commencer la visite",
  later: "Plus tard",
  close: "Fermer",
  next: "Suivant",

  applied_later: "Pris en compte : s'appliquera quand le téléphone en direct se déconnectera.",
  not_saved: "Réglage non enregistré",
  general: "Général",
  language: "Langue",
  language_system: "Langue du système",
  autostart: "Démarrer avec l'ordinateur",
  autostart_hint: "Oculibre attend vos téléphones dès l'ouverture de session, dans la barre des tâches.",
  stay_in_tray: "Rester dans la barre des tâches",
  stay_in_tray_hint: "Fermer la fenêtre ne quitte pas Oculibre : la caméra reste disponible.",
  virtual_camera: "Caméra virtuelle",
  status: "État",
  installed: "Installée",
  unavailable: "Indisponible",
  shape: "Forme de l'image",
  shape_hint: "Les applications ouvertes gardent la forme qu'elles ont vue.",
  landscape: "Paysage (comme une webcam)",
  portrait: "Portrait",
  phones_wifi: "Téléphones et Wi-Fi",
  accept_wifi: "Accepter les téléphones en Wi-Fi",
  accept_wifi_hint: "Seulement les téléphones reliés, connexion chiffrée.",
  known_phones: "Téléphones connus",
  none: "Aucun",
  computer_code_hint: "Le téléphone affiche le même au moment de le relier.",
  performance: "Performances",
  video_decoding: "Décodage de la vidéo",
  decoder_used: "Utilisé : {name}. ",
  decoder_next: "Un changement s'applique au prochain flux.",
  decoder_gpu: "Carte graphique (AMD, Intel)",
  decoder_nvidia: "Carte graphique (NVIDIA)",
  decoder_cpu: "Processeur",
  decoder_openh264: "Processeur (OpenH264)",
  decoder_missing: "(pas installé)",
  about_text: "Libre et gratuit, licence MIT. Aucune donnée ne quitte votre réseau.",
  licences: "Licences des composants",
  paired_wifi: "Relié en Wi-Fi",
  paired_wifi_on: "Relié en Wi-Fi le {date}",
  seen_on: " · vu le {date}",
  forget_warning_wifi: "Ses réglages seront effacés, et il faudra le relier à nouveau pour le Wi-Fi.",
  forget_warning_usb: "Ses réglages seront effacés. Par câble, il reste utilisable.",
  cancel: "Annuler",
  forget: "Oublier",
  forget_ask: "Oublier…",
  notice_ffmpeg: "libavcodec {libavcodec} : la bibliothèque de votre système.",
  notice_openh264: "© Cisco Systems. La bibliothèque de votre système.",
  notice_not_installed: "Pas installé sur cet ordinateur.",
  notice_jpeg: "Ce logiciel est basé en partie sur les travaux de l'Independent JPEG Group.",
  notice_fonts: "Polices incluses dans l'application.",

  vcam_missing_title: "La caméra virtuelle n'est pas installée",
  vcam_missing_text:
    "Sans elle, Zoom, Teams ou OBS ne voient pas Oculibre. L'installation demande le mot de passe de l'ordinateur.",
  vcam_install: "Installer la caméra virtuelle",
  vcam_installing: "Installation…",
  vcam_module_title: "Il manque un paquet pour la caméra virtuelle",
  vcam_module_text:
    "Le module v4l2loopback n'est pas installé pour ce noyau Linux. Installez le paquet linux-modules-v4l2loopback-generic (Ubuntu), ou v4l2loopback-dkms avec les en-têtes du noyau (linux-headers-amd64 sous Debian), puis réessayez.",
  vcam_secureboot_title: "Le démarrage sécurisé bloque la caméra virtuelle",
  vcam_secureboot_text:
    "Son module a été construit sur cet ordinateur et sa clé n'est pas encore acceptée. Dans un terminal : sudo update-secureboot-policy --enroll-key (Ubuntu) ou sudo mokutil --import /var/lib/dkms/mok.pub (Debian), choisissez un mot de passe, redémarrez, choisissez « Enroll MOK » sur l'écran bleu et tapez ce mot de passe.",
  vcam_inuse_title: "Une autre caméra virtuelle gêne",
  vcam_inuse_text:
    "Une autre application a déjà créé une caméra virtuelle avec d'autres réglages. Redémarrez l'ordinateur : la caméra Oculibre sera alors prête.",
  vcam_failed_title: "La caméra virtuelle n'a pas pu être installée",
  vcam_other: "Celle d'une autre application ({device})",
  no_decoder_title: "Aucun décodeur vidéo sur cet ordinateur",
  no_decoder_text:
    "Sans FFmpeg ni OpenH264, Oculibre ne peut pas afficher la vidéo du téléphone. Installez le paquet libavcodec (conseillé) ou libopenh264, puis relancez Oculibre.",
  ffmpeg_missing_title: "Décodage par le processeur",
  ffmpeg_missing_text:
    "FFmpeg n'est pas installé sur cet ordinateur : Oculibre décode la vidéo avec OpenH264, plus lent en haute résolution. Pour le décodage par la carte graphique, installez le paquet libavcodec.",
  decoder_windows: "Processeur (Windows)",
  decoder_gpu_windows: "Carte graphique",
  notice_media_foundation: "Le décodeur de Windows.",
  notice_adb: "© The Android Open Source Project, compilé par Google. Licences et sources : le dossier adb d'Oculibre.",
  no_decoder_text_windows:
    "Windows n'a pas de décodeur H.264 : sur une édition « N » de Windows, installez le Media Feature Pack (Paramètres › Applications › Fonctionnalités facultatives), puis relancez Oculibre.",
  mf_missing_text:
    "Le décodeur vidéo de Windows manque (une édition « N » de Windows ?) : Oculibre décode avec OpenH264, plus lent en haute résolution. Installez le Media Feature Pack pour le décodage par la carte graphique.",
  vcam_missing_text_windows: "Sans elle, Zoom, Teams ou OBS ne voient pas Oculibre. L'installer demande l'autorisation de Windows.",
  vcam_module_text_windows: "Le fichier de la caméra manque dans le dossier d'Oculibre. Réinstallez Oculibre.",
};

const texts: Record<Lang, Record<Key, string>> = { en, fr };

/** The language names, each in its own words (the settings list). */
export const LANGUAGES: [Lang, string][] = [
  ["fr", "Français"],
  ["en", "English"],
];

/** "" = the system's language (French if it is French, else English). */
export function resolve(choice: string): Lang {
  const tag = (choice || (typeof navigator !== "undefined" ? navigator.language : "") || "en").toLowerCase();
  return tag.startsWith("fr") ? "fr" : "en";
}

let current: Lang = resolve("");
const listeners = new Set<() => void>();
if (typeof document !== "undefined") document.documentElement.lang = current;

/** Applies the settings' choice ("" = system); every component using `useLang` draws again. */
export function setLanguage(choice: string) {
  const next = resolve(choice);
  if (next === current) return;
  current = next;
  document.documentElement.lang = next;
  listeners.forEach((listener) => listener());
}

/** The text of `key` in the current language, with `{name}` placeholders filled from `vars`. */
export function t(key: Key, vars?: Record<string, string | number>): string {
  let text = texts[current][key];
  if (vars) {
    for (const [name, value] of Object.entries(vars)) text = text.split(`{${name}}`).join(String(value));
  }
  return text;
}

/** For dates and numbers: "fr-FR" or "en-GB". */
export function locale(): string {
  return current === "fr" ? "fr-FR" : "en-GB";
}

/** The current language; the component draws again when it changes. */
export function useLang(): Lang {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => current,
  );
}
