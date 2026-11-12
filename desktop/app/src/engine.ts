// The engine (oculibre-core), seen from the page: its events and the commands of
// src-tauri/src/lib.rs.
//
// The types below describe the JSON that Rust sends (serde, see core/src/events.rs and its test
// `events_as_json`). Rust and TypeScript don't check each other: when an event changes in Rust,
// change it here too.

import { Channel, invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

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

/** A phone paired for Wi-Fi, as stored on this computer. */
export interface PairedPhone {
  fingerprint: string;
  name: string;
  /** Install id: the same as in its HELLO. */
  id: string;
  paired_on: string;
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
  rtt: number | null;
  dropped: number | null;
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
}

/** When changed settings take effect: now, or when the phone streaming now leaves. */
export type Applied = "Now" | "AfterThisPhone";

export function settings(): Promise<AppSettings> {
  return inApp
    ? invoke<AppSettings>("settings")
    : Promise.resolve({ wifi: true, portrait: false, decoder: "auto" });
}

export function setSettings(settings: AppSettings): Promise<Applied> {
  return invoke<Applied>("set_settings", { settings });
}

/** Forgets a paired phone; if it streams over Wi-Fi now, it is disconnected. */
export function forgetPhone(fingerprint: string): Promise<void> {
  return invoke("forget_phone", { fingerprint });
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

export function pairedPhones(): Promise<PairedPhone[]> {
  return inApp ? invoke<PairedPhone[]>("paired_phones") : Promise.resolve([]);
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

/** A phone's name for people: its own name, else its model. */
export function phoneName(hello: Hello): string {
  return hello.name || hello.device?.model || "Phone";
}
