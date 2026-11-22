// Centre of the main window (mockup Main2): the phone being served, its picture and its numbers;
// or, without a phone, what to do.

import { useEffect, useRef, useState } from "react";
import {
  type DecoderChoice,
  type PreviewMode,
  type Status,
  onPreview,
  onWindows,
  phoneName,
  setPreviewMode,
  setSettings,
  settings,
} from "../engine";
import type { EngineState } from "../useEngine";
import { type Key, t, useLang } from "../i18n";

export function Stage({ engine }: { engine: EngineState }) {
  useLang();
  const { phone, via, stream, paused, status } = engine;
  const [mode, setMode] = usePreviewMode();
  const [details, setDetails] = useDetailsOpen();
  const preview = usePreview(phone !== null && mode !== "Off");
  const decoder = useDecoderChoice(status?.behind ?? false);

  if (!phone) {
    return (
      <section aria-label={t("waiting")} className="oc-stack" style={stageStyle} data-tour="stage">
        <div className="oc-preview">
          <div className="oc-stack" style={{ alignItems: "center", gap: 14, maxWidth: 440, textAlign: "center", padding: 24 }}>
            <CameraIcon />
            <h1 className="oc-title oc-title--md">{t("waiting_title")}</h1>
            <p className="oc-text oc-muted">{t("waiting_text")}</p>
            {engine.key && <span className="oc-small">{t("computer_code", { key: engine.key })}</span>}
          </div>
        </div>
      </section>
    );
  }

  const live = stream !== null && !paused;
  return (
    <section aria-label={t("preview")} className="oc-stack" style={stageStyle} data-tour="stage">
      <header className="oc-cluster">
        <h1 className="oc-title oc-title--md">{phoneName(phone)}</h1>
        <span className={live ? "oc-pill" : "oc-pill oc-pill--paused"}>
          <span className={live ? "oc-dot" : "oc-dot oc-dot--paused"} aria-hidden="true" />
          {live ? t("live") : paused ? t("changing_mode") : t("starting")}
        </span>
        <span className="oc-muted" style={{ fontSize: "var(--oc-text-md)" }}>
          {via?.kind === "Wifi" ? t("wifi_encrypted") : t("usb_cable")}
        </span>
      </header>

      <div className="oc-preview">
        {mode === "Off" ? (
          <div className="oc-stack" style={{ alignItems: "center", gap: 12, color: "var(--oc-text-faint)", textAlign: "center", padding: 24 }}>
            <CameraIcon />
            <span className="oc-text">{t("preview_off")}</span>
            <span className="oc-small">{t("preview_off_text")}</span>
          </div>
        ) : preview ? (
          <img
            src={preview}
            alt={t("preview_alt")}
            // Absolute: the picture never sizes the frame (a portrait picture made it as tall as
            // the picture, pushing the figures out of the window); it fits inside instead.
            style={{ position: "absolute", inset: 0, width: "100%", height: "100%", objectFit: "contain", opacity: live ? 1 : 0.4 }}
          />
        ) : (
          <div className="oc-stack" style={{ alignItems: "center", gap: 12, color: "var(--oc-text-faint)", textAlign: "center", padding: 24 }}>
            <CameraIcon />
            <span className="oc-text">{t("picture_coming")}</span>
          </div>
        )}
        <div
          className="oc-segmented oc-segmented--sm"
          role="group"
          aria-label={t("preview")}
          style={{ position: "absolute", top: 12, right: 12 }}
        >
          {previewModes().map(([value, label, hint]) => (
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
        <button
          type="button"
          className="oc-stat oc-stat--button"
          aria-expanded={details}
          aria-controls="latency-details"
          title={details ? t("latency_details_hide") : t("latency_details_show")}
          onClick={() => setDetails(!details)}
        >
          <span className="oc-stat__label">
            {t("latency")} <Chevron open={details} />
          </span>
          <span className="oc-stat__value">{status?.latency != null ? ms(status.latency) : "—"}</span>
        </button>
        <Stat label={t("frames_per_second")} value={status ? status.shown_fps.toFixed(0) : "—"} />
        <Stat label={t("bitrate")} value={status ? `${status.mbits.toFixed(1)} Mbit/s` : "—"} />
        <Stat label={t("connection")} value={via?.kind === "Wifi" ? "Wi-Fi" : "USB"} />
      </div>
      {status?.behind && (
        <div className="oc-alert" role="status">
          <span className="oc-alert__icon" aria-hidden="true">!</span>
          <div className="oc-alert__body">
            <span className="oc-alert__title">{t("behind_title")}</span>
            <span className="oc-alert__text">{t("behind_text")}</span>
            {decoder.choice !== null && decoder.choice !== "auto" && (
              <span className="oc-alert__text">{t("behind_decoder", { choice: t(decoderLabel(decoder.choice)) })}</span>
            )}
            {decoder.saved && <span className="oc-alert__text">{t("behind_decoder_saved")}</span>}
            {(mode === "Full" || (decoder.choice !== null && decoder.choice !== "auto")) && (
              <div className="oc-alert__actions">
                {mode === "Full" && (
                  <button type="button" className="oc-btn oc-btn--secondary" onClick={() => setMode("Light")}>{t("behind_light")}</button>
                )}
                {decoder.choice !== null && decoder.choice !== "auto" && (
                  <button type="button" className="oc-btn oc-btn--secondary" onClick={decoder.setAuto}>{t("behind_decoder_auto")}</button>
                )}
              </div>
            )}
            <span className="oc-alert__code">{t("code", { code: 504 })}</span>
          </div>
        </div>
      )}
      {engine.decoderTooSlow && (
        <div className="oc-alert oc-alert--info" role="status">
          <span className="oc-alert__icon" aria-hidden="true">i</span>
          <div className="oc-alert__body">
            <span className="oc-alert__title">{t("gpu_slow_title")}</span>
            <span className="oc-alert__text">
              {t("gpu_slow_text", {
                decode: engine.decoderTooSlow.decode_ms.toFixed(0),
                frame: engine.decoderTooSlow.frame_ms.toFixed(0),
              })}
            </span>
            <span className="oc-alert__code">{t("code", { code: 505 })}</span>
          </div>
        </div>
      )}
      {status?.link_slow && (
        <div className="oc-alert" role="status">
          <span className="oc-alert__icon" aria-hidden="true">!</span>
          <div className="oc-alert__body">
            <span className="oc-alert__title">{t("link_slow_title")}</span>
            <span className="oc-alert__text">{t("link_slow_text")}</span>
            <span className="oc-alert__code">{t("code", { code: 501 })}</span>
          </div>
        </div>
      )}
      {details && <LatencyDetails status={status} />}
    </section>
  );
}

/** In the current language: a function, not a constant made once at load. */
const previewModes = (): [PreviewMode, string, string][] => [
  ["Off", t("preview_mode_off"), t("preview_mode_off_hint")],
  ["Light", t("preview_mode_light"), t("preview_mode_light_hint")],
  ["Full", t("preview_mode_full"), t("preview_mode_full_hint")],
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

const ms = (v: number) => `${Math.round(v)} ms`;

/** Whether the latency details are open; remembered by this computer. */
function useDetailsOpen(): [boolean, (open: boolean) => void] {
  const [open, setOpen] = useState(() => {
    try {
      return localStorage.getItem("latency-details") === "open";
    } catch {
      return false;
    }
  });
  const set = (value: boolean) => {
    setOpen(value);
    try {
      localStorage.setItem("latency-details", value ? "open" : "closed");
    } catch {
      // private storage refused: the choice only lasts until the window closes
    }
  };
  return [open, set];
}

/**
 * Where the delay of the last second comes from: the phone (camera, encoder, its queue), the
 * network (what remains, so it also counts the time the video waits in the sockets when the
 * computer reads too slowly) and the computer (decoding, then rotation + conversion + write).
 */
function LatencyDetails({ status }: { status: Status | null }) {
  const total = status?.latency;
  const parts =
    status && total != null && status.phone != null && status.pc != null
      ? { phone: status.phone, pc: status.pc, network: Math.max(0, total - status.phone - status.pc) }
      : null;
  const biggest = parts ? (Object.entries(parts).sort((a, b) => b[1] - a[1])[0][0] as keyof typeof parts) : null;
  const share = (v: number) => `${total ? Math.max(2, (v / total) * 100) : 0}%`;
  return (
    <section id="latency-details" className="oc-latency" aria-label={t("lat_title")}>
      <h2 className="oc-label">{t("lat_title")}</h2>
      {!parts || !status ? (
        <p className="oc-small">{t("lat_unmeasured")}</p>
      ) : (
        <>
          <div className="oc-latency__bar" aria-hidden="true">
            <span className="oc-latency__seg oc-latency__seg--phone" style={{ width: share(parts.phone) }} />
            <span className="oc-latency__seg oc-latency__seg--network" style={{ width: share(parts.network) }} />
            <span className="oc-latency__seg oc-latency__seg--pc" style={{ width: share(parts.pc) }} />
          </div>
          <dl className="oc-latency__rows">
            <Part kind="phone" strong={biggest === "phone"} label={t("lat_phone")} value={parts.phone}
              detail={status.split && t("lat_phone_split", {
                camera: ms(status.split.camera), encoder: ms(status.split.encoder), queue: ms(status.split.queue),
              })} />
            <Part kind="network" strong={biggest === "network"} label={t("lat_network")} value={parts.network}
              detail={t("lat_network_hint")} />
            <Part kind="pc" strong={biggest === "pc"} label={t("lat_pc")} value={parts.pc}
              detail={status.pc_decode != null && t("lat_pc_split", {
                decode: ms(status.pc_decode), output: ms(Math.max(0, parts.pc - status.pc_decode)),
              })} />
          </dl>
          <p className="oc-small">
            {t("lat_max_avg", {
              max: status.latency_max != null ? ms(status.latency_max) : "—",
              avg: status.latency_avg != null ? ms(status.latency_avg) : "—",
            })}
          </p>
        </>
      )}
      {status && (
        <p className="oc-small">
          {t("lat_frames", { shown: status.shown_fps.toFixed(0), camera: status.camera_fps ?? "—" })}
          {status.dropped != null && <> · {t("lat_dropped", { n: status.dropped })}</>}
          {status.skipped > 0 && <> · {t("lat_skipped", { n: status.skipped })}</>}
          {status.target_mbits != null && <> · {t("lat_lowered", { mbits: status.target_mbits.toFixed(1) })}</>}
          {status.errors > 0 && <> · {t("lat_errors", { n: status.errors })}</>}
          {status.rtt != null && <> · {t("lat_rtt", { rtt: `${status.rtt.toFixed(1)} ms` })}</>}
        </p>
      )}
    </section>
  );
}

function Part(props: { kind: string; strong: boolean; label: string; value: number; detail?: string | false | null }) {
  return (
    <div className="oc-latency__row" data-strong={props.strong || undefined}>
      <dt>
        <span className={`oc-latency__key oc-latency__seg--${props.kind}`} aria-hidden="true" />
        {props.label}
      </dt>
      <dd>
        <strong>{ms(props.value)}</strong>
        {props.detail && <span className="oc-small"> {props.detail}</span>}
      </dd>
    </div>
  );
}

function Chevron({ open }: { open: boolean }) {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" aria-hidden="true" style={{ verticalAlign: "-1px", transform: open ? "rotate(180deg)" : undefined }}>
      <path d="m6 9 6 6 6-6" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="oc-stat">
      <span className="oc-stat__label">{label}</span>
      <span className="oc-stat__value">{value}</span>
    </div>
  );
}

/** "1080p · 60 fps": the short side, like everyone says it; 2160 lines is called 4K. */
function qualityLabel(width: number, height: number, fps: number): string {
  const lines = Math.min(width, height);
  return t("quality", { size: lines >= 2160 ? "4K" : `${lines}p`, fps });
}

function CameraIcon() {
  return (
    <svg width="52" height="52" viewBox="0 0 24 24" fill="none" stroke="#3A4C70" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M23 7l-7 5 7 5V7z" />
      <rect x="1" y="5" width="15" height="14" rx="2" />
    </svg>
  );
}

/** The decoder chosen in the settings, read again whenever `behind` turns on (problem 504). */
function useDecoderChoice(behind: boolean) {
  const [choice, setChoice] = useState<DecoderChoice | null>(null);
  const [saved, setSaved] = useState(false);
  useEffect(() => {
    if (!behind) return;
    let live = true;
    settings().then((s) => live && setChoice(s.decoder)).catch(() => {});
    return () => {
      live = false;
    };
  }, [behind]);
  const setAuto = () => {
    settings()
      .then((s) => setSettings({ ...s, decoder: "auto" }))
      .then(() => {
        setChoice("auto");
        setSaved(true);
      })
      .catch(() => {});
  };
  return { choice, saved, setAuto };
}

/** The name the settings list gives a decoder choice. */
function decoderLabel(choice: DecoderChoice): Key {
  switch (choice) {
    case "vaapi":
      return onWindows ? "decoder_gpu_windows" : "decoder_gpu";
    case "nvdec":
      return "decoder_nvidia";
    case "software":
      return onWindows ? "decoder_windows" : "decoder_cpu";
    case "openh264":
      return "decoder_openh264";
    default:
      return "automatic";
  }
}
