// The main window (mockup design/mockups/Main2): phones on the left, the phone being served in
// the middle, its settings on the right.

import { useEffect, useState } from "react";
import { Pairing } from "./components/Pairing";
import { Panel } from "./components/Panel";
import { Settings } from "./components/Settings";
import { Sidebar } from "./components/Sidebar";
import { Stage } from "./components/Stage";
import { Tour, tourSeen } from "./components/Tour";
import { VcamProblem } from "./components/VcamProblem";
import { inApp, onNavigate, settings as loadSettings } from "./engine";
import { setLanguage, t, useLang } from "./i18n";
import { useEngine } from "./useEngine";

function App() {
  useLang();
  const engine = useEngine();
  // Which screen is shown: a few screens, no router library needed.
  const [screen, setScreen] = useState<"main" | "pairing" | "settings" | "about">("main");
  // The guided tour opens by itself the first time, then from the sidebar.
  const [touring, setTouring] = useState(() => inApp && !tourSeen());

  // The language chosen in the settings ("" = the system's).
  useEffect(() => {
    loadSettings().then((s) => setLanguage(s.language));
  }, []);

  // "Add a phone…" in the tray menu.
  useEffect(() => {
    const unlisten = onNavigate((to) => {
      if (to === "pairing") setScreen("pairing");
    });
    return () => {
      unlisten.then((stopListening) => stopListening());
    };
  }, []);

  if (screen === "pairing") {
    return <Pairing computerKey={engine.key} phoneLive={engine.phone !== null} onDone={() => setScreen("main")} />;
  }
  if (screen === "settings" || screen === "about") {
    return (
      <Settings
        engine={engine}
        section={screen === "about" ? "about" : undefined}
        onDone={() => setScreen("main")}
      />
    );
  }
  return (
    <div className="oc-app oc-app--sidebar">
      <Sidebar
        engine={engine}
        onAddPhone={() => setScreen("pairing")}
        onSettings={() => setScreen("settings")}
        onAbout={() => setScreen("about")}
        onTour={() => setTouring(true)}
      />
      <main className="oc-main">
        <div className="oc-stack" style={{ flex: "999 1 520px", minWidth: 0 }}>
          {!inApp && (
            <Notice level="info" title={t("browser_title")}>
              {t("browser_text")}
            </Notice>
          )}
          {engine.startError && vcamMissing(engine.startError) && (
            <VcamProblem style={{ margin: "24px 24px 0" }} />
          )}
          {engine.startError && !vcamMissing(engine.startError) && (
            <Notice level="error" title={t("start_error_title")} code={startCode(engine.startError)}>
              {engine.startError}
            </Notice>
          )}
          <Stage engine={engine} />
        </div>
        <Panel engine={engine} />
      </main>
      {touring && <Tour onClose={() => setTouring(false)} onAddPhone={() => setScreen("pairing")} />}
    </div>
  );
}

/** An alert at the top of the centre column (ui.css .oc-alert). */
function Notice(props: { level: "info" | "error"; title: string; code?: string; children: React.ReactNode }) {
  return (
    <div className={`oc-alert oc-alert--${props.level}`} role="alert" style={{ margin: "24px 24px 0" }}>
      <span className="oc-alert__icon" aria-hidden="true">{props.level === "error" ? "×" : "i"}</span>
      <div className="oc-alert__body">
        <span className="oc-alert__title">{props.title}</span>
        <span className="oc-alert__text">{props.children}</span>
        {props.code && <span className="oc-alert__code">{t("code", { code: props.code })}</span>}
      </div>
    </div>
  );
}

/** No virtual camera at all (core v4l2::find_loopback): one click sets it up. */
function vcamMissing(error: string): boolean {
  return error.includes("no v4l2loopback device");
}

/** Problem codes of the mockups (design/mockups/Problems): 103 another Oculibre, 401 no virtual camera. */
function startCode(error: string): string | undefined {
  if (error.includes("already used")) return "103";
  if (error.toLowerCase().includes("v4l2loopback") || error.includes("/dev/video")) return "401";
  return undefined;
}

export default App;
