# Build environment of Oculibre's .deb for one Ubuntu/Debian release (scripts/build-deb.sh).
# Built against that release's own libraries: the package then fits it (glibc, WebKitGTK, and the
# FFmpeg major version that Oculibre loads at run time).
ARG BASE=ubuntu:26.04
FROM ${BASE}

ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential pkg-config curl ca-certificates file \
        libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev \
        libavcodec-dev libavutil-dev libclang-dev clang \
        nodejs npm \
    && rm -rf /var/lib/apt/lists/*

# Rust, readable by any user (the build runs as the host user, see build-deb.sh).
ENV RUSTUP_HOME=/usr/local/rustup PATH=/usr/local/cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | CARGO_HOME=/usr/local/cargo sh -s -- -y --profile minimal \
    && chmod -R a+rX /usr/local/rustup /usr/local/cargo
