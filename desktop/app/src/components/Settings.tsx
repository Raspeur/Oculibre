// Settings of this computer (mockup design/mockups/Settings): virtual camera, phones and Wi-Fi,
// performance, about; general: language, start with the computer, stay in the tray.

import { getVersion } from "@tauri-apps/api/app";
import { useEffect, useRef, useState } from "react";
import logo from "../../../../oculibre_logo.svg";
import {
  type AppSettings,
  type DecoderChoice,
  type KnownPhone,
  autostart as loadAutostart,
  setAutostart,
  when,
  forgetPhone,
  inApp,
  setSettings,
  settings as loadSettings,
} from "../engine";
import type { EngineState } from "../useEngine";
import { LANGUAGES, setLanguage, t, useLang } from "../i18n";

export function Settings(props: { engine: EngineState; section?: "about"; onDone: () => void }) {
  useLang();
  const { engine } = props;
  const [values, setValues] = useState<AppSettings | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState<string>("");
  const [startsWithComputer, setStartsWithComputer] = useState(false);
  const about = useRef<HTMLElement>(null);

  useEffect(() => {
    loadSettings().then(setValues);
    loadAutostart().then(setStartsWithComputer);
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
          ? t("applied_later")
          : null,
      );
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="oc-app" style={{ display: "flex", flexDirection: "column" }}>
      <header className="oc-header">
        <button type="button" className="oc-icon-btn" aria-label={t("back")} onClick={props.onDone}>
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M15 18l-6-6 6-6" />
          </svg>
        </button>
        <h1 className="oc-title oc-title--sm">{t("settings")}</h1>
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
              <span className="oc-alert__title">{t("not_saved")}</span>
              <span className="oc-alert__text">{error}</span>
            </div>
          </div>
        )}

        <Section id="s-general" title={t("general")}>
          <div className="oc-row">
            <label htmlFor="lang" className="oc-row__label">{t("language")}</label>
            <select
              id="lang"
              className="oc-select oc-select--wide"
              disabled={!values}
              value={values?.language ?? ""}
              onChange={(e) => {
                if (!values) return;
                const language = e.currentTarget.value;
                setLanguage(language); // the window at once; the tray menu when saved
                change({ ...values, language });
              }}
            >
              <option value="">{t("language_system")}</option>
              {LANGUAGES.map(([code, name]) => (
                <option key={code} value={code}>{name}</option>
              ))}
            </select>
          </div>
          <div className="oc-row">
            <span className="oc-row__label">
              <span id="autostart-label">{t("autostart")}</span>
              <span className="oc-row__hint">{t("autostart_hint")}</span>
            </span>
            <button
              type="button"
              className="oc-switch"
              aria-pressed={startsWithComputer}
              aria-labelledby="autostart-label"
              disabled={!inApp}
              onClick={() => {
                const next = !startsWithComputer;
                setAutostart(next).then(
                  () => setStartsWithComputer(next),
                  (e) => setError(String(e)),
                );
              }}
            >
              <span className="oc-switch__knob" aria-hidden="true" />
            </button>
          </div>
          <div className="oc-row">
            <span className="oc-row__label">
              <span id="tray-label">{t("stay_in_tray")}</span>
              <span className="oc-row__hint">{t("stay_in_tray_hint")}</span>
            </span>
            <button
              type="button"
              className="oc-switch"
              aria-pressed={values?.tray ?? true}
              aria-labelledby="tray-label"
              disabled={!values}
              onClick={() => values && change({ ...values, tray: !values.tray })}
            >
              <span className="oc-switch__knob" aria-hidden="true" />
            </button>
          </div>
        </Section>

        <Section id="s-vcam" title={t("virtual_camera")}>
          <div className="oc-row">
            <span className="oc-row__label">{t("status")}</span>
            {engine.running ? (
              <span className="oc-status oc-strong">
                <span className="oc-dot" style={{ width: 7, height: 7 }} aria-hidden="true" />
                {t("installed")}
              </span>
            ) : (
              <span className="oc-row__value oc-row__value--todo">{t("unavailable")}</span>
            )}
          </div>
          <div className="oc-row">
            <span className="oc-row__label">{t("name_in_apps")}</span>
            <span className="oc-row__value">Oculibre</span>
          </div>
          <div className="oc-row">
            <span className="oc-row__label">
              <label htmlFor="shape">{t("shape")}</label>
              <span className="oc-row__hint">{t("shape_hint")}</span>
            </span>
            <select
              id="shape"
              className="oc-select oc-select--wide"
              disabled={!values}
              value={values?.portrait ? "portrait" : "landscape"}
              onChange={(e) => values && change({ ...values, portrait: e.currentTarget.value === "portrait" })}
            >
              <option value="landscape">{t("landscape")}</option>
              <option value="portrait">{t("portrait")}</option>
            </select>
          </div>
        </Section>

        <Section id="s-net" title={t("phones_wifi")}>
          <div className="oc-row">
            <span className="oc-row__label">
              <span id="wifi-label">{t("accept_wifi")}</span>
              <span className="oc-row__hint">{t("accept_wifi_hint")}</span>
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
          {engine.known.length === 0 ? (
            <div className="oc-row">
              <span className="oc-row__label">{t("known_phones")}</span>
              <span className="oc-row__value oc-row__value--muted">{t("none")}</span>
            </div>
          ) : (
            engine.known.map((p) => <KnownPhoneRow key={p.id} phone={p} onError={setError} />)
          )}
          <div className="oc-row">
            <span className="oc-row__label">
              {t("this_computer_code")}
              <span className="oc-row__hint">{t("computer_code_hint")}</span>
            </span>
            <span className="oc-row__value" style={{ letterSpacing: 1 }}>{engine.key ?? "—"}</span>
          </div>
        </Section>

        <Section id="s-perf" title={t("performance")}>
          <div className="oc-row">
            <span className="oc-row__label">
              <label htmlFor="dec">{t("video_decoding")}</label>
              <span className="oc-row__hint">
                {engine.decoder ? t("decoder_used", { name: engine.decoder }) : ""}{t("decoder_next")}
              </span>
            </span>
            <select
              id="dec"
              className="oc-select oc-select--wide"
              disabled={!values}
              value={values?.decoder ?? "auto"}
              onChange={(e) => values && change({ ...values, decoder: e.currentTarget.value as DecoderChoice })}
            >
              <option value="auto">{t("automatic")}</option>
              <option value="vaapi">{t("decoder_gpu")}</option>
              <option value="nvdec">{t("decoder_nvidia")}</option>
              <option value="software">{t("decoder_cpu")}</option>
              <option value="openh264">{t("decoder_openh264")}</option>
            </select>
          </div>
        </Section>

        <section id="about" ref={about} aria-labelledby="s-about" className="oc-stack">
          <h2 id="s-about" className="oc-label" style={{ padding: "0 4px" }}>{t("about")}</h2>
          <div className="oc-card oc-card--padded oc-cluster" style={{ gap: 16 }}>
            <div className="oc-logo oc-logo--lg">
              <img src={logo} alt="" />
            </div>
            <div className="oc-stack" style={{ flex: "1 1 260px", gap: 3 }}>
              <span className="oc-title" style={{ fontSize: "var(--oc-text-xl)" }}>Oculibre {version}</span>
              <span className="oc-muted" style={{ fontSize: "var(--oc-text-md)" }}>
                {t("about_text")}
              </span>
            </div>
          </div>
          <div className="oc-card">
            <div className="oc-row" style={{ minHeight: 44 }}>
              <span className="oc-label">{t("licences")}</span>
            </div>
            {notices().map(([name, licence, note]) => (
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

/**
 * A known phone, with "Forget" asked twice. Forgetting erases its settings; a phone paired for
 * Wi-Fi also needs a new pairing to come back over Wi-Fi. USB keeps working (the cable is trusted).
 */
function KnownPhoneRow({ phone, onError }: { phone: KnownPhone; onError: (e: string) => void }) {
  const [confirming, setConfirming] = useState(false);
  const how = phone.fingerprint
    ? phone.paired_on
      ? t("paired_wifi_on", { date: when(phone.paired_on) })
      : t("paired_wifi")
    : t("usb_cable");
  const seen = phone.last_seen ? t("seen_on", { date: when(phone.last_seen) }) : "";
  const warning = phone.fingerprint ? t("forget_warning_wifi") : t("forget_warning_usb");
  return (
    <div className="oc-row">
      <span className="oc-row__label">
        {phone.name}
        <span className="oc-row__hint">{confirming ? warning : how + seen}</span>
      </span>
      {confirming ? (
        <span className="oc-cluster" style={{ gap: 8 }}>
          <button type="button" className="oc-btn oc-btn--ghost" onClick={() => setConfirming(false)}>{t("cancel")}</button>
          <button
            type="button"
            className="oc-btn oc-btn--danger"
            onClick={() => forgetPhone(phone).catch((e) => onError(String(e)))}
          >
            {t("forget")}
          </button>
        </span>
      ) : (
        <button type="button" className="oc-btn oc-btn--secondary" onClick={() => setConfirming(true)}>{t("forget_ask")}</button>
      )}
    </div>
  );
}

/** Notices the licences of the components ask for (see docs/research/video-decoding.typ). */
const notices = (): [string, string, string?][] => [
  ["FFmpeg (libavcodec)", "LGPL-2.1+ / GPL-2+", t("notice_ffmpeg")],
  ["OpenH264", "BSD-2-Clause", "© Cisco Systems."],
  ["jpeg-encoder", "MIT / Apache-2.0, IJG", t("notice_jpeg")],
  ["Tauri", "MIT / Apache-2.0"],
  ["React", "MIT"],
  ["rustls, ring", "Apache-2.0 / MIT / ISC"],
  ["mdns-sd, qrcode, serde", "MIT / Apache-2.0"],
  ["Inter, Space Grotesk", "OFL-1.1", t("notice_fonts")],
];
