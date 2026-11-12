// Settings of this computer (mockup design/mockups/Settings): virtual camera, phones and Wi-Fi,
// performance, about. "General" (language, start with the computer, tray) comes with the tray.

import { getVersion } from "@tauri-apps/api/app";
import { useEffect, useRef, useState } from "react";
import logo from "../../../../oculibre_logo.svg";
import {
  type AppSettings,
  type DecoderChoice,
  forgetPhone,
  inApp,
  setSettings,
  settings as loadSettings,
} from "../engine";
import type { EngineState } from "../useEngine";

export function Settings(props: { engine: EngineState; section?: "about"; onDone: () => void }) {
  const { engine } = props;
  const [values, setValues] = useState<AppSettings | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState<string>("");
  const about = useRef<HTMLElement>(null);

  useEffect(() => {
    loadSettings().then(setValues);
    if (inApp) getVersion().then(setVersion);
  }, []);

  // Opened from "About": scroll there.
  useEffect(() => {
    if (props.section === "about") about.current?.scrollIntoView();
  }, [props.section, values]);

  async function change(next: AppSettings) {
    setValues(next);
    setError(null);
    try {
      const applied = await setSettings(next);
      setNotice(
        applied === "AfterThisPhone"
          ? "Pris en compte : s'appliquera quand le téléphone en direct se déconnectera."
          : null,
      );
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="oc-app" style={{ display: "flex", flexDirection: "column" }}>
      <header className="oc-header">
        <button type="button" className="oc-icon-btn" aria-label="Retour" onClick={props.onDone}>
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M15 18l-6-6 6-6" />
          </svg>
        </button>
        <h1 className="oc-title oc-title--sm">Réglages</h1>
      </header>

      <div className="oc-container">
        {notice && (
          <div className="oc-alert oc-alert--info" role="status">
            <span className="oc-alert__icon" aria-hidden="true">i</span>
            <div className="oc-alert__body">
              <span className="oc-alert__text">{notice}</span>
            </div>
          </div>
        )}
        {error && (
          <div className="oc-alert oc-alert--error" role="alert">
            <span className="oc-alert__icon" aria-hidden="true">×</span>
            <div className="oc-alert__body">
              <span className="oc-alert__title">Réglage non enregistré</span>
              <span className="oc-alert__text">{error}</span>
            </div>
          </div>
        )}

        <Section id="s-vcam" title="Caméra virtuelle">
          <div className="oc-row">
            <span className="oc-row__label">État</span>
            {engine.running ? (
              <span className="oc-status oc-strong">
                <span className="oc-dot" style={{ width: 7, height: 7 }} aria-hidden="true" />
                Installée
              </span>
            ) : (
              <span className="oc-row__value oc-row__value--todo">Indisponible</span>
            )}
          </div>
          <div className="oc-row">
            <span className="oc-row__label">Nom dans les applications</span>
            <span className="oc-row__value">Oculibre</span>
          </div>
          <div className="oc-row">
            <span className="oc-row__label">
              <label htmlFor="shape">Forme de l'image</label>
              <span className="oc-row__hint">Les applications ouvertes gardent la forme qu'elles ont vue.</span>
            </span>
            <select
              id="shape"
              className="oc-select oc-select--wide"
              disabled={!values}
              value={values?.portrait ? "portrait" : "landscape"}
              onChange={(e) => values && change({ ...values, portrait: e.currentTarget.value === "portrait" })}
            >
              <option value="landscape">Paysage (comme une webcam)</option>
              <option value="portrait">Portrait</option>
            </select>
          </div>
        </Section>

        <Section id="s-net" title="Téléphones et Wi-Fi">
          <div className="oc-row">
            <span className="oc-row__label">
              <span id="wifi-label">Accepter les téléphones en Wi-Fi</span>
              <span className="oc-row__hint">Seulement les téléphones reliés, connexion chiffrée.</span>
            </span>
            <button
              type="button"
              className="oc-switch"
              aria-pressed={values?.wifi ?? false}
              aria-labelledby="wifi-label"
              disabled={!values}
              onClick={() => values && change({ ...values, wifi: !values.wifi })}
            >
              <span className="oc-switch__knob" aria-hidden="true" />
            </button>
          </div>
          {engine.paired.length === 0 ? (
            <div className="oc-row">
              <span className="oc-row__label">Téléphones reliés</span>
              <span className="oc-row__value oc-row__value--muted">Aucun</span>
            </div>
          ) : (
            engine.paired.map((p) => (
              <PairedPhoneRow key={p.fingerprint} name={p.name} pairedOn={p.paired_on} fingerprint={p.fingerprint} onError={setError} />
            ))
          )}
          <div className="oc-row">
            <span className="oc-row__label">
              Code de cet ordinateur
              <span className="oc-row__hint">Le téléphone affiche le même au moment de le relier.</span>
            </span>
            <span className="oc-row__value" style={{ letterSpacing: 1 }}>{engine.key ?? "—"}</span>
          </div>
        </Section>

        <Section id="s-perf" title="Performances">
          <div className="oc-row">
            <span className="oc-row__label">
              <label htmlFor="dec">Décodage de la vidéo</label>
              <span className="oc-row__hint">
                {engine.decoder ? `Utilisé : ${engine.decoder}. ` : ""}Un changement s'applique au prochain flux.
              </span>
            </span>
            <select
              id="dec"
              className="oc-select oc-select--wide"
              disabled={!values}
              value={values?.decoder ?? "auto"}
              onChange={(e) => values && change({ ...values, decoder: e.currentTarget.value as DecoderChoice })}
            >
              <option value="auto">Automatique</option>
              <option value="vaapi">Carte graphique (AMD, Intel)</option>
              <option value="nvdec">Carte graphique (NVIDIA)</option>
              <option value="software">Processeur</option>
              <option value="openh264">Processeur (OpenH264)</option>
            </select>
          </div>
        </Section>

        <section id="about" ref={about} aria-labelledby="s-about" className="oc-stack">
          <h2 id="s-about" className="oc-label" style={{ padding: "0 4px" }}>À propos</h2>
          <div className="oc-card oc-card--padded oc-cluster" style={{ gap: 16 }}>
            <div className="oc-logo oc-logo--lg">
              <img src={logo} alt="" />
            </div>
            <div className="oc-stack" style={{ flex: "1 1 260px", gap: 3 }}>
              <span className="oc-title" style={{ fontSize: "var(--oc-text-xl)" }}>Oculibre {version}</span>
              <span className="oc-muted" style={{ fontSize: "var(--oc-text-md)" }}>
                Libre et gratuit, licence MIT. Aucune donnée ne quitte votre réseau.
              </span>
            </div>
          </div>
          <div className="oc-card">
            <div className="oc-row" style={{ minHeight: 44 }}>
              <span className="oc-label">Licences des composants</span>
            </div>
            {notices.map(([name, licence, note]) => (
              <div key={name} className="oc-row" style={{ alignItems: "flex-start" }}>
                <span className="oc-row__label">
                  {name}
                  {note && <span className="oc-row__hint">{note}</span>}
                </span>
                <span className="oc-row__value oc-row__value--muted">{licence}</span>
              </div>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}

function Section(props: { id: string; title: string; children: React.ReactNode }) {
  return (
    <section aria-labelledby={props.id} className="oc-stack">
      <h2 id={props.id} className="oc-label" style={{ padding: "0 4px" }}>{props.title}</h2>
      <div className="oc-card">{props.children}</div>
    </section>
  );
}

/** A paired phone, with "Forget" asked twice: it will need a new pairing to come back. */
function PairedPhoneRow(props: { name: string; pairedOn: string; fingerprint: string; onError: (e: string) => void }) {
  const [confirming, setConfirming] = useState(false);
  return (
    <div className="oc-row">
      <span className="oc-row__label">
        {props.name}
        <span className="oc-row__hint">
          {confirming ? "Il faudra le relier à nouveau pour l'utiliser en Wi-Fi." : `Relié le ${props.pairedOn}`}
        </span>
      </span>
      {confirming ? (
        <span className="oc-cluster" style={{ gap: 8 }}>
          <button type="button" className="oc-btn oc-btn--ghost" onClick={() => setConfirming(false)}>Annuler</button>
          <button
            type="button"
            className="oc-btn oc-btn--danger"
            onClick={() => forgetPhone(props.fingerprint).catch((e) => props.onError(String(e)))}
          >
            Oublier
          </button>
        </span>
      ) : (
        <button type="button" className="oc-btn oc-btn--secondary" onClick={() => setConfirming(true)}>Oublier…</button>
      )}
    </div>
  );
}

/** Notices the licences of the components ask for (see docs/research/video-decoding.typ). */
const notices: [string, string, string?][] = [
  ["FFmpeg (libavcodec)", "LGPL-2.1+ / GPL-2+", "La bibliothèque de votre système, selon sa construction."],
  ["OpenH264", "BSD-2-Clause", "© Cisco Systems."],
  ["jpeg-encoder", "MIT / Apache-2.0, IJG", "Ce logiciel est basé en partie sur les travaux de l'Independent JPEG Group."],
  ["Tauri", "MIT / Apache-2.0"],
  ["React", "MIT"],
  ["rustls, ring", "Apache-2.0 / MIT / ISC"],
  ["mdns-sd, qrcode, serde", "MIT / Apache-2.0"],
  ["Inter, Space Grotesk", "OFL-1.1", "Polices incluses dans l'application."],
];
