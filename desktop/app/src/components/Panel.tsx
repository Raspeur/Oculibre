// Right column of the main window (mockup Main2): camera, quality, picture, output, stop.

import { useEffect, useState } from "react";
import {
  type CameraCapabilities,
  type Capabilities,
  type ModeCapabilities,
  type ModeError,
  type Rotate,
  type View,
  capabilities,
  finish,
  setMode,
  setView,
} from "../engine";
import type { EngineState } from "../useEngine";

export function Panel({ engine }: { engine: EngineState }) {
  const { phone, stream } = engine;
  const [caps, setCaps] = useState<Capabilities | null>(null);
  const [switching, setSwitching] = useState(false);
  const [refused, setRefused] = useState<string | null>(null);
  const [view, setViewState] = useState<View>({ rotate: { kind: "Auto" }, mirror: false });

  // Ask the phone for its cameras when it arrives; forget them when it leaves.
  useEffect(() => {
    setCaps(null);
    if (phone) {
      capabilities().then(setCaps, () => setCaps(null));
    }
  }, [phone]);

  // Show what the engine applies: also the settings remembered for this phone when it comes
  // back (Transform event: the rotation in use, and whether it is automatic).
  useEffect(() => {
    const t = engine.transform;
    if (!t) return;
    setViewState({
      rotate: t.auto ? { kind: "Auto" } : { kind: "Fixed", degrees: t.rotation as 0 | 90 | 180 | 270 },
      mirror: t.mirror,
    });
  }, [engine.transform]);

  // The refused-mode message goes away by itself (mockup: problem 302, 10 s).
  useEffect(() => {
    if (!refused) return;
    const timer = setTimeout(() => setRefused(null), 10_000);
    return () => clearTimeout(timer);
  }, [refused]);

  const camera = caps?.cameras.find((c) => c.id === stream?.camera?.id) ?? null;
  const canSwitch = phone !== null && caps !== null && stream !== null && !switching;

  async function switchTo(cam: CameraCapabilities, mode: ModeCapabilities) {
    setSwitching(true);
    setRefused(null);
    try {
      await setMode({ camera: cam.id, width: mode.width, height: mode.height, fps: mode.fps, path: mode.path });
    } catch (e) {
      const error = e as ModeError;
      if (error.kind === "Refused") {
        setRefused(`${qualityText(mode)} : ${error.data}`);
      }
    } finally {
      setSwitching(false);
    }
  }

  async function changeView(next: View) {
    setViewState(next);
    await setView(next).catch(() => {}); // not running yet: applied at start (useEngine)
  }

  // Sizes this camera offers (largest first), and the frame rates of the current size.
  const sizes = camera ? uniqueSizes(camera.modes) : [];
  const current = stream ? `${stream.width}x${stream.height}` : "";
  const rates = camera
    ? camera.modes.filter((m) => `${m.width}x${m.height}` === current).map((m) => m.fps)
    : [];

  return (
    <aside className="oc-panel" aria-label="Réglages de la caméra">
      <div className="oc-panel__section" style={{ gap: 10 }}>
        <span className="oc-label">Source</span>
        {caps && caps.cameras.length > 0 ? (
          <div
            className="oc-segmented"
            role="group"
            aria-label="Caméra du téléphone"
            style={{ gridTemplateColumns: `repeat(${Math.min(caps.cameras.length, 3)}, minmax(0, 1fr))` }}
          >
            {caps.cameras.map((c) => (
              <button
                key={c.id}
                type="button"
                className="oc-segmented__item"
                aria-pressed={c.id === camera?.id}
                disabled={!canSwitch}
                onClick={() => c.id !== camera?.id && switchTo(c, closestMode(c, stream))}
              >
                {cameraLabel(c, caps.cameras)}
              </button>
            ))}
          </div>
        ) : (
          <span className="oc-small">{phone ? "Lecture des caméras…" : "Aucun téléphone connecté."}</span>
        )}
      </div>

      <div className="oc-panel__section">
        <span className="oc-label" style={{ paddingBottom: 6 }}>Vidéo</span>
        <div className="oc-row oc-row--panel">
          <label htmlFor="res" className="oc-row__label">Résolution</label>
          <select
            id="res"
            className="oc-select"
            disabled={!canSwitch || !camera}
            value={current}
            onChange={(e) => {
              const [w, h] = e.currentTarget.value.split("x").map(Number);
              const modes = camera!.modes.filter((m) => m.width === w && m.height === h);
              // Keep the frame rate if this size has it, else the highest one.
              const mode = modes.find((m) => m.fps === stream?.fps) ?? modes.sort((a, b) => b.fps - a.fps)[0];
              switchTo(camera!, mode);
            }}
          >
            {!camera && <option value="">—</option>}
            {sizes.map(([w, h]) => (
              <option key={`${w}x${h}`} value={`${w}x${h}`}>
                {sizeLabel(w, h)}
              </option>
            ))}
          </select>
        </div>
        <div className="oc-row oc-row--panel">
          <label htmlFor="fps" className="oc-row__label">Images/s</label>
          <select
            id="fps"
            className="oc-select"
            disabled={!canSwitch || !camera}
            value={stream?.fps ?? ""}
            onChange={(e) => {
              const fps = Number(e.currentTarget.value);
              const mode = camera!.modes.find((m) => `${m.width}x${m.height}` === current && m.fps === fps)!;
              switchTo(camera!, mode);
            }}
          >
            {!camera && <option value="">—</option>}
            {[...new Set(rates)].sort((a, b) => a - b).map((fps) => (
              <option key={fps} value={fps}>
                {fps >= 60 ? `${fps}` : fps}
              </option>
            ))}
          </select>
        </div>
        {switching && <span className="oc-small">Changement en cours…</span>}
        {refused && (
          <div className="oc-alert oc-alert--info" role="status" style={{ marginTop: 6 }}>
            <span className="oc-alert__icon" aria-hidden="true">i</span>
            <div className="oc-alert__body">
              <span className="oc-alert__title">Ce mode n'a pas fonctionné</span>
              <span className="oc-alert__text">{refused}. Le téléphone garde le mode précédent.</span>
              <span className="oc-alert__code">Code 302</span>
            </div>
          </div>
        )}
      </div>

      <div className="oc-panel__section">
        <span className="oc-label" style={{ paddingBottom: 6 }}>Image</span>
        <div className="oc-row oc-row--panel">
          <label htmlFor="rot" className="oc-row__label">Rotation</label>
          <select
            id="rot"
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
        <div className="oc-row oc-row--panel">
          <span id="mirror-label" className="oc-row__label">Miroir</span>
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
      </div>

      <div className="oc-panel__section">
        <span className="oc-label" style={{ paddingBottom: 6 }}>Sortie</span>
        <div className="oc-row oc-row--panel oc-row--info">
          <span className="oc-row__label">Nom dans les applications</span>
          <span className="oc-row__value">Oculibre</span>
        </div>
        <div className="oc-row oc-row--panel oc-row--info">
          <span className="oc-row__label">Format envoyé</span>
          <span className="oc-row__value">
            {engine.outputSize ? `${engine.outputSize[0]} × ${engine.outputSize[1]}` : "—"}
          </span>
        </div>
        <div className="oc-row oc-row--panel oc-row--info">
          <span className="oc-row__label">Décodage</span>
          <span className="oc-row__value oc-row__value--muted">{engine.decoder ?? "—"}</span>
        </div>
      </div>

      {phone && (
        <div className="oc-panel__section" style={{ flexDirection: "row" }}>
          <button type="button" className="oc-btn oc-btn--secondary" onClick={() => finish()}>
            <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
              <path d="M6 6h12v12H6z" />
            </svg>
            Arrêter la caméra
          </button>
        </div>
      )}
    </aside>
  );
}

/** "Arrière", "Avant"; "Arrière 2"… when several cameras face the same way. */
function cameraLabel(camera: CameraCapabilities, all: CameraCapabilities[]): string {
  const name = camera.facing === "front" ? "Avant" : camera.facing === "back" ? "Arrière" : "Externe";
  const same = all.filter((c) => c.facing === camera.facing);
  return same.length > 1 ? `${name} ${same.indexOf(camera) + 1}` : name;
}

/** On another camera, the same size and rate if it has them, else its closest to 1080p. */
function closestMode(camera: CameraCapabilities, stream: EngineState["stream"]): ModeCapabilities {
  const same = camera.modes.find((m) => m.width === stream?.width && m.height === stream?.height && m.fps === stream?.fps);
  if (same) return same;
  const score = (m: ModeCapabilities) => Math.abs(Math.min(m.width, m.height) - 1080) * 10 + Math.abs(m.fps - 30);
  return [...camera.modes].sort((a, b) => score(a) - score(b))[0];
}

function uniqueSizes(modes: ModeCapabilities[]): [number, number][] {
  const seen = new Map<string, [number, number]>();
  for (const m of modes) seen.set(`${m.width}x${m.height}`, [m.width, m.height]);
  return [...seen.values()].sort((a, b) => b[0] * b[1] - a[0] * a[1]);
}

/** "1080p", "4K", "720p": the short side, like everyone says it. */
function sizeLabel(width: number, height: number): string {
  const lines = Math.min(width, height);
  return lines >= 2160 ? "4K" : `${lines}p`;
}

function qualityText(mode: ModeCapabilities): string {
  return `${sizeLabel(mode.width, mode.height)} à ${mode.fps} i/s`;
}

function rotateFrom(value: string): Rotate {
  return value === "auto" ? { kind: "Auto" } : { kind: "Fixed", degrees: Number(value) as 0 | 90 | 180 | 270 };
}
