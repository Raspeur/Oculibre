# Oculibre desktop app

The window of Oculibre (#15): [Tauri 2](https://tauri.app) with a React + TypeScript page, over
the same engine as the `oculibre` command (`desktop/core`).

```
app/
  src/                 the page (React + TypeScript, built by Vite)
    main.tsx           entry point; imports the shared look (design/mockups/ui.css)
    App.tsx            the window
    engine.ts          the engine seen from the page: event types, commands
  src-tauri/           the program (Rust)
    src/lib.rs         the bridge: #[tauri::command] functions, engine events → page
    tauri.conf.json    window, build, packaging
    capabilities/      what the page is allowed to ask
    icons/             generated from the logo: npm run tauri icon ../../oculibre_logo.svg
```

## How the page and Rust talk

- **Page → Rust**: `invoke("start", { wifi: true })` calls the `start` command of `lib.rs`;
  arguments and results travel as JSON, a Rust `Err` becomes a rejected promise.
- **Rust → page**: every engine event is emitted as `engine`; `onEngineEvent` (`engine.ts`)
  receives it as `{ kind: "Status", data: {…} }`. The JSON shape is pinned by the test
  `events_as_json` in `core/src/events.rs`: change `engine.ts` with it.

## Develop

Linux needs the WebKitGTK stack once:

```sh
sudo apt install libwebkit2gtk-4.1-dev libxdo-dev libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev
```

Then, from this folder:

```sh
npm install          # once, and after a change of package.json
npm run tauri dev    # opens the window; the page reloads on save, Rust rebuilds on save
npm run build        # type-checks and builds the page alone (dist/)
```

The virtual camera must be loaded as for the command line (`bash scripts/vcam-load.sh`).

Two traps:

- **VS Code installed as a snap**: its terminal carries variables pointing into `/snap`, and
  WebKitGTK crashes (`WebKitNetworkProcess: symbol lookup error: /snap/core20/…`). The `tauri`
  npm script runs through `scripts/no-snap-env.sh`, which removes them; outside a snap it changes
  nothing.
- **The page in a browser** (`http://localhost:1420`): only the layout, no Rust behind it
  (`invoke` exists only in the Tauri window). The page says so and disables Start.
