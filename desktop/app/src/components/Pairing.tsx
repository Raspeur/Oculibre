// "Add a phone" (mockup design/mockups/Pairing-fts1). Over the USB cable there is nothing to do
// (the cable is trusted); the QR code pairs a phone for Wi-Fi: a one-time code to scan, the code
// to compare, then "<phone> is ready". Problem 201 when the code expires. With Wi-Fi off in the
// settings, a card offers to turn it on instead of the QR code.

import { useEffect, useRef, useState } from "react";
import { type AppSettings, type PairingOffer, onEngineEvent, pairingOffer, setSettings, settings as loadSettings } from "../engine";

export function Pairing(props: { computerKey: string | null; phoneLive: boolean; onDone: () => void }) {
  const { computerKey, onDone } = props;
  const [values, setValues] = useState<AppSettings | null>(null);
  const [turningOn, setTurningOn] = useState(false);
  const [offer, setOffer] = useState<PairingOffer | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [secondsLeft, setSecondsLeft] = useState(0);
  const [paired, setPaired] = useState<string | null>(null);

  async function newOffer() {
    setError(null);
    try {
      const o = await pairingOffer();
      setOffer(o);
      setSecondsLeft(o.ttl_seconds);
    } catch (e) {
      setError(String(e));
    }
  }

  // A first offer when the screen opens, if Wi-Fi is on. Once only: while developing, React runs
  // effects twice (StrictMode), and a second offer would cancel the first (and invite a USB phone
  // twice).
  const asked = useRef(false);
  useEffect(() => {
    if (asked.current) return;
    asked.current = true;
    loadSettings().then((s) => {
      setValues(s);
      if (s.wifi) newOffer();
    });
  }, []);

  /** Wi-Fi on (a new receiver, at once), then the QR code. */
  async function turnWifiOn() {
    if (!values) return;
    setTurningOn(true);
    setError(null);
    try {
      const next = { ...values, wifi: true };
      await setSettings(next, true);
      setValues(next);
      await newOffer();
    } catch (e) {
      setError(String(e));
    } finally {
      setTurningOn(false);
    }
  }

  // The phone that takes the offer is announced by the engine.
  useEffect(() => {
    const unlisten = onEngineEvent((event) => {
      if (event.kind === "Paired") setPaired(event.data.name);
    });
    return () => {
      unlisten.then((stopListening) => stopListening());
    };
  }, []);

  // Count down once a second while the code is valid.
  useEffect(() => {
    if (!offer || secondsLeft <= 0 || paired) return;
    const timer = setTimeout(() => setSecondsLeft((s) => s - 1), 1000);
    return () => clearTimeout(timer);
  }, [offer, secondsLeft, paired]);

  const expired = offer !== null && secondsLeft <= 0;

  return (
    <div className="oc-app oc-app--stack" style={{ paddingBottom: 32, gap: 28 }}>
      <header className="oc-header">
        <button type="button" className="oc-icon-btn" aria-label="Retour" onClick={onDone}>
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M15 18l-6-6 6-6" />
          </svg>
        </button>
        <div className="oc-row__label" style={{ gap: 2 }}>
          <span className="oc-small">Téléphones</span>
          <h1 className="oc-title oc-title--sm">Ajouter un téléphone</h1>
        </div>
      </header>

      {paired ? (
        <section className="oc-card oc-card--raised oc-stack" style={doneStyle}>
          <div className="oc-hero-icon oc-hero-icon--ok" style={{ width: 64, height: 64 }}>
            <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="#3DDC97" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
              <path d="M20 6L9 17l-5-5" />
            </svg>
          </div>
          <h2 className="oc-title" style={{ fontSize: 28 }}>{paired} est prêt</h2>
          <p className="oc-text oc-text--lg oc-muted">
            Appuyez sur « Démarrer » sur le téléphone. La prochaine fois, il suffira d'ouvrir l'app.
          </p>
          <button type="button" className="oc-btn oc-btn--primary" style={{ minHeight: 40, padding: "0 24px" }} onClick={onDone}>
            Terminer
          </button>
        </section>
      ) : (
        <div style={contentStyle}>
          <section className="oc-stack" style={{ flex: "999 1 420px", minWidth: 0, gap: 24 }}>
            <div className="oc-card oc-card--raised oc-cluster" style={{ flexWrap: "nowrap", gap: 14, padding: "14px 16px", borderRadius: "var(--oc-r-sm)" }}>
              <span className="oc-badge oc-badge--ok" aria-hidden="true">✓</span>
              <span className="oc-row__label" style={{ gap: 4 }}>
                <span className="oc-text oc-text--lg oc-strong">Par câble USB : rien à faire</span>
                <span className="oc-text oc-muted">Branchez le téléphone et appuyez sur « Démarrer » dans l'app Oculibre du téléphone.</span>
              </span>
            </div>
            <p className="oc-text oc-text--lg oc-muted">
              Pour l'utiliser aussi en Wi-Fi, reliez-le une seule fois. Ensuite, il se reconnecte tout seul.
            </p>
            <ol className="oc-stack" style={{ margin: 0, padding: 0, listStyle: "none", gap: 14 }}>
              {steps.map((s, i) => (
                <li key={s.title} className="oc-card oc-card--raised" style={stepStyle}>
                  <span className="oc-badge oc-badge--round">{i + 1}</span>
                  <span className="oc-row__label" style={{ gap: 4 }}>
                    <span className="oc-text oc-text--lg oc-strong">{s.title}</span>
                    <span className="oc-text oc-muted">{s.text}</span>
                  </span>
                </li>
              ))}
            </ol>
            <div className="oc-cluster oc-muted" style={{ gap: 10, fontSize: "var(--oc-text-base)" }}>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#7F8EA8" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M12 22V12M12 12l-4-4M12 12l4-4M5 8h14" />
              </svg>
              {offer?.sent_over_usb
                ? "Envoyé aussi au téléphone branché en USB : confirmez sur son écran."
                : "Téléphone branché en USB ? L'invitation lui est envoyée directement."}
            </div>
          </section>

          <section aria-label="QR code d'appairage" className="oc-card oc-card--raised oc-stack" style={qrCardStyle}>
            {values && !values.wifi ? (
              <div className="oc-stack" style={{ alignItems: "center", gap: 12, textAlign: "center" }}>
                <span className="oc-text oc-strong">Le Wi-Fi est désactivé</span>
                <span className="oc-text oc-muted">
                  Oculibre n'accepte pour l'instant que le câble. Activez le Wi-Fi pour relier ce téléphone.
                </span>
                {props.phoneLive && (
                  <span className="oc-small">
                    La vidéo du téléphone en direct va s'interrompre : appuyez ensuite à nouveau sur « Démarrer » sur le téléphone.
                  </span>
                )}
                <button type="button" className="oc-btn oc-btn--primary" disabled={turningOn} onClick={turnWifiOn}>
                  {turningOn ? "Activation…" : "Activer le Wi-Fi"}
                </button>
              </div>
            ) : error ? (
              <div className="oc-alert oc-alert--error" role="alert">
                <span className="oc-alert__icon" aria-hidden="true">×</span>
                <div className="oc-alert__body">
                  <span className="oc-alert__title">Impossible de créer un QR code</span>
                  <span className="oc-alert__text">{error}</span>
                  <div className="oc-alert__actions">
                    <button type="button" className="oc-btn oc-btn--secondary" onClick={newOffer}>Réessayer</button>
                  </div>
                </div>
              </div>
            ) : expired ? (
              <div className="oc-alert" role="alert">
                <span className="oc-alert__icon" aria-hidden="true">!</span>
                <div className="oc-alert__body">
                  <span className="oc-alert__title">Ce QR code a expiré</span>
                  <span className="oc-alert__text">
                    Pour votre sécurité, un QR code ne sert que 10 minutes et pour un seul téléphone.
                  </span>
                  <div className="oc-alert__actions">
                    <button type="button" className="oc-btn oc-btn--primary" onClick={newOffer}>Nouveau QR code</button>
                  </div>
                  <span className="oc-alert__code">Code 201</span>
                </div>
              </div>
            ) : offer ? (
              <QrCode grid={offer.qr} />
            ) : (
              <span className="oc-small">Préparation…</span>
            )}
            <div className="oc-stack" style={{ alignItems: "center", gap: 6 }}>
              <span className="oc-muted" style={{ fontSize: "var(--oc-text-md)" }}>Code de cet ordinateur</span>
              <span className="oc-title oc-title--lg" style={{ letterSpacing: 2 }}>{computerKey ?? "—"}</span>
            </div>
            {offer && !expired && (
              <span className="oc-muted" style={{ fontSize: "var(--oc-text-md)" }}>
                Valable encore {minutes(secondsLeft)} · pour un seul téléphone
              </span>
            )}
          </section>
        </div>
      )}
    </div>
  );
}

/** The QR code as SVG: one square per dark module, on white, with the 4-module quiet zone. */
function QrCode({ grid }: { grid: PairingOffer["qr"] }) {
  const size = grid.width + 8;
  const squares: string[] = [];
  grid.dark.forEach((dark, i) => {
    if (dark) squares.push(`M${(i % grid.width) + 4} ${Math.floor(i / grid.width) + 4}h1v1h-1z`);
  });
  return (
    <svg
      viewBox={`0 0 ${size} ${size}`}
      width={260}
      height={260}
      role="img"
      aria-label="QR code à scanner avec le téléphone"
      shapeRendering="crispEdges"
      style={{ borderRadius: "var(--oc-r-sm)" }}
    >
      <rect width={size} height={size} fill="#FFFFFF" />
      <path d={squares.join("")} fill="#0B1220" />
    </svg>
  );
}

function minutes(seconds: number): string {
  return seconds >= 60 ? `${Math.ceil(seconds / 60)} min` : `${seconds} s`;
}

const steps = [
  {
    title: "Installez Oculibre sur le téléphone",
    text: "Android 5 ou plus récent.",
  },
  {
    title: "Scannez le QR code avec le téléphone",
    text: "Avec l'appareil photo. Le téléphone et l'ordinateur doivent être sur le même Wi-Fi.",
  },
  {
    title: "Vérifiez le code et confirmez sur le téléphone",
    text: "Le téléphone affiche un code : il doit être identique à celui sous le QR code. C'est ce qui garantit que personne d'autre ne s'est glissé entre les deux.",
  },
];

const contentStyle = {
  width: "100%",
  maxWidth: 1040,
  margin: "auto 0",
  padding: "0 24px",
  boxSizing: "border-box",
  display: "flex",
  flexWrap: "wrap",
  gap: 32,
  alignItems: "stretch",
} as const;
const stepStyle = { display: "flex", gap: 14, alignItems: "flex-start", padding: "14px 16px", borderRadius: "var(--oc-r-sm)" } as const;
const qrCardStyle = { flex: "1 1 340px", maxWidth: 400, padding: 24, alignItems: "center", justifyContent: "center", gap: 18 } as const;
const doneStyle = { width: "100%", maxWidth: 560, margin: "auto 0", padding: "40px 32px", alignItems: "center", gap: 16, textAlign: "center" } as const;
