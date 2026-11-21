// Shared layout for every Oculibre document.
//
// Usage, at the top of a document:
//   #import "oculibre.typ": *            (or "../oculibre.typ" from a sub-folder)
//   #show: oculibre-doc.with(title: [My title], subtitle: [...], revision: "1.0")
//
// Build: from the repository root, `bash scripts/build-docs.sh` (PDFs go to docs/build/).
// The repository root must be the Typst root (the logo lives there): `typst compile --root . …`.

#import "@preview/isc-hei-document:0.8.1": *

#let oculibre-pink = rgb("#dc0068")

#let oculibre-doc(
  title: none,
  subtitle: none,
  revision: none,
  date: none,
  show-toc: false,
  body,
) = {
  show: project.with(
    doc-type: "document",
    show-cover: false,
    show-toc: show-toc,
    fancy-line: true,
    title: title,
    subtitle: subtitle,
    authors: ("Oculibre contributors",),
    date: date,
    revision: revision,
    language: "en",
    logo: image("../oculibre_logo+titre.svg"),
    code-theme: "bluloco-light",
  )

  set table(
    stroke: 0.5pt + luma(180),
    inset: 6pt,
    fill: (x, y) => if y == 0 { luma(230) } else if calc.even(y) { luma(247) } else { white },
  )
  show table.cell.where(y: 0): strong
  show table: set text(size: 0.92em)
  show table: set par(justify: false)
  show raw.where(block: true): set text(size: 0.85em)
  show raw.where(block: true): block.with(
    fill: luma(248),
    stroke: 0.5pt + luma(210),
    inset: 8pt,
    radius: 3pt,
    width: 100%,
  )

  body
}

// Status marks for roadmaps and result tables.
#let done = text(fill: rgb("#2e7d32"), weight: "bold")[✓]
#let open = text(fill: luma(140))[○]
#let fail = text(fill: rgb("#c62828"), weight: "bold")[✗]

// Highlighted remark.
#let remark(title: [Note], body) = block(
  width: 100%,
  inset: 10pt,
  radius: 3pt,
  stroke: (left: 3pt + oculibre-pink),
  fill: oculibre-pink.lighten(94%),
)[*#title.* #body]

// Long identifiers (e.g. vendor camera tags) that may break after dots and underscores in narrow
// table columns: #tag("org.quic.camera2.streamBasedFPS.info.StreamBasedFPSTable").
#let tag(name) = raw(name.replace(".", ".\u{200B}").replace("_", "_\u{200B}"))
