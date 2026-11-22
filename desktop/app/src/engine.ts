// The engine (oculibre-core), seen from the page: its events and the commands of
// src-tauri/src/lib.rs.
//
// The types below describe the JSON that Rust sends (serde, see core/src/events.rs and its test
// `events_as_json`). Rust and TypeScript don't check each other: when an event changes in Rust,
// change it here too.

import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { locale, t } from "./i18n";

// --- what the events carry

/** A Rust `Result`: `{ Ok: value }` or `{ Err: "why" }`. */
export type Result<T> = { Ok: T } | { Err: string };

export type Rotate = { kind: "Auto" } | { kind: "Fixed"; degrees: 0 | 90 | 180 | 270 };

/** Rotation and mirror chosen by the user. */
export interface View {
  rotate: Rotate;
  mirror: boolean;
}

/** Rotation and mirror actually applied to the pictures. */
export interface Transform {
  rotation: number;
  mirror: boolean;
}

export type Via = { kind: "Usb" } | { kind: "Wifi"; tls: string };
export type Refusal = { kind: "WifiOff" } | { kind: "Tls"; data: string };
export type End =
  | { kind: "Bye"; reason: string; message: string | null }
  | { kind: "Closed" }
  | { kind: "Finished" };

export interface WifiInfo {
  addresses: string[];
  paired: number;
  key: string;
  announced: boolean;
}

/** The phone's HELLO (protocol §7.1); only the fields the app uses. */
export interface Hello {
  id: string;
  app: string;
  app_version: string;
  name: string;
  device?: { manufacturer?: string; model?: string; os?: string; os_version?: string };
  features: string[];
}

/** A new video stream (protocol §8.1); only the fields the app uses. */
export interface StreamConfig {
  width: number;
  height: number;
  fps: number;
  path?: string;
  camera?: { id: string; facing: string; sensor_orientation: number };
}

/** One way a camera can stream (protocol §9.2). `path`: "normal", "unlisted" or "high_speed". */
export interface ModeCapabilities {
  width: number;
  height: number;
  fps: number;
  path?: string;
}

export interface CameraCapabilities {
  id: string;
  /** "back", "front" or "external". */
  facing: string;
  sensor_orientation: number;
  /** Focal length in 35 mm equivalent (apps 0.3.1 and newer). */
  equivalent_focal_mm?: number;
  modes: ModeCapabilities[];
}

export interface Capabilities {
  cameras: CameraCapabilities[];
}

/** What `setMode` asks the phone: a camera and one of its modes. */
export interface SetMode {
  camera: string;
  width: number;
  height: number;
  fps: number;
  path?: string;
}

/** Why `setMode` failed: refused (the phone keeps its previous mode) or the session is over. */
export type ModeError = { kind: "Refused"; data: string } | { kind: "Session"; data: string };

/** A phone this computer knows: paired for Wi-Fi, or seen over USB. */
export interface KnownPhone {
  /** Install id: the same as in its HELLO. */
  id: string;
  name: string;
  /** Paired for Wi-Fi: its key fingerprint and the pairing date. */
  fingerprint: string | null;
  paired_on: string | null;
  /** Last visit: UTC time ("2026-11-12T10:53Z"; older entries: the date alone) and way. */
  last_seen: string | null;
  last_via: "usb" | "wifi" | null;
}

export interface PhoneSplit {
  camera: number;
  encoder: number;
  queue: number;
}

/** The last second (latencies in ms, null until measured). */
export interface Status {
  shown_fps: number;
  camera_fps: number | null;
  mbits: number;
  latency: number | null;
  latency_max: number | null;
  latency_avg: number | null;
  phone: number | null;
  split: PhoneSplit | null;
  pc: number | null;
  /** The decoding part of `pc`; the rest is rotation, conversion and write. */
  pc_decode: number | null;
  rtt: number | null;
  dropped: number | null;
  /** Pictures this computer could not write in time (a newer one took their place), since the start. */
  skipped: number;
  /** This computer does not keep up (problem 504). */
  behind: boolean;
  /** The connection does not carry the video: bitrate lowered or late video dropped (problem 501). */
  link_slow: boolean;
  /** What the phone aims at while it has lowered its bitrate for the network, Mbit/s. */
  target_mbits: number | null;
  errors: number;
  clock_mismatch: boolean;
}

export interface Summary {
  seconds: number;
  fps: number;
  latency_avg: number | null;
  latency_max: number | null;
  dropped: number | null;
  errors: number;
  phone: PhoneSplit | null;
  pc_decode: number | null;
  pc_output: number | null;
}

/** Everything the engine reports: look at `kind`, then `data` has the matching type. */
export type EngineEvent =
  | { kind: "VirtualCamera"; data: { path: string } }
  | { kind: "Usb"; data: Result<null> }
  | { kind: "Wifi"; data: WifiInfo }
  | { kind: "AnnounceFailed"; data: string }
  | { kind: "PhoneConnected"; data: { peer: string; via: Via } }
  | { kind: "PhoneRefused"; data: { peer: string; reason: Refusal } }
  | { kind: "AcceptFailed"; data: string }
  | { kind: "Paired"; data: { name: string } }
  | { kind: "SessionEnded"; data: Result<End> }
  | { kind: "Phone"; data: Hello }
  | { kind: "Stream"; data: StreamConfig }
  | { kind: "StreamPaused"; data: { reason: string } }
  | { kind: "Transform"; data: { transform: Transform; auto: boolean } }
  | { kind: "Decoder"; data: { name: string } }
  | { kind: "VirtualCameraSize"; data: { wanted: [number, number]; size: [number, number] } }
  | { kind: "RequestRefused"; data: { id: number; code: string; message: string | null } }
  | { kind: "Status"; data: Status }
  | { kind: "Summary"; data: Summary };

/**
 * Whether the page runs in the Tauri window. In a plain browser (http://localhost:1420 while
 * `tauri dev` runs) there is no Rust behind the page: it only shows the layout.
 */
export const inApp: boolean = isTauri();

/** Windows (#17): its own decoder (Media Foundation), its own virtual camera (a DLL). */
export const onWindows: boolean = typeof navigator !== "undefined" && /Windows/.test(navigator.userAgent);

// --- Rust → page

/** Calls `handler` for every engine event; returns the function that stops listening. */
export function onEngineEvent(handler: (event: EngineEvent) => void): Promise<UnlistenFn> {
  if (!inApp) {
    return Promise.resolve(() => {}); // nothing to listen to in a browser
  }
  return listen<EngineEvent>("engine", (e) => handler(e.payload));
}

// --- page → Rust (the #[tauri::command] functions; a Rust Err becomes a rejected promise)

/** Starts waiting for phones (with the saved settings); resolves with the address they use. */
export function start(): Promise<string> {
  return invoke<string>("start");
}

/** The receiver restarted after a change of settings: `{Ok: address}` or `{Err: why}`. */
export function onReceiver(handler: (result: Result<string>) => void): Promise<UnlistenFn> {
  if (!inApp) return Promise.resolve(() => {});
  return listen<Result<string>>("receiver", (e) => handler(e.payload));
}

/** The paired phones changed (a phone was forgotten). */
export function onPhonesChanged(handler: () => void): Promise<UnlistenFn> {
  if (!inApp) return Promise.resolve(() => {});
  return listen("phones", () => handler());
}

// --- settings of this computer

export type DecoderChoice = "auto" | "vaapi" | "nvdec" | "software" | "openh264";

export interface AppSettings {
  /** Accept paired phones over the network (encrypted). */
  wifi: boolean;
  /** Portrait virtual camera (landscape like every webcam otherwise). */
  portrait: boolean;
  decoder: DecoderChoice;
  /** Closing the window keeps Oculibre in the system tray. */
  tray: boolean;
  /** "fr", "en", or "" for the system's language (the window and the tray menu). */
  language: string;
}

/** When changed settings take effect: now, or when the phone streaming now leaves. */
export type Applied = "Now" | "AfterThisPhone";

export function settings(): Promise<AppSettings> {
  return inApp
    ? invoke<AppSettings>("settings")
    : Promise.resolve({ wifi: true, portrait: false, decoder: "auto", tray: true, language: "" });
}

/**
 * Saves and applies the settings. `now`: restart the receiver at once even while a phone streams
 * (turning Wi-Fi on to pair it): its video stops and it must be started again.
 */
export function setSettings(settings: AppSettings, now = false): Promise<Applied> {
  return invoke<Applied>("set_settings", { settings, now });
}

/** Whether Oculibre starts with the computer (hidden, in the system tray). */
export function autostart(): Promise<boolean> {
  return inApp ? invoke<boolean>("autostart") : Promise.resolve(false);
}

export function setAutostart(enabled: boolean): Promise<void> {
  return invoke("set_autostart", { enabled });
}

/** The tray menu asks for a screen ("pairing"). */
export function onNavigate(handler: (screen: string) => void): Promise<UnlistenFn> {
  if (!inApp) return Promise.resolve(() => {});
  return listen<string>("navigate", (e) => handler(e.payload));
}

/** Forgets a phone: its settings, and its Wi-Fi pairing if any (then cut if it streams over Wi-Fi). */
export function forgetPhone(phone: KnownPhone): Promise<void> {
  return invoke("forget_phone", { id: phone.id, fingerprint: phone.fingerprint });
}

export function stop(): Promise<void> {
  return invoke("stop");
}

export function setView(view: View): Promise<void> {
  return invoke("set_view", { view });
}

/** The cameras and modes of the phone being served. */
export function capabilities(): Promise<Capabilities> {
  return invoke<Capabilities>("capabilities");
}

/** Switches camera or quality; resolves once the phone streams in it, rejects with a ModeError. */
export function setMode(mode: SetMode): Promise<void> {
  return invoke("set_mode", { mode });
}

/** Stops the phone's camera; the receiver keeps waiting for phones. */
export function finish(): Promise<void> {
  return invoke("finish");
}

export function knownPhones(): Promise<KnownPhone[]> {
  return inApp ? invoke<KnownPhone[]>("known_phones") : Promise.resolve([]);
}

/** A one-time pairing offer: show `qr`; valid `ttl_seconds`, for one phone. */
export interface PairingOffer {
  uri: string;
  /** `width` × `width` modules, row after row, true = dark (no quiet zone). */
  qr: { width: number; dark: boolean[] };
  ttl_seconds: number;
  /** Also opened on the phone plugged in over USB. */
  sent_over_usb: boolean;
}

/** Opens a pairing offer (needs Wi-Fi on); a `Paired` event follows when a phone takes it. */
export function pairingOffer(): Promise<PairingOffer> {
  return invoke<PairingOffer>("pairing_offer");
}

/**
 * Calls `handler` with a small JPEG of the virtual camera's picture, about 10 times a second
 * while a phone streams. The bytes come through a Tauri channel, without any JSON.
 */
export function onPreview(handler: (jpeg: ArrayBuffer) => void): void {
  if (!inApp) return;
  const channel = new Channel<ArrayBuffer>();
  channel.onmessage = handler;
  invoke("preview", { onFrame: channel });
}

/** Off: nothing computed. Light: ~10 pictures a second (never slows the source). Full: every one. */
export type PreviewMode = "Off" | "Light" | "Full";

export function setPreviewMode(mode: PreviewMode): Promise<void> {
  return inApp ? invoke("set_preview_mode", { mode }) : Promise.resolve();
}

/** "12 nov. à 11:53" / "12 Nov at 11:53" (local time) from "2026-11-12T10:53Z"; the day alone from a date. */
export function when(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  const day = date.toLocaleDateString(locale(), { day: "numeric", month: "short" });
  if (!iso.includes("T")) return day;
  return t("at_time", { day, time: date.toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" }) });
}

/** A phone's name for people: its own name, else its model. */
export function phoneName(hello: Hello): string {
  return hello.name || hello.device?.model || t("phone");
}

// --- the virtual camera (Linux, #18): core vcam_setup.rs

/** What the virtual camera looks like now. */
export type VcamStatus =
  | { kind: "Ready"; data: { device: string } }
  | { kind: "Other"; data: { device: string } }
  | { kind: "NotLoaded" }
  | { kind: "NotInstalled" }
  | { kind: "SecureBoot" };

/** Why setting it up did not work ("Cancelled": the password window was closed). */
export type FixError =
  | { kind: "Cancelled" }
  | { kind: "NotInstalled" }
  | { kind: "SecureBoot" }
  | { kind: "InUse" }
  | { kind: "Failed"; data: string };

export function vcamStatus(): Promise<VcamStatus> {
  return inApp ? invoke<VcamStatus>("vcam_status") : Promise.resolve({ kind: "NotLoaded" });
}

/** Asks for the password (polkit), sets the camera up, restarts the receiver. */
export function fixVcam(): Promise<void> {
  return invoke<void>("fix_vcam");
}

/**
 * Why each decoding library is missing (null: it is there, e.g. "libavcodec.so.62 is not
 * installed"). Without FFmpeg OpenH264 decodes (problem 502); without either, nothing does (503).
 */
export interface Decoders {
  ffmpeg: string | null;
  openh264: string | null;
  /** FFmpeg as found: version, libavcodec version, licence ("GPL version 2 or later"…). */
  ffmpeg_info: { version: string; libavcodec: string; license: string } | null;
  /** "2.6.0" */
  openh264_version: string | null;
}

export function decoders(): Promise<Decoders> {
  return inApp
    ? invoke<Decoders>("decoders")
    : Promise.resolve({ ffmpeg: null, openh264: null, ffmpeg_info: null, openh264_version: null });
}

/** Opens the licences of every component the app ships, in the computer's browser. */
export function openLicences(): Promise<void> {
  return invoke<void>("open_licences");
}
