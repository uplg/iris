###############################################################################
# 1) Frontend build (bun + Vite)
###############################################################################
FROM oven/bun:1 AS web-builder
WORKDIR /app/web
COPY web/package.json web/bun.lock* ./
RUN bun install --frozen-lockfile
COPY web/ ./
RUN bun run build

###############################################################################
# 2) Rust workspace build
###############################################################################
FROM rust:1.95-trixie AS rust-builder
WORKDIR /app
ENV CARGO_TERM_COLOR=never
COPY rust-toolchain.toml Cargo.toml Cargo.lock* ./
COPY crates ./crates
COPY migrations ./migrations
RUN cargo build --release --bin iris

###############################################################################
# 3) Runtime
###############################################################################
FROM debian:trixie-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        ffmpeg \
        tini \
    && rm -rf /var/lib/apt/lists/*

RUN useradd --system --create-home --uid 1001 iris
WORKDIR /srv/iris
RUN mkdir -p /srv/iris/web /data /data/downloads && chown -R iris:iris /srv/iris /data

COPY --from=rust-builder /app/target/release/iris /usr/local/bin/iris
COPY --from=web-builder /app/web/dist /srv/iris/web
COPY config/config.toml.example /srv/iris/config/config.toml.example
COPY config/providers.toml.example /srv/iris/config/providers.toml.example

USER iris
ENV IRIS_CONFIG=/srv/iris/config/config.toml
EXPOSE 8080
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/iris"]
