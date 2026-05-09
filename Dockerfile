# syntax=docker/dockerfile:1.7
# ^ enables BuildKit cache-mounts (`--mount=type=cache`). Default with
# Docker 23+. Without this directive the cache mounts below are silently
# ignored and you're back to recompiling everything every time.

###############################################################################
# 1) Frontend build (bun + Vite)
###############################################################################
FROM oven/bun:1 AS web-builder
WORKDIR /app/web
COPY web/package.json web/bun.lock* ./
RUN --mount=type=cache,target=/root/.bun/install/cache,sharing=locked \
    bun install --frozen-lockfile
COPY web/ ./
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
