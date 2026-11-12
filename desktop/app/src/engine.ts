// The engine (oculibre-core), seen from the page: its events and the commands of
// src-tauri/src/lib.rs.
//
// The types below describe the JSON that Rust sends (serde, see core/src/events.rs and its test
// `events_as_json`). Rust and TypeScript don't check each other: when an event changes in Rust,
// change it here too.

import { invoke, isTauri } from "@tauri-apps/api/core";
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

/** Starts waiting for phones; resolves with the address they connect to. */
export function start(wifi: boolean): Promise<string> {
  return invoke<string>("start", { wifi });
}

export function stop(): Promise<void> {
  return invoke("stop");
}

export function setView(view: View): Promise<void> {
  return invoke("set_view", { view });
}

/** A phone's name for people: its own name, else its model. */
export function phoneName(hello: Hello): string {
  return hello.name || hello.device?.model || "Phone";
}
