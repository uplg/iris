# syntax=docker/dockerfile:1.7
# ^ enables BuildKit cache-mounts (`--mount=type=cache`). Default with
# Docker 23+. Without this directive the cache mounts below are silently
# ignored and you're back to recompiling everything every time.

###############################################################################
# 0) Custom libav.js build — adds AC-3 / E-AC-3 codecs that none of the
#    npm-published libav.js variants ship (Dolby licensing). Built once,
#    cached on subsequent docker builds via BuildKit's `target` cache.
#
#    The Iris client uses libav.js for real-time audio transcode in
#    Tier B (Mediabunny → MSE). Without these codecs, files with
#    Dolby audio would have to fall back to Tier F (server-side
#    ffmpeg) — defeating the point of client-side transcode.
###############################################################################
# Recent emsdk — the image is multi-arch (linux/amd64 + linux/arm64)
# so building on Apple Silicon doesn't go through qemu emulation.
FROM emscripten/emsdk:5.0.7 AS libav-builder
WORKDIR /build
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        git make python3 yasm nasm pkg-config xz-utils \
    && rm -rf /var/lib/apt/lists/*
# Clone libav.js. The version is pinned to match what the web frontend
# pulled in via `bun install libav.js` so the loader's expected
# filename (`libav-6.8.8.0-iris.*`) matches the version emscripten
# spits out.
ARG LIBAVJS_REPO=https://github.com/Yahweasel/libav.js
ARG LIBAVJS_REF=v6.8.8.0
RUN git clone --depth 1 --branch ${LIBAVJS_REF} ${LIBAVJS_REPO} /build/libav.js
WORKDIR /build/libav.js
# Custom variant config. `mkconfig.js` must run from inside `configs/`
# because it reads `fragments/...` via relative paths. Fragments not
# present on disk (anything not under `configs/fragments/`) fall
# through to plain `--enable-<kind>=<name>` ffmpeg configure flags —
# that's how `decoder-ac3` / `decoder-eac3` / `parser-aac` get
# enabled without needing standalone fragment directories.
#
# Decoders only — no muxers / encoders / video — because the Iris
# client uses libav exclusively to decode non-WebCodecs audio
# (AC-3, E-AC-3, FLAC, PCM, DTS) into PCM samples; encoding back to
# AAC is done by `WebCodecs.AudioEncoder`, muxing by Mediabunny.
#
# `decoder-dca` is ffmpeg's native DTS Coherent Acoustics decoder.
# It handles core DTS losslessly and falls back to the core layer
# for DTS-HD MA (the extension substream is silently dropped) —
# acceptable for Tier B since we re-encode to AAC stereo/5.1 anyway.
# This pairs with the `mediabunny` patch (see `patches/mediabunny+*.patch`)
# which teaches the Matroska parser to surface `A_DTS` tracks as
# `dts` — upstream mediabunny ignores them. If that patch ever fails
# to apply, DTS files quietly fall through to Tier F (server-side
# ffmpeg HLS) which is still functional.
RUN cd configs && node mkconfig.js iris \
    '["avformat","avcodec","avfilter","swresample","audio-filters","parser-aac","parser-ac3","parser-dca","decoder-ac3","decoder-eac3","decoder-flac","decoder-dca","decoder-pcm_s16le","decoder-pcm_s24le","decoder-pcm_s32le","decoder-pcm_f32le"]'
RUN --mount=type=cache,target=/build/libav.js/build,sharing=locked \
    make build-iris -j"$(nproc)" \
    && cp dist/libav-6.8.8.0-iris.wasm.wasm /libav-iris.wasm \
    && cp dist/libav-6.8.8.0-iris.wasm.mjs /libav-iris.wasm.mjs \
    && cp dist/libav-6.8.8.0-iris.wasm.js /libav-iris.wasm.js

###############################################################################
# 1) Frontend build (bun + Vite)
###############################################################################
FROM oven/bun:1 AS web-builder
WORKDIR /app/web
# Copy lockfiles AND the patches directory before installing — bun
# resolves `patchedDependencies` paths during `install`, so the patch
# files must already exist on disk by the time we run it. Without
# this the build fails with `Couldn't find patch file:
# patches/<pkg>@<ver>.patch`. The `patches/` directory is created
# under `web/` by `bun patch --commit`.
COPY web/package.json web/bun.lock* ./
COPY web/patches ./patches
RUN --mount=type=cache,target=/root/.bun/install/cache,sharing=locked \
    bun install --frozen-lockfile
COPY web/ ./
# Drop the iris-variant WASM into public/ so Vite copies it into dist.
# (The npm-package libav.js wasm files in public/libavjs/ stay as the
# fallback when the iris variant isn't present.)
COPY --from=libav-builder /libav-iris.wasm public/libavjs/libav-6.8.8.0-iris.wasm.wasm
COPY --from=libav-builder /libav-iris.wasm.mjs public/libavjs/libav-6.8.8.0-iris.wasm.mjs
COPY --from=libav-builder /libav-iris.wasm.js public/libavjs/libav-6.8.8.0-iris.wasm.js
RUN --mount=type=cache,target=/root/.bun/install/cache,sharing=locked \
    bun run build

###############################################################################
# 2) Rust workspace build
#
# Three cache mounts:
#   - cargo registry: index + downloaded crate tarballs from crates.io
#   - cargo git: cloned git dependencies (none today, future-proof)
#   - target: the build output directory, where >95% of compile time lives
#
# Cache mounts persist across `docker build` invocations on the same host,
# so a one-line code change recompiles iris-api only (~10 s) instead of
# the entire dep tree (~2 min). They are NOT layered, so the binary
# itself must be copied out *before* the RUN finishes — otherwise it
# vanishes with the cache when the next build runs.
###############################################################################
FROM rust:1.95-trixie AS rust-builder
WORKDIR /app
ENV CARGO_TERM_COLOR=never
COPY rust-toolchain.toml Cargo.toml Cargo.lock* ./
COPY crates ./crates
COPY migrations ./migrations
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release --bin iris \
    && cp /app/target/release/iris /iris

###############################################################################
# 3) Runtime
###############################################################################
FROM debian:trixie-slim AS runtime
ARG TARGETARCH
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        ffmpeg \
        tini \
        curl \
    && rm -rf /var/lib/apt/lists/*

# shaka-packager — used to convert ffmpeg's per-stream MP4 outputs into
# a proper HLS-CMAF manifest tree (master.m3u8 + per-rendition .m3u8 +
# init/segment .m4s). ffmpeg's own HLS muxer produces manifests with
# overly-strict CODECS attributes that browsers reject upfront via
# `MediaSource.isTypeSupported`; shaka writes correct codec strings.
ARG SHAKA_VERSION=v3.7.2
RUN set -eux; \
    case "${TARGETARCH}" in \
        amd64) shaka_arch=x64 ;; \
        arm64) shaka_arch=arm64 ;; \
        *) echo "unsupported arch: ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    curl -fsSL -o /usr/local/bin/packager \
        "https://github.com/shaka-project/shaka-packager/releases/download/${SHAKA_VERSION}/packager-linux-${shaka_arch}"; \
    chmod +x /usr/local/bin/packager; \
    /usr/local/bin/packager --version | head -1; \
    apt-get purge -y --auto-remove curl; \
    rm -rf /var/lib/apt/lists/*

RUN useradd --system --create-home --uid 1001 iris
WORKDIR /srv/iris
RUN mkdir -p /srv/iris/web /data /data/downloads && chown -R iris:iris /srv/iris /data

COPY --from=rust-builder /iris /usr/local/bin/iris
COPY --from=web-builder /app/web/dist /srv/iris/web
COPY config/config.toml.example /srv/iris/config/config.toml.example
COPY config/providers.toml.example /srv/iris/config/providers.toml.example

USER iris
ENV IRIS_CONFIG=/srv/iris/config/config.toml
EXPOSE 8080
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/iris"]
