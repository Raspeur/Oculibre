// Left column of the main window (mockup Main2): the logo, the phones, adding a phone.

import logo from "../../../../oculibre_logo.svg";
import { phoneName } from "../engine";
import type { EngineState } from "../useEngine";

export function Sidebar(props: {
  engine: EngineState;
  onAddPhone: () => void;
  onSettings: () => void;
  onAbout: () => void;
  onTour: () => void;
}) {
  const { engine, onAddPhone } = props;
  const { phone, via, stream, paused } = engine;
  // Known phones other than the one being served are offline (or not started). A phone gets a
  // new random identity when its app data is wiped: the older USB-only identities with the same
  // name are left to the settings (to forget them), here only the most recent one shows.
  const live = phone ? phoneName(phone) : null;
  const shownNames = new Set<string>();
  const others = engine.known.filter((p) => {
    if (p.id === phone?.id) return false;
    if (p.fingerprint) return true; // paired for Wi-Fi: always
    if (p.name === live || shownNames.has(p.name)) return false;
    shownNames.add(p.name); // the list comes most recent first
    return true;
  });
  const link = via?.kind === "Wifi" ? "Wi-Fi" : "USB";

  return (
    <nav className="oc-sidebar" aria-label="Oculibre">
      <div className="oc-cluster">
        <div className="oc-logo oc-logo--md">
          <img src={logo} alt="" />
        </div>
        <span className="oc-brand">Oculibre</span>
      </div>

      <div className="oc-stack">
        <span className="oc-label">Téléphones</span>
        <div className="oc-stack" data-tour="phones">

        {phone && (
          <button type="button" className="oc-nav-item" aria-current="true">
            <span
              className={stream && !paused ? "oc-dot oc-dot--lg" : "oc-dot oc-dot--lg oc-dot--paused"}
              aria-hidden="true"
            />
            <span className="oc-row__label">
              <span className="oc-nav-item__name">{phoneName(phone)}</span>
              <span className="oc-small">
                {stream && !paused ? "En direct" : "Connecté"} · {link}
              </span>
            </span>
          </button>
        )}

        {others.map((p) => (
          <button key={p.id} type="button" className="oc-nav-item">
            <span className="oc-dot oc-dot--lg oc-dot--off" aria-hidden="true" />
            <span className="oc-row__label">
              <span className="oc-nav-item__name">{p.name}</span>
              <span className="oc-small">Hors ligne · {p.last_via === "usb" ? "USB" : "Wi-Fi"}</span>
            </span>
          </button>
        ))}

        {!phone && others.length === 0 && (
          <p className="oc-small" style={{ margin: "0 4px" }}>
            Aucun téléphone pour l'instant.
          </p>
        )}

        </div>

        <button
          type="button"
          className="oc-nav-add"
          data-tour="add"
          disabled={!engine.running}
          title={engine.running ? undefined : "Oculibre n'attend pas encore les téléphones"}
          onClick={onAddPhone}
        >
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="#FE633C" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
            <path d="M12 5v14M5 12h14" />
          </svg>
          Ajouter un téléphone
        </button>
      </div>

      <div className="oc-stack" style={{ marginTop: "auto", gap: 2 }}>
        <button type="button" className="oc-nav-link" onClick={props.onSettings}>
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <path d="M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1 14h6M9 8h6M17 16h6" />
          </svg>
          Réglages
        </button>
        <button type="button" className="oc-nav-link" onClick={props.onTour}>
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <circle cx="12" cy="12" r="10" />
            <path d="M9.1 9a3 3 0 0 1 5.8 1c0 2-3 3-3 3M12 17h.01" />
          </svg>
          Visite guidée
        </button>
        <button type="button" className="oc-nav-link" onClick={props.onAbout}>
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
            <circle cx="12" cy="12" r="10" />
            <path d="M12 16v-4M12 8h.01" />
          </svg>
          À propos
        </button>
      </div>
    </nav>
  );
}
