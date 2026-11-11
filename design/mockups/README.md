# UI mockups

Clickable mockups of the desktop app (#15) and the Android app (#16), made for #14.
This folder is the source of truth; the online canvas is only a viewer, republished from here.

| File | Screen |
|---|---|
| `ui.css` | Shared tokens (colours, type, radii, sizes) and components. Change the look here. |
| `UIKit.dc.html` | Every token and component of `ui.css` on one page |
| `Main2.dc.html` | PC: main window (phones, preview, camera settings, Start/Stop) |
| `Pairing-fts1.dc.html` | PC: add a phone (steps + QR code) |
| `Settings.dc.html` | PC: settings and About |
| `Tray-vy8x.dc.html` | PC: tray menu |
| `PhoneWelcomeA.dc.html` | Phone: first launch, one step per screen |
| `PhoneHome.dc.html` | Phone: everyday screen |
| `PhoneWelcomeB.dc.html` | Phone: settings with the checks list |
| `canvas.json` | Board positions and titles on the canvas |
| `oculibre_logo.svg` | Copy of the logo at the repository root |

## Conventions

- Tokens are CSS variables named `--oc-*`; classes follow BEM: `.oc-btn`, `.oc-btn--primary`,
  `.oc-row__label`.
- States use ARIA attributes, not extra classes: `aria-pressed="true"` (switch, segment, big
  button), `aria-current="true"` (selected phone).
- Orange (`--oc-accent`) only for the primary action or the active element; green
  (`--oc-live`) for live and OK.
- Inline `style` is kept for one-off layout (widths, gaps), never for colours or fonts.

## File format

The screens are `.dc.html` files for the claude.ai design canvas: plain HTML inside `<x-dc>`,
with `{{value}}` holes, `<sc-for>` / `<sc-if>` and a small `DCLogic` class for the clickable
state. `support.js` is provided by the canvas and is not in this folder, so the files don't
render on their own in a browser. The real apps take their values from `ui.css`.
