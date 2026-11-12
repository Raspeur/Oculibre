// Centre of the main window (mockup Main2): the phone being served, its picture and its numbers;
// or, without a phone, what to do.

import { phoneName } from "../engine";
import type { EngineState } from "../useEngine";

export function Stage({ engine }: { engine: EngineState }) {
  const { phone, via, stream, paused, status } = engine;

  if (!phone) {
    return (
      <section aria-label="En attente" className="oc-stack" style={stageStyle}>
        <div className="oc-preview">
          <div className="oc-stack" style={{ alignItems: "center", gap: 14, maxWidth: 440, textAlign: "center", padding: 24 }}>
            <CameraIcon />
            <h1 className="oc-title oc-title--md">En attente d'un téléphone</h1>
            <p className="oc-text oc-muted">
              Ouvrez Oculibre sur le téléphone et appuyez sur « Démarrer ». Par câble USB, il suffit
              de le brancher ; en Wi-Fi, il doit être relié à cet ordinateur et sur le même réseau.
            </p>
            {engine.key && <span className="oc-small">Code de cet ordinateur : {engine.key}</span>}
          </div>
        </div>
      </section>
    );
  }

  const live = stream !== null && !paused;
  return (
    <section aria-label="Aperçu" className="oc-stack" style={stageStyle}>
      <header className="oc-cluster">
        <h1 className="oc-title oc-title--md">{phoneName(phone)}</h1>
        <span className={live ? "oc-pill" : "oc-pill oc-pill--paused"}>
          <span className={live ? "oc-dot" : "oc-dot oc-dot--paused"} aria-hidden="true" />
          {live ? "En direct" : paused ? "Changement de mode…" : "Démarrage…"}
        </span>
        <span className="oc-muted" style={{ fontSize: "var(--oc-text-md)" }}>
          {via?.kind === "Wifi" ? "Wi-Fi chiffré" : "Câble USB"}
        </span>
      </header>

      <div className="oc-preview">
        <div className="oc-stack" style={{ alignItems: "center", gap: 12, color: "var(--oc-text-faint)", textAlign: "center", padding: 24 }}>
          <CameraIcon />
          <span className="oc-text">L'image est envoyée à la caméra « Oculibre ».</span>
          <span className="oc-small">Aperçu dans cette fenêtre : bientôt.</span>
        </div>
        {stream && (
          <span className="oc-chip" style={{ position: "absolute", top: 12, left: 12 }}>
            {qualityLabel(stream.width, stream.height, stream.fps)}
          </span>
        )}
      </div>

      <div className="oc-stats">
        <Stat label="Latence" value={status?.latency != null ? `${status.latency.toFixed(0)} ms` : "—"} />
        <Stat label="Images/s" value={status ? status.shown_fps.toFixed(0) : "—"} />
        <Stat label="Débit" value={status ? `${status.mbits.toFixed(1)} Mbit/s` : "—"} />
        <Stat label="Connexion" value={via?.kind === "Wifi" ? "Wi-Fi" : "USB"} />
      </div>
    </section>
  );
}

const stageStyle = { flex: "999 1 520px", minWidth: 0, padding: 24, boxSizing: "border-box", gap: 16 } as const;

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="oc-stat">
      <span className="oc-stat__label">{label}</span>
      <span className="oc-stat__value">{value}</span>
    </div>
  );
}

/** "1080p · 60 i/s": the short side, like everyone says it; 2160 lines is called 4K. */
function qualityLabel(width: number, height: number, fps: number): string {
  const lines = Math.min(width, height);
  return `${lines >= 2160 ? "4K" : `${lines}p`} · ${fps} i/s`;
}

function CameraIcon() {
  return (
    <svg width="52" height="52" viewBox="0 0 24 24" fill="none" stroke="#3A4C70" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M23 7l-7 5 7 5V7z" />
      <rect x="1" y="5" width="15" height="14" rx="2" />
    </svg>
  );
}
