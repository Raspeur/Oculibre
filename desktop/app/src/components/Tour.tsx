// Guided tour of the main window (mockup design/mockups/Main2, "Visite guidée"): a welcome, five
// steps that light up a part of the window, an end that leads to "add a phone".
//
// The parts are found by their `data-tour` attribute: the other components only carry it (several
// parts with the same value are lit together). The light is a box placed over them
// (getBoundingClientRect), whose huge shadow dims the rest.

import { useEffect, useLayoutEffect, useState } from "react";
import logo from "../../../../oculibre_logo.svg";
import { t, useLang } from "../i18n";

interface Step {
  target: string;
  /** Where the bubble goes: right of the part, or left of it. */
  side: "right" | "left";
  title: string;
  text: string;
}

/** In the current language: a function, not a constant made once at load. */
const steps = (): Step[] => [
  {
    target: "add",
    side: "right",
    title: t("tour1_title"),
    text: t("tour1_text"),
  },
  {
    target: "phones",
    side: "right",
    title: t("tour2_title"),
    text: t("tour2_text"),
  },
  {
    target: "stage",
    side: "left",
    title: t("tour3_title"),
    text: t("tour3_text"),
  },
  {
    target: "source",
    side: "left",
    title: t("tour4_title"),
    text: t("tour4_text"),
  },
  {
    target: "output",
    side: "left",
    title: t("tour5_title"),
    text: t("tour5_text"),
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
  useLang();
  const [step, setStep] = useState(0);
  const [rect, setRect] = useState<DOMRect | null>(null);
  const all = steps();
  const current = step >= 1 && step <= all.length ? all[step - 1] : null;

  // Find the part to light up, and follow it when the window changes size.
  // Keyed on the target's name: `steps()` makes new objects at each drawing, `current` itself
  // would run this again every time (and setRect would draw again: an endless loop).
  const target = current?.target;
  useLayoutEffect(() => {
    if (!target) {
      setRect(null);
      return;
    }
    const measure = () => {
      // Every part with this name, joined into one box.
      const boxes = [...document.querySelectorAll(`[data-tour="${target}"]`)].map((e) =>
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
  }, [target]);

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

  if (step === 0 || step > all.length) {
    const end = step > all.length;
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
          <h2 id="tour-title" className="oc-title oc-title--md">{end ? t("tour_end_title") : t("tour_welcome_title")}</h2>
          <p className="oc-text oc-muted">
            {end ? t("tour_end_text") : t("tour_welcome_text")}
          </p>
          <div className="oc-cluster" style={{ justifyContent: "center", paddingTop: 6 }}>
            <button type="button" className="oc-btn oc-btn--ghost" onClick={close}>
              {end ? t("close") : t("later")}
            </button>
            {end ? (
              <button type="button" className="oc-btn oc-btn--primary" onClick={() => { close(); props.onAddPhone(); }}>
                {t("add_phone")}
              </button>
            ) : (
              <button type="button" className="oc-btn oc-btn--primary" autoFocus onClick={() => setStep(1)}>
                {t("tour_start")}
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
        aria-label={t("tour")}
        style={{ position: "fixed", left, top, right: "auto", bottom: "auto", width: BUBBLE_WIDTH }}
      >
        <span className="oc-coach__step">{t("tour_step", { n: step, total: all.length })}</span>
        <h2 className="oc-coach__title">{current!.title}</h2>
        <p className="oc-coach__text">{current!.text}</p>
        <div className="oc-coach__actions">
          <button type="button" className="oc-btn oc-btn--ghost" style={quiet} onClick={close}>
            {t("tour_skip")}
          </button>
          <span className="oc-cluster" style={{ gap: 4, flexWrap: "nowrap" }}>
            {step > 1 && (
              <button type="button" className="oc-btn oc-btn--ghost" style={quiet} onClick={() => setStep(step - 1)}>
                {t("back")}
              </button>
            )}
            <button type="button" className="oc-btn oc-btn--primary" style={{ whiteSpace: "nowrap" }} autoFocus onClick={() => setStep(step + 1)}>
              {step === all.length ? t("done") : t("next")}
            </button>
          </span>
        </div>
      </div>
    </>
  );
}
