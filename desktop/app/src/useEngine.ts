// The engine's state as the window sees it, kept up to date from its events.
//
// `useReducer`: every engine event goes through one pure function, `reduce(state, action)`, that
// returns the next state. React draws again whatever depends on what changed.

import { useEffect, useReducer } from "react";
import {
  type EngineEvent,
  type Hello,
  type KnownPhone,
  type Status,
  type StreamConfig,
  type Transform,
  type Via,
  inApp,
  onEngineEvent,
  onPhonesChanged,
  onReceiver,
  knownPhones,
  start,
} from "./engine";

export interface EngineState {
  /** The receiver waits for phones (started when the window opens). */
  running: boolean;
  /** Where phones connect, once running. */
  address: string | null;
  /** Why the receiver could not start. */
  startError: string | null;
  /** The phone being served and how it came. */
  phone: Hello | null;
  via: Via | null;
  stream: StreamConfig | null;
  /** The phone stopped its stream for a moment (mode change). */
  paused: boolean;
  status: Status | null;
  /** Rotation and mirror applied, and whether the rotation follows the phone. */
  transform: (Transform & { auto: boolean }) | null;
  decoder: string | null;
  /** Size of the virtual camera (it may be locked by an app). */
  outputSize: [number, number] | null;
  /** Phones this computer knows (paired for Wi-Fi, or seen over USB). */
  known: KnownPhone[];
  /** Short code of this computer, shown when pairing (Wi-Fi on). */
  key: string | null;
}

const initial: EngineState = {
  running: false,
  address: null,
  startError: null,
  phone: null,
  via: null,
  stream: null,
  paused: false,
  status: null,
  transform: null,
  decoder: null,
  outputSize: null,
  known: [],
  key: null,
};

/** What can change the state: an engine event, or something the window did. */
type Action =
  | { type: "event"; event: EngineEvent }
  | { type: "started"; address: string }
  | { type: "startFailed"; error: string }
  | { type: "known"; phones: KnownPhone[] };

/** The phone left: forget everything about it. */
const noPhone = {
  phone: null,
  via: null,
  stream: null,
  paused: false,
  status: null,
  transform: null,
  decoder: null,
  outputSize: null,
};

export function reduce(state: EngineState, action: Action): EngineState {
  switch (action.type) {
    case "started":
      return { ...state, running: true, address: action.address, startError: null };
    case "startFailed":
      return { ...state, running: false, startError: action.error };
    case "known":
      return { ...state, known: action.phones };
    case "event":
      return reduceEvent(state, action.event);
  }
}

function reduceEvent(state: EngineState, event: EngineEvent): EngineState {
  switch (event.kind) {
    case "Wifi":
      return { ...state, key: event.data.key };
    case "PhoneConnected":
      return { ...state, via: event.data.via };
    case "Phone":
      return { ...state, phone: event.data };
    case "Stream":
      return { ...state, stream: event.data, paused: false };
    case "StreamPaused":
      return { ...state, paused: true };
    case "Status":
      return { ...state, status: event.data };
    case "Transform":
      return { ...state, transform: { ...event.data.transform, auto: event.data.auto } };
    case "Decoder":
      return { ...state, decoder: event.data.name };
    case "VirtualCameraSize":
      return { ...state, outputSize: event.data.size };
    case "SessionEnded":
      return { ...state, ...noPhone };
    default:
      return state; // the others are not shown in the window (yet)
  }
}

/** Starts the receiver when the window opens and follows the engine. */
export function useEngine(): EngineState {
  const [state, dispatch] = useReducer(reduce, initial);

  useEffect(() => {
    const refreshPhones = () => knownPhones().then((phones) => dispatch({ type: "known", phones }));
    const unlisten = onEngineEvent((event) => {
      dispatch({ type: "event", event });
      // A new pairing, or a phone that came (its visit is now known).
      if (event.kind === "Paired" || event.kind === "Phone") refreshPhones();
    });
    refreshPhones();
    const unlistenPhones = onPhonesChanged(refreshPhones);
    // The receiver restarted after a change of settings (Wi-Fi, shape of the virtual camera).
    const unlistenReceiver = onReceiver((result) => {
      dispatch("Ok" in result ? { type: "started", address: result.Ok } : { type: "startFailed", error: result.Err });
      refreshPhones();
    });
    if (inApp) {
      // A second call (React runs effects twice while developing) gets the same address.
      start().then(
        (address) => dispatch({ type: "started", address }),
        (error) => dispatch({ type: "startFailed", error: String(error) }),
      );
    }
    return () => {
      for (const u of [unlisten, unlistenPhones, unlistenReceiver]) u.then((stopListening) => stopListening());
    };
  }, []);

  return state;
}
