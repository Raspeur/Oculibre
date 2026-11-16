#!/usr/bin/env bash
# Builds Oculibre's .deb for each Ubuntu/Debian release (#18), each in a Docker container of that
# release, then checks that it installs on a clean system of the same release.
#
#   bash scripts/build-deb.sh                 # every release below
#   bash scripts/build-deb.sh ubuntu-24.04    # one (or several)
#
# Packages land in desktop/target/deb/: Oculibre_<version>_<release>_amd64.deb.
#
# Why one package per release: FFmpeg changes its binary interface at each major version, and
# Oculibre loads the libavcodec major it was built against (libavcodec.so.60 on Ubuntu 24.04,
# .61 on Debian 13, .62 on Ubuntu 26.04). The window (HTML/JS) is the same everywhere: it is
# built once here, the containers only compile the Rust side.
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
OUT=$ROOT/desktop/target/deb

declare -A BASE=(
    [ubuntu-26.04]=ubuntu:26.04
    [ubuntu-24.04]=ubuntu:24.04
    [debian-13]=debian:13
)
# What decodes the video there (recommended, not required: without it Oculibre says what to
# install). OpenH264 only where it is version 2.6 (libopenh264.so.8, the ABI Oculibre loads).
declare -A DECODERS=(
    [ubuntu-26.04]='libavcodec62 | libopenh264-8'
    [ubuntu-24.04]='libavcodec60'
    [debian-13]='libavcodec61 | libopenh264-8'
)

targets=("$@")
[ ${#targets[@]} -eq 0 ] && targets=(ubuntu-26.04 ubuntu-24.04 debian-13)
for t in "${targets[@]}"; do
    [ -n "${BASE[$t]:-}" ] || { echo "unknown release: $t (known: ${!BASE[*]})" >&2; exit 1; }
done

version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/desktop/Cargo.toml" | head -1)

echo "== The window (once, on this computer)"
(cd "$ROOT/desktop/app" && npm run build >/dev/null)

for t in "${targets[@]}"; do
    echo "== $t: build environment"
    docker build -q -t "oculibre-deb:$t" --build-arg "BASE=${BASE[$t]}" "$ROOT/scripts/deb" >/dev/null

    # A copy of the sources (the build writes next to them), and caches kept between runs.
    work=$OUT/$t
    mkdir -p "$work/src" "$work/cache"
    rsync -a --delete --exclude target --exclude node_modules "$ROOT/desktop/" "$work/src/desktop/"

    config=$(printf '{"build":{"beforeBuildCommand":""},"bundle":{"linux":{"deb":{"recommends":["%s","adb","pkexec"]}}}}' "${DECODERS[$t]}")
    echo "== $t: build (first time: several minutes)"
    docker run --rm --user "$(id -u):$(id -g)" \
        -e HOME=/tmp -e CARGO_HOME=/cache/cargo -e CARGO_TARGET_DIR=/cache/target -e npm_config_cache=/cache/npm \
        -v "$work/src:/work" -v "$work/cache:/cache" -w /work/desktop/app \
        "oculibre-deb:$t" \
        sh -c 'npm ci --ignore-scripts --no-audit --no-fund --loglevel=error && npx tauri build --bundles deb --config "$1"' \
        sh "$config"

    deb=$OUT/Oculibre_${version}_${t}_amd64.deb
    cp "$work/cache/target/release/bundle/deb/Oculibre_${version}_amd64.deb" "$deb"

    echo "== $t: install on a clean ${BASE[$t]}"
    docker run --rm -v "$OUT:/debs:ro" "${BASE[$t]}" sh -c '
        apt-get update -qq >/dev/null
        if ! apt-get install -y -qq --no-install-recommends "/debs/$1" >/tmp/apt.log 2>&1; then
            cat /tmp/apt.log; exit 1
        fi
        missing=$(ldd /usr/bin/oculibre-app | grep "not found" || true)
        if [ -n "$missing" ]; then echo "missing libraries: $missing"; exit 1; fi
        dpkg -s oculibre | grep -E "^(Depends|Recommends):"
        echo "installs, every library found"
    ' sh "$(basename "$deb")"
    echo "== $t: $deb"
done
