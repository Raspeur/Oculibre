# Oculibre desktop app

The window of Oculibre (#15): [Tauri 2](https://tauri.app) with a React + TypeScript page, over
the same engine as the `oculibre` command (`desktop/core`). Linux and Windows.

```
app/
  src/                 the page (React + TypeScript, built by Vite)
    main.tsx           entry point; imports the fonts and the shared look (design/mockups/ui.css)
    App.tsx            the window: sidebar, stage, panel, or a full screen (pairing, settings)
    engine.ts          the engine seen from the page: event types, command wrappers
    useEngine.ts       the window's state, rebuilt from the engine events
    i18n.ts            English and French texts
    components/        Sidebar, Stage (preview), Panel (camera settings), Pairing, Settings,
                       Tour (guided tour), VcamProblem (virtual camera not ready)
  src-tauri/           the program (Rust)
    src/lib.rs         the bridge: #[tauri::command] functions, engine events → page
    src/settings.rs    this computer's settings (Wi-Fi, portrait, decoder, tray, language)
    src/tray.rs        the tray icon and its menu
    tauri.conf.json    window, build, Linux package; tauri.windows.conf.json: Windows installer
    capabilities/      what the page is allowed to ask
    icons/             generated from the logo: npm run tauri icon ../../oculibre_logo.svg
    linux/             .deb files: module config, post-install script, the vcam-setup root helper
    windows/           NSIS hooks, installer pictures, adb download script and licences
```

## How the page and Rust talk

- **Page → Rust**: `invoke("start", { wifi: true })` calls the `start` command of `lib.rs`;
  arguments and results travel as JSON, a Rust `Err` becomes a rejected promise.
- **Rust → page**: every engine event is emitted as `engine`; `onEngineEvent` (`engine.ts`)
  receives it as `{ kind: "Status", data: {…} }`. The JSON shape is pinned by the test
  `events_as_json` in `core/src/events.rs`: change `engine.ts` with it.

## Develop

Windows: see [Windows](../README.md#windows) in the desktop README. Linux needs the WebKitGTK
stack once:

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

## Package

Windows: `npm run tauri build` → `../target/release/bundle/nsis/Oculibre_<version>_x64-setup.exe`
(details in [the desktop README](../README.md#windows)). Linux, for every supported release at once:
`bash scripts/build-deb.sh` from the repository root ([details](../README.md#packages-deb)).

### Linux `.deb` for this computer only

```sh
npm run tauri build -- --bundles deb
# → ../target/release/bundle/deb/Oculibre_<version>_amd64.deb
sudo apt install ./../target/release/bundle/deb/Oculibre_*_amd64.deb
```

The package (`src-tauri/tauri.conf.json`, `bundle.linux.deb`, files in `src-tauri/linux/`):

- depends on FFmpeg (`libavcodec62`, `libavutil60`: one package per FFmpeg major version, see
  `docs/research/video-decoding.typ`) and on the virtual camera module: Ubuntu's signed one
  first (`linux-modules-v4l2loopback-generic`, loads with Secure Boot, already pulled by the
  generic kernel on 26.04), else `v4l2loopback-dkms`; recommends `adb` (USB);
- installs `/etc/modprobe.d/oculibre.conf` (`/dev/video10`, "Oculibre") and
  `/etc/modules-load.d/oculibre.conf` (loaded at every boot), the same as
  `scripts/vcam-load.sh --persist`;
- loads the camera right away after installation (`linux/postinst.sh`, never fails the
  installation; says what to do with Secure Boot and a DKMS module);
- the launcher has `StartupWMClass=oculibre-app`: on Wayland the window (app id = program name)
  finds its icon. Under `tauri dev` there is no launcher, hence a generic icon.

Two traps:

- **VS Code installed as a snap**: its terminal carries variables pointing into `/snap`, and
  WebKitGTK crashes (`WebKitNetworkProcess: symbol lookup error: /snap/core20/…`). The `tauri`
  npm script runs through `scripts/no-snap-env.sh`, which removes them; outside a snap it changes
  nothing.
- **The page in a browser** (`http://localhost:1420`): only the layout, no Rust behind it
  (`invoke` exists only in the Tauri window). The page says so and disables Start.
