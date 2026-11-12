// The main window (mockup design/mockups/Main2): phones on the left, the phone being served in
// the middle, its settings on the right.

import { useEffect, useState } from "react";
import { Pairing } from "./components/Pairing";
import { Panel } from "./components/Panel";
import { Settings } from "./components/Settings";
import { Sidebar } from "./components/Sidebar";
import { Stage } from "./components/Stage";
import { inApp, onNavigate } from "./engine";
import { useEngine } from "./useEngine";

function App() {
  const engine = useEngine();
  // Which screen is shown: a few screens, no router library needed.
  const [screen, setScreen] = useState<"main" | "pairing" | "settings" | "about">("main");

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
      />
      <main className="oc-main">
        <div className="oc-stack" style={{ flex: "999 1 520px", minWidth: 0 }}>
          {!inApp && (
            <Notice level="info" title="Aperçu dans un navigateur">
              Seule la mise en page s'affiche ici : pour piloter le téléphone, ouvrez la fenêtre
              d'Oculibre (npm run tauri dev).
            </Notice>
          )}
          {engine.startError && (
            <Notice level="error" title="Oculibre ne peut pas attendre les téléphones" code={startCode(engine.startError)}>
              {engine.startError}
            </Notice>
          )}
          <Stage engine={engine} />
        </div>
        <Panel engine={engine} />
      </main>
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
        {props.code && <span className="oc-alert__code">Code {props.code}</span>}
      </div>
    </div>
  );
}

/** Problem codes of the mockups (design/mockups/Problems): 103 another Oculibre, 401 no virtual camera. */
function startCode(error: string): string | undefined {
  if (error.includes("already used")) return "103";
  if (error.toLowerCase().includes("v4l2loopback") || error.includes("/dev/video")) return "401";
  return undefined;
}

export default App;
