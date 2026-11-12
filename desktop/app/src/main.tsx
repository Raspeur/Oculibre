import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
// Fonts shipped with the app (OFL-1.1): nothing is downloaded when it opens.
import "@fontsource-variable/inter";
import "@fontsource-variable/space-grotesk";
// The look of every screen: tokens and components shared with the mockups (design/mockups).
import "../../../design/mockups/ui.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
