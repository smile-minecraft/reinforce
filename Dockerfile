# syntax=docker/dockerfile:1

FROM node:24-slim AS web-builder
WORKDIR /app/web

RUN corepack enable && corepack prepare pnpm@9 --activate

COPY web/package.json web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile

COPY web/ ./
RUN pnpm build

FROM rust:1-bookworm AS rust-builder
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY plugins ./plugins
COPY fixtures ./fixtures
COPY --from=web-builder /app/web/dist ./web/dist

RUN cargo build --release --locked \
    -p reinforce-server \
    -p reinforce-plugin-mineru \
    -p reinforce-plugin-tikz \
    -p reinforce-plugin-acp

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 reinforce \
    && useradd --system --uid 10001 --gid reinforce --home-dir /data --shell /usr/sbin/nologin reinforce \
    && mkdir -p /data/blobs /data/fonts \
    && chown -R reinforce:reinforce /data

COPY --from=rust-builder /app/target/release/reinforce /usr/local/bin/reinforce
COPY --from=rust-builder /app/target/release/reinforce-plugin-mineru /usr/local/bin/reinforce-plugin-mineru
COPY --from=rust-builder /app/target/release/reinforce-plugin-tikz /usr/local/bin/reinforce-plugin-tikz
COPY --from=rust-builder /app/target/release/reinforce-plugin-acp /usr/local/bin/reinforce-plugin-acp

ENV REINFORCE_HOST=0.0.0.0 \
    REINFORCE_PORT=3000 \
    REINFORCE_DB=/data/reinforce.db \
    REINFORCE_BLOBS=/data/blobs \
    REINFORCE_FONTS=/data/fonts

USER reinforce
WORKDIR /data

VOLUME ["/data"]
EXPOSE 3000

HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${REINFORCE_PORT}/api/meta" || exit 1

ENTRYPOINT ["reinforce"]
CMD ["serve"]
