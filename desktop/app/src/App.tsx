// First window of the desktop app (#15, step 1): checks the bridge with the engine both ways.
// Not the final layout yet: the screens of design/mockups come next, with the same classes.

import { useEffect, useState } from "react";
import logo from "../../../oculibre_logo.svg";
import {
  type EngineEvent,
  type Hello,
  type Rotate,
  type Status,
  type StreamConfig,
  type View,
  inApp,
  onEngineEvent,
  phoneName,
  setView,
  start,
  stop,
} from "./engine";

/** One line of the journal: when, and what happened in plain words. */
interface Line {
  at: string;
  text: string;
}

function App() {
  // React state: when one of these changes, React draws the screen again.
  const [running, setRunning] = useState(false);
  const [wifi, setWifi] = useState(false);
  const [address, setAddress] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [phone, setPhone] = useState<Hello | null>(null);
  const [stream, setStream] = useState<StreamConfig | null>(null);
  const [status, setStatus] = useState<Status | null>(null);
  const [view, setViewState] = useState<View>({ rotate: { kind: "Auto" }, mirror: false });
  const [journal, setJournal] = useState<Line[]>([]);

  // Listen to the engine once, when the window opens; stop listening when it closes.
  useEffect(() => {
    const unlisten = onEngineEvent((event) => {
      switch (event.kind) {
        case "Phone":
          setPhone(event.data);
          break;
        case "Stream":
          setStream(event.data);
          break;
        case "Status":
          setStatus(event.data);
          return; // every second: not in the journal
        case "SessionEnded":
          setPhone(null);
          setStream(null);
          setStatus(null);
          break;
      }
      const at = new Date().toLocaleTimeString();
      setJournal((lines) => [{ at, text: describe(event) }, ...lines].slice(0, 50));
    });
    return () => {
      unlisten.then((stopListening) => stopListening());
    };
  }, []);

  async function toggleRunning() {
    setError(null);
    try {
      if (running) {
        await stop();
        setRunning(false);
        setAddress(null);
        setPhone(null);
        setStream(null);
        setStatus(null);
      } else {
        setAddress(await start(wifi));
        setRunning(true);
        await setView(view); // the view chosen before Start
      }
    } catch (e) {
      setError(String(e)); // a Rust Err(String) arrives here
    }
  }

  async function changeView(next: View) {
    setViewState(next);
    if (running) {
      await setView(next).catch((e) => setError(String(e)));
    }
  }

  const live = phone !== null;
  return (
    <div className="oc-app oc-app--stack">
      <header className="oc-header">
        <div className="oc-logo oc-logo--md">
          <img src={logo} alt="" />
        </div>
        <span className="oc-brand">Oculibre</span>
        <span className={live ? "oc-pill" : "oc-pill oc-pill--paused"}>
          <span className={live ? "oc-dot" : "oc-dot oc-dot--paused"} aria-hidden="true" />
          {live ? `En direct · ${phoneName(phone)}` : running ? "En attente d'un téléphone" : "Arrêté"}
        </span>
      </header>

      <main className="oc-container" style={{ maxWidth: 960 }}>
        {!inApp && (
          <div className="oc-alert oc-alert--info" role="status">
            <span className="oc-alert__icon" aria-hidden="true">i</span>
            <div className="oc-alert__body">
              <span className="oc-alert__title">Aperçu dans un navigateur</span>
              <span className="oc-alert__text">
                Seule la mise en page s'affiche ici : pour piloter le téléphone, ouvrez la fenêtre
                d'Oculibre (npm run tauri dev).
              </span>
            </div>
          </div>
        )}
        {error && (
          <div className="oc-alert oc-alert--error" role="alert">
            <span className="oc-alert__icon" aria-hidden="true">×</span>
            <div className="oc-alert__body">
              <span className="oc-alert__title">Impossible de continuer</span>
              <span className="oc-alert__text">{error}</span>
            </div>
          </div>
        )}

        <section className="oc-card">
          <div className="oc-row">
            <span className="oc-row__label">
              <span id="wifi-label">Accepter les téléphones en Wi-Fi</span>
              <span className="oc-row__hint">Seulement les téléphones reliés, connexion chiffrée.</span>
            </span>
            <button
              type="button"
              className="oc-switch"
              aria-pressed={wifi}
              aria-labelledby="wifi-label"
              disabled={running}
              onClick={() => setWifi(!wifi)}
            >
              <span className="oc-switch__knob" aria-hidden="true" />
            </button>
          </div>
          <div className="oc-row">
            <span className="oc-row__label">
              Récepteur
              <span className="oc-row__hint">
                {address ? `Les téléphones se connectent à ${address}` : "Arrêté"}
              </span>
            </span>
            <button
              type="button"
              className={running ? "oc-btn oc-btn--secondary" : "oc-btn oc-btn--primary"}
              disabled={!inApp}
              onClick={toggleRunning}
            >
              {running ? "Arrêter" : "Démarrer"}
            </button>
          </div>
        </section>

        <section className="oc-stats" aria-label="Mesures">
          <Stat label="Latence" value={ms(status?.latency)} />
          <Stat label="Images/s" value={status ? status.shown_fps.toFixed(0) : "—"} />
          <Stat label="Débit" value={status ? `${status.mbits.toFixed(1)} Mbit/s` : "—"} />
          <Stat
            label="Flux"
            value={stream ? `${stream.width}×${stream.height} · ${stream.fps}` : "—"}
          />
        </section>

        <section className="oc-card">
          <div className="oc-row">
            <label htmlFor="rotation" className="oc-row__label">
              Rotation
            </label>
            <select
              id="rotation"
              className="oc-select"
              value={view.rotate.kind === "Auto" ? "auto" : String(view.rotate.degrees)}
              onChange={(e) => changeView({ ...view, rotate: rotateFrom(e.currentTarget.value) })}
            >
              <option value="auto">Automatique</option>
              <option value="0">0°</option>
              <option value="90">90°</option>
              <option value="180">180°</option>
              <option value="270">270°</option>
            </select>
          </div>
          <div className="oc-row">
            <span id="mirror-label" className="oc-row__label">
              Miroir
            </span>
            <button
              type="button"
              className="oc-switch"
              aria-pressed={view.mirror}
              aria-labelledby="mirror-label"
              onClick={() => changeView({ ...view, mirror: !view.mirror })}
            >
              <span className="oc-switch__knob" aria-hidden="true" />
            </button>
          </div>
        </section>

        <section className="oc-stack">
          <h2 className="oc-label">Journal</h2>
          <ol className="oc-card oc-card--padded oc-stack" style={{ margin: 0, gap: 6, listStyle: "none" }}>
            {journal.length === 0 && <li className="oc-small">Rien pour l'instant.</li>}
            {journal.map((line, i) => (
              <li key={journal.length - i} className="oc-text" style={{ fontSize: 13 }}>
                <span className="oc-muted">{line.at}</span> {line.text}
              </li>
            ))}
          </ol>
        </section>
      </main>
    </div>
  );
}

/** A small React component: one tile of the stats row. */
function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="oc-stat">
      <span className="oc-stat__label">{label}</span>
      <span className="oc-stat__value">{value}</span>
    </div>
  );
}

function ms(value: number | null | undefined): string {
  return value == null ? "—" : `${value.toFixed(0)} ms`;
}

function rotateFrom(value: string): Rotate {
  return value === "auto" ? { kind: "Auto" } : { kind: "Fixed", degrees: Number(value) as 0 | 90 | 180 | 270 };
}

/** An engine event in plain words, for the journal. */
function describe(event: EngineEvent): string {
  switch (event.kind) {
    case "VirtualCamera":
      return `Caméra virtuelle : ${event.data.path}`;
    case "Usb":
      return "Ok" in event.data ? "USB prêt" : `USB indisponible : ${event.data.Err}`;
    case "Wifi":
      return `Wi-Fi activé · code de cet ordinateur ${event.data.key}`;
    case "AnnounceFailed":
      return `Annonce sur le réseau impossible : ${event.data}`;
    case "PhoneConnected":
      return `Téléphone connecté (${event.data.via.kind === "Usb" ? "câble USB" : "Wi-Fi chiffré"})`;
    case "PhoneRefused":
      return `Connexion refusée depuis ${event.data.peer}`;
    case "AcceptFailed":
      return `Erreur réseau : ${event.data}`;
    case "Paired":
      return `${event.data.name} est relié`;
    case "SessionEnded":
      return "Ok" in event.data ? "Le téléphone s'est arrêté" : `Connexion perdue : ${event.data.Err}`;
    case "Phone":
      return `${phoneName(event.data)} est là`;
    case "Stream":
      return `Vidéo ${event.data.width}×${event.data.height} à ${event.data.fps} i/s`;
    case "StreamPaused":
      return "Vidéo en pause (changement de mode)";
    case "Transform":
      return `Rotation ${event.data.transform.rotation}°${event.data.transform.mirror ? ", miroir" : ""}`;
    case "Decoder":
      return `Décodage : ${event.data.name}`;
    case "VirtualCameraSize":
      return `Caméra virtuelle en ${event.data.size[0]}×${event.data.size[1]}`;
    case "RequestRefused":
      return `Le téléphone a refusé une demande (${event.data.code})`;
    case "Status":
      return "État";
    case "Summary":
      return `Session : ${event.data.seconds} s, ${event.data.fps.toFixed(1)} i/s`;
  }
}

export default App;
