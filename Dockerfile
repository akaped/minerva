# syntax=docker/dockerfile:1
# Minerva server with its embedded web app. Build: docker compose build

# 1. Browser bundles (app.js, mermaid.js) from source.
FROM node:24-bookworm-slim AS web
WORKDIR /src
COPY package.json package-lock.json ./
RUN npm ci
COPY web ./web
RUN npm run build

# 2. The Rust server; web assets are embedded at compile time.
FROM rust:1.96-bookworm AS server
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY web ./web
COPY plugins ./plugins
COPY --from=web /src/web/dist ./web/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --locked --release -p minerva-server \
    && cp target/release/minerva-server /usr/local/bin/minerva-server

# 3. Minimal runtime: one binary, a non-root user, and a data volume.
FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /data --shell /usr/sbin/nologin minerva \
    && mkdir -p /data && chown minerva:minerva /data
COPY --from=server /usr/local/bin/minerva-server /usr/local/bin/minerva-server
USER minerva
# Built-in plugins are compiled in; mount a folder at /plugins to add or override themes.
ENV MINERVA_DATABASE=/data/minerva.db \
    MINERVA_BIND=0.0.0.0:3000 \
    MINERVA_PLUGINS=/plugins
VOLUME /data
EXPOSE 3000
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD ["bash", "-c", "exec 3<>/dev/tcp/127.0.0.1/3000 && printf 'GET /api/v1/health HTTP/1.0\\r\\n\\r\\n' >&3 && grep -q '\"status\":\"ok\"' <&3"]
ENTRYPOINT ["/usr/local/bin/minerva-server"]
