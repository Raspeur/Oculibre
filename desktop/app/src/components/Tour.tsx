// Guided tour of the main window (mockup design/mockups/Main2, "Visite guidée"): a welcome, five
// steps that light up a part of the window, an end that leads to "add a phone".
//
// The parts are found by their `data-tour` attribute: the other components only carry it (several
// parts with the same value are lit together). The light is a box placed over them
// (getBoundingClientRect), whose huge shadow dims the rest.

import { useEffect, useLayoutEffect, useState } from "react";
import logo from "../../../../oculibre_logo.svg";

interface Step {
  target: string;
  /** Where the bubble goes: right of the part, or left of it. */
  side: "right" | "left";
  title: string;
  text: string;
}

const steps: Step[] = [
  {
    target: "add",
    side: "right",
    title: "Reliez votre téléphone",
    text: "Par câble USB, rien à faire : branchez-le et appuyez sur « Démarrer » dans l'app du téléphone. Pour le Wi-Fi, reliez-le une seule fois ici, avec un QR code.",
  },
  {
    target: "phones",
    side: "right",
    title: "Vos téléphones",
    text: "Point vert : en direct. Cercle gris : arrêté ou trop loin. Oculibre se souvient des réglages de chacun.",
  },
  {
    target: "stage",
    side: "left",
    title: "Démarrer depuis le téléphone",
    text: "Le téléphone filme quand vous appuyez sur « Démarrer » dans son app. Ici, vous voyez ce que reçoivent les applications.",
  },
  {
    target: "source",
    side: "left",
    title: "Caméra, qualité et image",
    text: "Arrière ou avant, résolution, images par seconde, rotation, miroir : tout se règle ici, sans toucher au téléphone. Oculibre s'en souvient pour ce téléphone.",
  },
  {
    target: "output",
    side: "left",
    title: "Dans vos applications",
    text: "Dans Zoom, Teams, Discord ou OBS, choisissez la caméra « Oculibre ». Fermer cette fenêtre la garde disponible.",
  },
];

const BUBBLE_WIDTH = 340;
/** Secondary actions of the bubble: plain text, never on two lines. */
const quiet = { padding: "0 8px", whiteSpace: "nowrap" } as const;
const GAP = 18;

/** Whether the tour was already shown (or skipped) on this computer. */
export function tourSeen(): boolean {
  try {
    return localStorage.getItem("oculibre.tour") === "done";
  } catch {
    return true; // no storage: don't insist at every start
  }
}

function markSeen() {
  try {
    localStorage.setItem("oculibre.tour", "done");
  } catch {
    // not remembered
  }
}

/** `step`: 0 welcome, 1–5 the parts, 6 the end. */
export function Tour(props: { onClose: () => void; onAddPhone: () => void }) {
  const [step, setStep] = useState(0);
  const [rect, setRect] = useState<DOMRect | null>(null);
  const current = step >= 1 && step <= steps.length ? steps[step - 1] : null;

  // Find the part to light up, and follow it when the window changes size.
  useLayoutEffect(() => {
    if (!current) {
      setRect(null);
      return;
    }
    const measure = () => {
      // Every part with this name, joined into one box.
      const boxes = [...document.querySelectorAll(`[data-tour="${current.target}"]`)].map((e) =>
        e.getBoundingClientRect(),
      );
      if (boxes.length === 0) {
        setRect(null);
        return;
      }
      const left = Math.min(...boxes.map((b) => b.left));
      const top = Math.min(...boxes.map((b) => b.top));
      const right = Math.max(...boxes.map((b) => b.right));
      const bottom = Math.max(...boxes.map((b) => b.bottom));
      setRect(new DOMRect(left, top, right - left, bottom - top));
    };
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [current]);

  // Escape closes the tour.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  function close() {
    markSeen();
    props.onClose();
  }

  if (step === 0 || step > steps.length) {
    const end = step > steps.length;
    return (
      <div className="oc-scrim">
        <div className="oc-dialog" role="dialog" aria-labelledby="tour-title" style={{ alignItems: "center", textAlign: "center" }}>
          {end ? (
            <div className="oc-hero-icon oc-hero-icon--ok" style={{ width: 64, height: 64 }}>
              <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="#3DDC97" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
                <path d="M20 6L9 17l-5-5" />
              </svg>
            </div>
          ) : (
            <div className="oc-logo oc-logo--xl">
              <img src={logo} alt="" />
            </div>
          )}
          <h2 id="tour-title" className="oc-title oc-title--md">{end ? "C'est tout !" : "Bienvenue dans Oculibre"}</h2>
          <p className="oc-text oc-muted">
            {end
              ? "Première chose à faire : brancher votre téléphone, ou le relier en Wi-Fi. La visite reste disponible dans la barre de gauche."
              : "Votre téléphone devient la webcam de cet ordinateur. Une visite d'une minute pour savoir où cliquer ?"}
          </p>
          <div className="oc-cluster" style={{ justifyContent: "center", paddingTop: 6 }}>
            <button type="button" className="oc-btn oc-btn--ghost" onClick={close}>
              {end ? "Fermer" : "Plus tard"}
            </button>
            {end ? (
              <button type="button" className="oc-btn oc-btn--primary" onClick={() => { close(); props.onAddPhone(); }}>
                Ajouter un téléphone
              </button>
            ) : (
              <button type="button" className="oc-btn oc-btn--primary" autoFocus onClick={() => setStep(1)}>
                Commencer la visite
              </button>
            )}
          </div>
        </div>
      </div>
    );
  }

  // The light over the part, cut to what the window shows (a column can be taller than the
  // window), and the bubble beside it, kept inside the window too.
  const measured = rect ?? new DOMRect(window.innerWidth / 2, window.innerHeight / 2, 0, 0);
  const edge = 8;
  const lightTop = Math.max(measured.top - 4, edge);
  const lightLeft = Math.max(measured.left - 4, edge);
  const lightBottom = Math.min(measured.bottom + 4, window.innerHeight - edge);
  const lightRight = Math.min(measured.right + 4, window.innerWidth - edge);
  const r = new DOMRect(lightLeft + 4, lightTop + 4, lightRight - lightLeft - 8, lightBottom - lightTop - 8);
  const left =
    current!.side === "right"
      ? Math.min(r.right + GAP, window.innerWidth - BUBBLE_WIDTH - 8)
      : Math.max(r.left - GAP - BUBBLE_WIDTH, 8);
  const top = Math.max(8, Math.min(r.top, window.innerHeight - 240));
  return (
    <>
      {/* Catches the clicks: the window behind waits for the tour. */}
      <div style={{ position: "fixed", inset: 0, zIndex: 19 }} onClick={() => {}} />
      <div
        aria-hidden="true"
        style={{
          position: "fixed",
          top: lightTop,
          left: lightLeft,
          width: lightRight - lightLeft,
          height: lightBottom - lightTop,
          borderRadius: "var(--oc-r-md)",
          outline: "2px solid var(--oc-accent)",
          boxShadow: "0 0 0 9999px var(--oc-scrim)",
          pointerEvents: "none",
          zIndex: 20,
          transition: "all 200ms ease",
        }}
      />
      <div
        className={`oc-coach oc-coach--${current!.side}`}
        role="dialog"
        aria-label="Visite guidée"
        style={{ position: "fixed", left, top, right: "auto", bottom: "auto", width: BUBBLE_WIDTH }}
      >
        <span className="oc-coach__step">Étape {step} sur {steps.length}</span>
        <h2 className="oc-coach__title">{current!.title}</h2>
        <p className="oc-coach__text">{current!.text}</p>
        <div className="oc-coach__actions">
          <button type="button" className="oc-btn oc-btn--ghost" style={quiet} onClick={close}>
            Passer la visite
          </button>
          <span className="oc-cluster" style={{ gap: 4, flexWrap: "nowrap" }}>
            {step > 1 && (
              <button type="button" className="oc-btn oc-btn--ghost" style={quiet} onClick={() => setStep(step - 1)}>
                Retour
              </button>
            )}
            <button type="button" className="oc-btn oc-btn--primary" style={{ whiteSpace: "nowrap" }} autoFocus onClick={() => setStep(step + 1)}>
              {step === steps.length ? "Terminer" : "Suivant"}
            </button>
          </span>
        </div>
      </div>
    </>
  );
}
