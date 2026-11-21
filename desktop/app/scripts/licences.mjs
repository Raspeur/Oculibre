// Writes desktop/target/licences/THIRD-PARTY-LICENSES.html: every component the desktop app ships
// on this platform, with the full text of its licence, for the About screen ("Show the licences").
// Run by the package build (beforeBuildCommand), or by hand: `npm run licences` (from desktop/app).
//
// - Rust crates: cargo-about (`cargo install --locked cargo-about --features cli`), the crates
//   built into the app for this computer's target, build and dev dependencies left out
//   (desktop/about.toml). The licences allowed at all are checked in CI by cargo-deny (deny.toml).
// - The Rust standard library, compiled into every Rust program.
// - The page: the npm packages that end up in the bundle, and the fonts.
// - Windows: adb, shipped for USB phones, with its own licence file.
// FFmpeg and OpenH264 are not shipped: they are the computer's libraries (see the About screen).

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const app = join(dirname(fileURLToPath(import.meta.url)), "..");
const desktop = join(app, "..");
const outDir = join(desktop, "target", "licences");
const outFile = join(outDir, "THIRD-PARTY-LICENSES.html");

// The npm packages bundled into the page (runtime dependencies and what they pull in).
const NPM = [
  "react",
  "react-dom",
  "scheduler",
  "@tauri-apps/api",
  "@fontsource-variable/inter",
  "@fontsource-variable/space-grotesk",
];

// A new runtime dependency must be added above (with what it pulls into the bundle).
const pkgJson = JSON.parse(readFileSync(join(app, "package.json"), "utf8"));
const missing = Object.keys(pkgJson.dependencies ?? {}).filter((d) => !NPM.includes(d));
if (missing.length) throw new Error(`licences.mjs: add ${missing.join(", ")} (and what it bundles) to NPM`);

const run = (cmd, args, cwd) =>
  execFileSync(cmd, args, { cwd, encoding: "utf8", maxBuffer: 256 * 1024 * 1024, stdio: ["ignore", "pipe", "inherit"] });

const host = /^host: (.+)$/m.exec(run("rustc", ["-vV"], desktop))[1].trim();
const version = pkgJson.version;

// component = { name, version, url? }, grouped by the exact text of their licence.
const groups = new Map(); // text → { id, name, components: [] }
const add = (id, name, text, component) => {
  const key = text.replace(/\r\n/g, "\n").trim();
  if (!key) throw new Error(`no licence text for ${component.name}`);
  if (!groups.has(key)) groups.set(key, { id, name, text: key, components: [] });
  groups.get(key).components.push(component);
};

// 1. Rust crates.
// Written to a file, not read from its output: on Windows cargo-about refuses to write into a pipe
// when PowerShell started the build.
mkdirSync(outDir, { recursive: true });
const cargoJson = join(outDir, "cargo-about.json");
try {
  run(
    "cargo",
    ["about", "generate", "--format", "json", "--locked", "--fail", "-o", cargoJson,
      "-c", "about.toml", "-m", join("app", "src-tauri", "Cargo.toml"), "--target", host],
    desktop,
  );
} catch (e) {
  console.error("\nlicences: cargo-about failed (see above). Not installed? cargo install --locked cargo-about --features cli\n");
  throw e;
}
const cargo = JSON.parse(readFileSync(cargoJson, "utf8"));
for (const licence of cargo.licenses) {
  for (const { crate } of licence.used_by) {
    if (!crate.source) continue; // our own crates (MIT, Oculibre's licence)
    add(licence.id, licence.name, licence.text, {
      name: crate.name,
      version: crate.version,
      url: crate.repository || `https://crates.io/crates/${crate.name}`,
      // What the crate offers, when it is more than the licence it is listed under here.
      offers: crate.license && crate.license !== licence.id ? crate.license : undefined,
    });
  }
}

// 2. The Rust standard library (MIT OR Apache-2.0), texts from the toolchain.
const sysroot = run("rustc", ["--print", "sysroot"], desktop).trim();
const rustVersion = /^rustc (\S+)/.exec(run("rustc", ["--version"], desktop))[1];
const rustLicences = join(sysroot, "share", "doc", "rust", "licenses");
for (const [id, name, file] of [
  ["MIT", "MIT License", "MIT.txt"],
  ["Apache-2.0", "Apache License 2.0", "Apache-2.0.txt"],
]) {
  const path = join(rustLicences, file);
  if (!existsSync(path)) throw new Error(`missing ${path} (rustup's documentation component)`);
  // The toolchain's MIT text has blanks; Rust's COPYRIGHT: copyrights are retained by the contributors.
  const text = readFileSync(path, "utf8").replace("<year> <copyright holders>", "The Rust Project Contributors");
  add(id, name, text, {
    name: "Rust standard library",
    version: rustVersion,
    url: "https://github.com/rust-lang/rust",
  });
}

// 3. The page's npm packages and fonts.
const LICENCE_NAMES = { MIT: "MIT License", "OFL-1.1": "SIL Open Font License 1.1", "Apache-2.0": "Apache License 2.0" };
for (const pkg of NPM) {
  const dir = join(app, "node_modules", ...pkg.split("/"));
  const meta = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
  const files = readdirSync(dir).filter((f) => /^(licen[cs]e|copying)/i.test(f) && !/\.spdx$/i.test(f));
  if (files.length === 0) throw new Error(`no licence file in ${dir}`);
  const url = `https://www.npmjs.com/package/${pkg}`;
  for (const file of files) {
    const id = /apache/i.test(file) ? "Apache-2.0" : /mit/i.test(file) ? "MIT" : meta.license;
    add(id, LICENCE_NAMES[id] ?? id, readFileSync(join(dir, file), "utf8"), { name: pkg, version: meta.version, url });
  }
}

// 4. Windows: adb (Android platform-tools), fetched by src-tauri/windows/fetch-adb.ps1.
const adb = join(desktop, "target", "adb", "adb-licences.txt");
const extra = [];
if (host.includes("windows")) {
  if (!existsSync(adb)) throw new Error(`missing ${adb}: run src-tauri/windows/fetch-adb.ps1 first`);
  extra.push({ title: "adb (Android Debug Bridge), shipped for USB phones", text: readFileSync(adb, "utf8") });
}

// The page.
const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
// One card per licence; inside, its texts (they differ by the copyright lines) with who uses each.
const byId = new Map(); // id → { name, texts: [{ text, components }] }
for (const g of groups.values()) {
  g.components.sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));
  if (!byId.has(g.id)) byId.set(g.id, { id: g.id, name: g.name, texts: [] });
  byId.get(g.id).texts.push(g);
}
const key = (c) => `${c.name}@${c.version}`;
const cards = [...byId.values()].map((l) => {
  const seen = new Map();
  for (const t of l.texts) for (const c of t.components) seen.set(key(c), c);
  l.components = [...seen.values()].sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));
  l.texts.sort((a, b) => b.components.length - a.components.length);
  return l;
}).sort((a, b) => b.components.length - a.components.length || a.id.localeCompare(b.id));
const count = new Set(cards.flatMap((l) => l.components.map(key))).size;
const link = (c) =>
  `${c.url ? `<a href="${esc(c.url)}">${esc(c.name)}</a>` : esc(c.name)} ${esc(c.version)}` +
  (c.offers ? ` <span class="offers">(${esc(c.offers)})</span>` : "");
// cargo-about falls back on the standard text, with its blanks, when a crate does not include its
// own licence file (Apache-2.0's appendix always has blanks: not counted).
const generic = (id, text) =>
  /<YEAR>|<COPYRIGHT HOLDER>|<year>|<copyright holders>|\[year\]|\[fullname\]/.test(id === "Apache-2.0" ? "" : text);
const names = (cs) => cs.map((c) => `${esc(c.name)} ${esc(c.version)}`).join(", ");

const html = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Oculibre ${esc(version)}: open source licences</title>
<style>
  :root { --bg: #f6f7f9; --card: #ffffff; --text: #0b1220; --muted: #5b6475; --line: #dde1e8; --accent: #fe633c; }
  @media (prefers-color-scheme: dark) {
    :root { --bg: #0b1220; --card: #111a2b; --text: #e6eaf2; --muted: #93a0b5; --line: #22304a; }
  }
  body { margin: 0; background: var(--bg); color: var(--text); font: 15px/1.5 system-ui, sans-serif; }
  main { max-width: 960px; margin: 0 auto; padding: 32px 16px 64px; }
  h1 { font-size: 26px; margin: 0 0 4px; }
  h2 { font-size: 18px; margin: 0 0 8px; }
  p, li { color: var(--muted); }
  a { color: var(--accent); }
  section { background: var(--card); border: 1px solid var(--line); border-radius: 10px; padding: 16px 20px; margin: 16px 0; }
  ul.components { columns: 2 260px; padding-left: 18px; margin: 0 0 12px; }
  ul.components li { break-inside: avoid; }
  details summary { cursor: pointer; color: var(--accent); }
  pre { white-space: pre-wrap; word-break: break-word; font: 12.5px/1.45 ui-monospace, monospace; color: var(--text);
        background: var(--bg); border-radius: 6px; padding: 12px; margin: 8px 0 0; }
  .count { color: var(--muted); font-weight: 400; font-size: 15px; margin-left: 6px; }
  .offers { color: var(--muted); font-size: 12.5px; }
  details { margin-top: 4px; }
</style>
</head>
<body>
<main>
<h1>Open source licences</h1>
<p>Oculibre ${esc(version)} (${esc(host)}) is free software under the MIT licence
(<a href="https://github.com/Raspeur/Oculibre">source code</a>). It is built with ${count} components made by
other people, listed below with the text of their licence. Thank you to all of them.</p>
<ul>${cards.map((l) => `<li><a href="#${esc(l.id)}">${esc(l.name)}</a>: ${l.components.length}</li>`).join("")}</ul>
<p>${host.includes("windows")
  ? "The video is decoded by Windows' own Media Foundation, or by OpenH264 (BSD-2-Clause, Cisco) when it is installed: neither is shipped with Oculibre."
  : "The video is decoded by the computer's own FFmpeg (LGPL-2.1+ or GPL-2+, as the system built it), or OpenH264 (BSD-2-Clause, Cisco): neither is shipped with Oculibre."}
Components under the MPL-2.0: their source code is at the address given for each one. A component that
offers a choice of licences shows them in brackets; it is listed under the one Oculibre uses.</p>
${cards
  .map(
    (l) => `<section id="${esc(l.id)}">
<h2>${esc(l.name)} <span class="count">${l.components.length}</span></h2>
<ul class="components">${l.components.map((c) => `<li>${link(c)}</li>`).join("")}</ul>
${l.texts
  .map(
    (t) => `<details><summary>${l.texts.length === 1 ? "Licence text" : `Licence text of ${names(t.components)}`}${
      generic(l.id, t.text) ? " (standard text: not included in the component itself)" : ""
    }</summary><pre>${esc(t.text)}</pre></details>`,
  )
  .join("\n")}
</section>`,
  )
  .join("\n")}
${extra.map((e) => `<section><h2>${esc(e.title)}</h2><pre>${esc(e.text)}</pre></section>`).join("\n")}
</main>
</body>
</html>
`;

writeFileSync(outFile, html);
console.log(`licences: ${count} components, ${cards.length} licences, ${groups.size} texts → ${outFile}`);
