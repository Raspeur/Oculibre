// Builds oculibre.org into site/build/: one page per language from template.html and i18n/*.json,
// the download links of the latest GitHub release, the sitemap and the files GitHub Pages wants.
// Node only, no dependency: `node site/build.mjs` (OFFLINE=1 skips the GitHub API).
import { createHash } from "node:crypto";
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SITE = "https://oculibre.org";
const REPO = "Raspeur/Oculibre";
const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const out = join(here, "build");
const read = (p) => readFileSync(p, "utf8");

// ---------- latest release (falls back to the releases page if GitHub cannot be asked)
async function latestRelease() {
  const fallback = `https://github.com/${REPO}/releases/latest`;
  const version = read(join(root, "desktop/Cargo.toml")).match(/^version\s*=\s*"([^"]+)"/m)[1];
  const none = { version, win: fallback, apk: fallback, sums: fallback, debs: [] };
  if (process.env.OFFLINE) return none;
  const headers = { Accept: "application/vnd.github+json", "User-Agent": "oculibre-site-build" };
  if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
  try {
    const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, { headers });
    if (!res.ok) throw new Error(`GitHub API: ${res.status}`);
    const rel = await res.json();
    const find = (re) => rel.assets.find((a) => re.test(a.name))?.browser_download_url ?? fallback;
    const debs = rel.assets
      .map((a) => ({ m: a.name.match(/_(ubuntu|debian)-([\d.]+)_amd64\.deb$/), url: a.browser_download_url }))
      .filter((d) => d.m)
      .map((d) => ({ label: `${d.m[1] === "ubuntu" ? "Ubuntu" : "Debian"} ${d.m[2]}`, url: d.url }))
      .sort((a, b) => b.label.localeCompare(a.label, "en", { numeric: true }));
    return {
      version: rel.tag_name.replace(/^v/, ""),
      win: find(/_x64-setup\.exe$/),
      apk: find(/_android\.apk$/),
      sums: find(/^SHA256SUMS\.txt$/),
      debs,
    };
  } catch (e) {
    console.warn(`warning: ${e.message}; download buttons lead to the releases page`);
    return none;
  }
}

// ---------- templating: {{key}} from the language file, {name} inside its values
const escapeAttr = (s) => s.replace(/&(?!(?:[a-z]+|#\d+);)/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

function render(template, values) {
  // Placeholders inside an attribute get escaped; the others may hold our own inline HTML.
  const inAttr = new Set();
  for (const m of template.matchAll(/="[^"]*"/g)) for (const k of m[0].matchAll(/\{\{(\w+)\}\}/g)) inAttr.add(k[1]);
  const html = template.replace(/\{\{(\w+)\}\}/g, (_, k) => {
    if (!(k in values)) throw new Error(`no value for {{${k}}}`);
    const v = String(values[k]);
    return inAttr.has(k) && !/^(https?:|\/|#)/.test(v) ? escapeAttr(v) : v;
  });
  return html;
}

function fill(text, vars) {
  return text.replace(/\{(\w+)\}/g, (all, k) => (k in vars ? vars[k] : all));
}

// ---------- build
const rel = await latestRelease();
const langs = readdirSync(join(here, "i18n")).filter((f) => f.endsWith(".json")).map((f) => f.slice(0, -5));
const strings = Object.fromEntries(langs.map((l) => [l, JSON.parse(read(join(here, "i18n", `${l}.json`)))]));
const keys = Object.keys(strings.en);
for (const l of langs) {
  const missing = keys.filter((k) => !(k in strings[l]));
  const extra = Object.keys(strings[l]).filter((k) => !keys.includes(k));
  if (missing.length || extra.length) throw new Error(`${l}.json: missing ${missing}, extra ${extra}`);
}

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
cpSync(join(here, "static"), join(out, "static"), { recursive: true });
// The README's diagram follows the system theme; the site is always dark.
const diagram = read(join(root, "docs/assets/how-it-works.svg")).replace("@media (prefers-color-scheme: dark) {", "@media all {");
writeFileSync(join(out, "static/img/how-it-works.svg"), diagram);

const assetVersion = createHash("sha256")
  .update(read(join(here, "static/style.css")) + read(join(here, "static/main.js")))
  .digest("hex").slice(0, 10);
const template = read(join(here, "template.html"));
const debLinks = rel.debs.length
  ? rel.debs.map((d) => `<a class="btn" href="${d.url}">${d.label} <span>.deb</span></a>`).join("\n            ")
  : `<a class="btn" href="https://github.com/${REPO}/releases/latest">.deb <span>GitHub</span></a>`;

for (const l of langs) {
  const s = strings[l];
  const other = langs.find((x) => x !== l);
  const url = SITE + s.path;
  const values = {};
  for (const [k, v] of Object.entries(s)) values[k] = fill(v, { version: rel.version, sums: rel.sums });
  const jsonld = {
    "@context": "https://schema.org",
    "@type": "SoftwareApplication",
    name: "Oculibre",
    description: s.description,
    url,
    inLanguage: s.lang,
    applicationCategory: "MultimediaApplication",
    operatingSystem: "Windows 10, Windows 11, Linux, Android 5.0+",
    softwareVersion: rel.version,
    downloadUrl: `https://github.com/${REPO}/releases/latest`,
    license: "https://opensource.org/licenses/MIT",
    isAccessibleForFree: true,
    offers: { "@type": "Offer", price: "0", priceCurrency: "EUR" },
    image: `${SITE}/static/img/og.png`,
    screenshot: `${SITE}/static/img/desktop.webp`,
    author: { "@type": "Person", name: "Raspeur", url: "https://github.com/Raspeur" },
    codeRepository: `https://github.com/${REPO}`,
  };
  Object.assign(values, {
    site: SITE,
    url,
    alt_lang: other,
    alt_path: strings[other].path,
    asset_version: assetVersion,
    win_url: rel.win,
    apk_url: rel.apk,
    deb_links: debLinks,
    jsonld: JSON.stringify(jsonld).replace(/</g, "\\u003c"),
  });
  const dir = join(out, s.path);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "index.html"), render(template, values));
}

// 404: GitHub Pages serves /404.html for every missing path, in both languages at once.
const en = strings.en, fr = strings.fr;
writeFileSync(join(out, "404.html"), `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<title>${en.notfound_title} · Oculibre</title>
<link rel="icon" href="/static/img/favicon.svg" type="image/svg+xml">
<link rel="stylesheet" href="/static/style.css?v=${assetVersion}">
</head>
<body>
<main class="notfound">
  <a class="brand" href="/"><span class="brand__tile"><img src="/static/img/logo.svg" alt="" width="28" height="28"></span><span class="brand__name">Oculibre</span></a>
  <h1>404</h1>
  <p>${en.notfound_text} <a href="/">${en.notfound_home}</a></p>
  <p lang="fr">${fr.notfound_text} <a href="/fr/">${fr.notfound_home}</a></p>
</main>
</body>
</html>
`);

const today = new Date().toISOString().slice(0, 10);
const alternates = langs.map((l) => `    <xhtml:link rel="alternate" hreflang="${l}" href="${SITE}${strings[l].path}"/>`).join("\n");
writeFileSync(join(out, "sitemap.xml"), `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9" xmlns:xhtml="http://www.w3.org/1999/xhtml">
${langs.map((l) => `  <url>\n    <loc>${SITE}${strings[l].path}</loc>\n    <lastmod>${today}</lastmod>\n${alternates}\n  </url>`).join("\n")}
</urlset>
`);
writeFileSync(join(out, "robots.txt"), `User-agent: *\nAllow: /\n\nSitemap: ${SITE}/sitemap.xml\n`);
writeFileSync(join(out, "CNAME"), "oculibre.org\n");
writeFileSync(join(out, ".nojekyll"), "");

if (!existsSync(join(out, "static/img/og.png"))) throw new Error("static/img/og.png is missing");
console.log(`site built in ${out} (version ${rel.version}, ${langs.join(" + ")})`);
