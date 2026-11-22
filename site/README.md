# oculibre.org

The project's website: one page in English (`/`) and French (`/fr/`), plain HTML and CSS, no
framework and no dependency. Published on GitHub Pages by
[`.github/workflows/site.yml`](../.github/workflows/site.yml) after every merge that touches it,
and again when a release is published.

| File | What it is |
|---|---|
| `template.html` | The page, with `{{key}}` placeholders |
| `i18n/en.json`, `i18n/fr.json` | The texts. Both files must have the same keys (the build checks) |
| `static/` | CSS, the one small script (download button for the visitor's system), fonts, images |
| `build.mjs` | Fills the template for each language, adds the download links of the latest release, the sitemap, `robots.txt`, `CNAME`, the 404 page |

## Work on it

```sh
node site/build.mjs                         # → site/build/ (OFFLINE=1: no GitHub API, links to the releases page)
python3 -m http.server -d site/build 8000   # then http://localhost:8000
```

- Change a text: edit both `i18n/*.json`. Values may hold inline HTML (`<strong>`, links).
- The diagram is `docs/assets/how-it-works.svg` (the README's), forced to its dark colours.
- The screenshots are WebP copies of `docs/assets/screenshots/`: after changing one, convert it
  again (`magick in.png -quality 88 -define webp:method=6 out.webp`).
- `static/img/og.png` (1200 × 630) is the picture shown when the site is shared on social networks.
- No tracker, no cookie, no outside request: the page's Content-Security-Policy only allows the
  site itself.

Fonts: Inter and Space Grotesk, latin subset, from `@fontsource-variable` 5.3.0, under the SIL Open
Font License (`static/fonts/*-OFL.txt`).
