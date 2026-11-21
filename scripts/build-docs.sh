#!/usr/bin/env bash
# Builds every Typst document of docs/ into docs/build/*.pdf.
#   bash scripts/build-docs.sh            # all documents
#   bash scripts/build-docs.sh --watch docs/protocol.typ
# Needs typst (https://typst.app, or: cargo install --locked typst-cli) and the template fonts
# (Source Sans 3, Fira Code, Inria Sans).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
command -v typst > /dev/null || { echo "typst not found: cargo install --locked typst-cli"; exit 1; }
mkdir -p docs/build

pdf_for() { echo "docs/build/$(echo "${1#docs/}" | tr '/' '-' | sed 's/\.typ$/.pdf/')"; }

if [ "${1:-}" = "--watch" ]; then
    exec typst watch --root . "$2" "$(pdf_for "$2")"
fi

find docs -name '*.typ' ! -name 'oculibre.typ' ! -name 'example.typ' ! -path 'docs/build/*' | sort |
while read -r f; do
    out="$(pdf_for "$f")"
    typst compile --root . "$f" "$out" && echo "✓ $out"
done
