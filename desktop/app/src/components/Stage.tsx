// Centre of the main window (mockup Main2): the phone being served, its picture and its numbers;
// or, without a phone, what to do.

import { useEffect, useRef, useState } from "react";
import { type PreviewMode, onPreview, phoneName, setPreviewMode } from "../engine";
import type { EngineState } from "../useEngine";

export function Stage({ engine }: { engine: EngineState }) {
  const { phone, via, stream, paused, status } = engine;
  const [mode, setMode] = usePreviewMode();
  const preview = usePreview(phone !== null && mode !== "Off");

  if (!phone) {
    return (
      <section aria-label="En attente" className="oc-stack" style={stageStyle} data-tour="stage">
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
    <section aria-label="Aperçu" className="oc-stack" style={stageStyle} data-tour="stage">
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
        {mode === "Off" ? (
          <div className="oc-stack" style={{ alignItems: "center", gap: 12, color: "var(--oc-text-faint)", textAlign: "center", padding: 24 }}>
            <CameraIcon />
            <span className="oc-text">Aperçu arrêté</span>
            <span className="oc-small">Les applications reçoivent l'image normalement.</span>
          </div>
        ) : preview ? (
          <img
            src={preview}
            alt="Aperçu de ce que reçoivent les applications"
            style={{ width: "100%", height: "100%", objectFit: "contain", opacity: live ? 1 : 0.4 }}
          />
        ) : (
          <div className="oc-stack" style={{ alignItems: "center", gap: 12, color: "var(--oc-text-faint)", textAlign: "center", padding: 24 }}>
            <CameraIcon />
            <span className="oc-text">L'image arrive…</span>
          </div>
        )}
        <div
          className="oc-segmented oc-segmented--sm"
          role="group"
          aria-label="Aperçu"
          style={{ position: "absolute", top: 12, right: 12 }}
        >
          {previewModes.map(([value, label, hint]) => (
            <button
              key={value}
              type="button"
              className="oc-segmented__item"
              aria-pressed={mode === value}
              title={hint}
              onClick={() => setMode(value)}
            >
              {label}
            </button>
          ))}
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

const previewModes: [PreviewMode, string, string][] = [
  ["Off", "Arrêté", "Aucun calcul pour l'aperçu"],
  ["Light", "Léger", "Environ 10 images/s : ne ralentit jamais la caméra"],
  ["Full", "Fluide", "Toutes les images : quand rien d'autre ne demande l'ordinateur"],
];

/** The preview mode, remembered by the window (localStorage) and given to the engine. */
function usePreviewMode(): [PreviewMode, (mode: PreviewMode) => void] {
  const [mode, setModeState] = useState<PreviewMode>(() => {
    try {
      const saved = localStorage.getItem("oculibre.preview");
      if (saved === "Off" || saved === "Light" || saved === "Full") return saved;
    } catch {
      // storage unavailable: the default
    }
    return "Light";
  });

  // Tell the engine at start and at every change.
  useEffect(() => {
    setPreviewMode(mode);
    try {
      localStorage.setItem("oculibre.preview", mode);
    } catch {
      // not remembered, still applied
    }
  }, [mode]);

  return [mode, setModeState];
}

/**
 * The latest preview picture as an object URL (an address in memory an <img> can show), or
 * null. Each new picture frees the previous URL: otherwise every picture would stay in memory.
 */
function usePreview(withPhone: boolean): string | null {
  const [url, setUrl] = useState<string | null>(null);
  const current = useRef<string | null>(null);

  const show = (next: string | null) => {
    if (current.current) URL.revokeObjectURL(current.current);
    current.current = next;
    setUrl(next);
  };

  // Subscribe once; a second call (StrictMode while developing) replaces the first channel.
  useEffect(() => {
    onPreview((jpeg) => show(URL.createObjectURL(new Blob([jpeg], { type: "image/jpeg" }))));
    return () => show(null);
  }, []);

  // The phone left: no old picture when the next one comes.
  useEffect(() => {
    if (!withPhone) show(null);
  }, [withPhone]);

  return withPhone ? url : null;
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
