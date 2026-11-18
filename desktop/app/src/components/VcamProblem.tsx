// The virtual camera is missing (mockup design/mockups/Problems, 4xx): what it means, and one
// button that sets it up with the computer's password (polkit asks, the app never sees it).
// Shown instead of the start error in the main window, and in Settings › Virtual camera.

import { useEffect, useState } from "react";
import { type FixError, type VcamStatus, fixVcam, onWindows, vcamStatus } from "../engine";
import { type Key, t, useLang } from "../i18n";

export function VcamProblem(props: { style?: React.CSSProperties; onFixed?: () => void }) {
  useLang();
  const [status, setStatus] = useState<VcamStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<FixError | null>(null);

  useEffect(() => {
    vcamStatus().then(setStatus);
  }, []);

  async function fix() {
    setBusy(true);
    setError(null);
    try {
      await fixVcam(); // the receiver starts again by itself: this alert goes away
      setStatus(await vcamStatus());
      props.onFixed?.();
    } catch (e) {
      const why = e as FixError;
      if (why.kind !== "Cancelled") setError(why); // closing the password window is not an error
    } finally {
      setBusy(false);
    }
  }

  // What to say: the outcome of the last try first, else what the computer looks like.
  const problem: [code: number, title: Key, text: Key | null] =
    error?.kind === "SecureBoot" || (!error && status?.kind === "SecureBoot")
      ? [403, "vcam_secureboot_title", "vcam_secureboot_text"]
      : error?.kind === "InUse"
        ? [405, "vcam_inuse_title", "vcam_inuse_text"]
        : error?.kind === "NotInstalled" || status?.kind === "NotInstalled"
          ? [404, "vcam_module_title", onWindows ? "vcam_module_text_windows" : "vcam_module_text"]
          : error?.kind === "Failed"
            ? [401, "vcam_failed_title", null]
            : [401, "vcam_missing_title", onWindows ? "vcam_missing_text_windows" : "vcam_missing_text"];
  const [code, title, text] = problem;
  // A restart is the only fix for 405; Secure Boot needs a terminal and a restart.
  const canTry = code === 401 || code === 404;

  return (
    <div className="oc-alert oc-alert--error" role="alert" style={props.style}>
      <span className="oc-alert__icon" aria-hidden="true">×</span>
      <div className="oc-alert__body">
        <span className="oc-alert__title">{t(title)}</span>
        <span className="oc-alert__text">{text ? t(text) : error?.kind === "Failed" ? error.data : ""}</span>
        {canTry && (
          <div className="oc-alert__actions">
            <button type="button" className="oc-btn oc-btn--primary" disabled={busy} onClick={fix}>
              {busy ? t("vcam_installing") : code === 404 ? t("retry") : t("vcam_install")}
            </button>
          </div>
        )}
        <span className="oc-alert__code">{t("code", { code })}</span>
      </div>
    </div>
  );
}
