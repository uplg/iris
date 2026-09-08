# syntax=docker/dockerfile:1.27
# ^ enables BuildKit cache-mounts (`--mount=type=cache`). Default with
# Docker 23+. Without this directive the cache mounts below are silently
# ignored and you're back to recompiling everything every time.

# Custom libav.js build — adds AC-3 / E-AC-3 codecs that none of the
# npm-published libav.js variants ship (Dolby licensing). The Iris client
# uses libav.js for real-time audio transcode in Tier B (Mediabunny → MSE);
# without these codecs, Dolby-audio files fall back to Tier F (server-side
# ffmpeg), defeating the point of client-side transcode.

# Recent emsdk — the image is multi-arch (linux/amd64 + linux/arm64)
# so building on Apple Silicon doesn't go through qemu emulation.
FROM emscripten/emsdk:6.0.9 AS libav-builder
WORKDIR /build
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        git make python3 yasm nasm pkg-config xz-utils \
    && rm -rf /var/lib/apt/lists/*
# Clone libav.js. The version is pinned to match what the web frontend
# pulled in via `bun install libav.js` so the loader's expected
# filename (`libav-6.10.9.0-iris.*`) matches the version emscripten
# spits out.
ARG LIBAVJS_REPO=https://github.com/Yahweasel/libav.js
ARG LIBAVJS_REF=v6.10.9.0
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
# This pairs with mediabunny's Matroska parser, which surfaces `A_DTS`
# tracks as `dts` natively since 1.55 (it used to take a local patch).
# Without a client-side DTS decoder those files fall through to Tier F
# (server-side ffmpeg HLS), which is still functional.
RUN cd configs && node mkconfig.js iris \
    '["avformat","avcodec","avfilter","swresample","audio-filters","parser-aac","parser-ac3","parser-dca","decoder-ac3","decoder-eac3","decoder-flac","decoder-dca","decoder-pcm_s16le","decoder-pcm_s24le","decoder-pcm_s32le","decoder-pcm_f32le"]'
RUN --mount=type=cache,target=/build/libav.js/build,sharing=locked \
    make build-iris -j"$(nproc)" \
    && cp dist/libav-6.10.9.0-iris.wasm.wasm /libav-iris.wasm \
    && cp dist/libav-6.10.9.0-iris.wasm.mjs /libav-iris.wasm.mjs \
    && cp dist/libav-6.10.9.0-iris.wasm.js /libav-iris.wasm.js

# Frontend build (bun + Vite)
FROM oven/bun:1.4.2 AS web-builder
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
COPY --from=libav-builder /libav-iris.wasm public/libavjs/libav-6.10.9.0-iris.wasm.wasm
COPY --from=libav-builder /libav-iris.wasm.mjs public/libavjs/libav-6.10.9.0-iris.wasm.mjs
COPY --from=libav-builder /libav-iris.wasm.js public/libavjs/libav-6.10.9.0-iris.wasm.js
# Per-deploy build id baked into the bundle + emitted to dist/version.json so
# already-open tabs can detect a redeploy and offer a reload. `.git` is excluded
# from the build context, so Vite can't read the sha itself — pass it as a build
# arg (e.g. `--build-arg IRIS_WEB_BUILD_ID=$(git rev-parse --short HEAD)`). If
# left empty, Vite falls back to a build timestamp (still unique per build, so a
# deploy is still detected). The ARG also busts the build cache when it changes,
# forcing version.json to regenerate.
ARG IRIS_WEB_BUILD_ID=""
ENV IRIS_WEB_BUILD_ID=${IRIS_WEB_BUILD_ID}
RUN --mount=type=cache,target=/root/.bun/install/cache,sharing=locked \
    bun run build

# Rust workspace build. `cargo chef cook` compiles the dependency tree from a
# manifest-only recipe into a real image LAYER, so editing a `.rs` file leaves
# it untouched. This replaces the previous `--mount=type=cache` on /app/target:
# the two are mutually exclusive (a cache mount would shadow the cooked layer),
# and the trade is deliberate — a workspace-crate edit loses incremental reuse
# and recompiles its dependents, but cold and off-host builds gain a layer that
# survives `builder prune` and travels via `--cache-from`. The registry / git
# mounts stay: they hold downloaded sources, not build output.
#
# The chef base must stay on trixie — the runtime stage's glibc note depends
# on that pairing. Neither cargo-chef nor the official rust image has shipped
# a 1.98.1 tag yet (checked 2026-09-08: `rust:1.98-trixie` still resolves to
# the 1.98.0 digest), so the base stays 1.98.0 and rustup installs the
# toolchain `rust-toolchain.toml` names into its own cached layer, before any
# source is copied. Swap the base tag once a 1.98.1 image exists.
#
# `--bin iris` on `cook` mirrors the final build, so cook doesn't also compile
# every crate's dev-dependencies. `migrations/` lands in the app layer only:
# `iris-db` embeds it via `sqlx::migrate!`, so it must invalidate that layer
# and must not invalidate the deps layer.
FROM lukemathwalker/cargo-chef:0.1.78-rust-1.98.0-trixie AS chef
WORKDIR /app
ENV CARGO_TERM_COLOR=never
COPY rust-toolchain.toml ./
RUN rustup toolchain install && cargo --version

FROM chef AS planner
COPY rust-toolchain.toml Cargo.toml Cargo.lock* ./
COPY crates ./crates
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS rust-builder
COPY rust-toolchain.toml ./
COPY --from=planner /app/recipe.json ./recipe.json
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo chef cook --release --bin iris --recipe-path recipe.json
COPY Cargo.toml Cargo.lock* ./
COPY crates ./crates
COPY migrations ./migrations
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    cargo build --release --bin iris \
    && cp /app/target/release/iris /iris

# Runtime — Chainguard Wolfi (glibc)
#
# glibc (2.43) just like Debian, so the prebuilt glibc shaka-packager binary
# and the glibc Rust binary (built above on rust 1.98.1 / trixie glibc 2.41 —
# older, so it runs fine on Wolfi's newer 2.43) work UNCHANGED. NOT Alpine:
# musl would break the prebuilt shaka binary and hurt librqbit's allocation-
# heavy throughput. Codec parity re-verified on ffmpeg 9.0.1 (2026-09-08):
# Wolfi's build ships every decoder / demuxer / encoder / bsf Iris uses
# server-side (h264/hevc/vp9/av1+dav1d, aac/ac3/eac3/dts/flac/opus/truehd,
# libx264/libx265 + the native AAC encoder, mkv/mp4/ts/avi demux, hls/mp4
# mux, ass/pgs/srt/webvtt, filter_units). It is built WITHOUT libzimg, so
# there is no `zscale`: the remuxer's HDR → SDR flatten rides on swscale's
# own colour management instead (`transcode_video_filter` in `remuxer.rs`).
#
# Why Wolfi over debian:trixie-slim (measured on arm64, runtime layers only):
#   image size  750 MB → 298 MB   ·   CVEs  261 (11 crit / 39 high) → 0
#
# The `iris-data` named volume is host-side and untouched by swapping the
# base; the process runs as uid 1001 — the same uid the old Debian `iris`
# user wrote prod data with — so it keeps full read/write on existing data.
FROM cgr.dev/chainguard/wolfi-base AS runtime
ARG TARGETARCH
# `apk` here is Wolfi's package manager — glibc packages, NOT Alpine's musl.
# `ffmpeg-9.0` is Wolfi's versioned package (it provides the bare `ffmpeg`
# name too, which is what a plain `apk add ffmpeg` resolves to today); naming
# it keeps the line from silently jumping majors on the next rebuild.
RUN apk add --no-cache ca-certificates-bundle ffmpeg-9.0 tini curl

ARG SHAKA_VERSION=v3.9.3
RUN set -eux; \
    case "${TARGETARCH}" in \
        amd64) shaka_arch=x64 ;; \
        arm64) shaka_arch=arm64 ;; \
        *) echo "unsupported arch: ${TARGETARCH}" >&2; exit 1 ;; \
    esac; \
    # wolfi-base has no /usr/local/bin (Debian does) — create it before curl.
    mkdir -p /usr/local/bin; \
    curl -fsSL -o /usr/local/bin/packager \
        "https://github.com/shaka-project/shaka-packager/releases/download/${SHAKA_VERSION}/packager-linux-${shaka_arch}"; \
    chmod +x /usr/local/bin/packager; \
    /usr/local/bin/packager --version | head -1; \
    apk del curl

# Run as a non-root numeric UID (Chainguard-idiomatic — no passwd entry
# needed). uid 1001 is the SAME uid the previous Debian `iris` user wrote
# the `iris-data` volume with, so existing prod files (owned 1001) stay
# read/write across the migration — verified against a live volume.
WORKDIR /srv/iris
RUN mkdir -p /srv/iris/web /srv/iris/config /data /data/downloads \
    && chown -R 1001:1001 /srv/iris /data

COPY --from=rust-builder /iris /usr/local/bin/iris
COPY --from=web-builder /app/web/dist /srv/iris/web
COPY config/config.toml.example /srv/iris/config/config.toml.example
COPY config/providers.toml.example /srv/iris/config/providers.toml.example

USER 1001:1001
ENV HOME=/srv/iris
ENV IRIS_CONFIG=/srv/iris/config/config.toml
EXPOSE 8080
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/iris"]
