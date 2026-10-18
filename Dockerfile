# syntax=docker/dockerfile:1
FROM --platform=$BUILDPLATFORM node:22-bookworm-slim AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY --from=web /web/dist web/dist
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p grocery-cli \
 && cp target/release/grocery /grocery

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl tzdata \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --uid 1000 --user-group --no-create-home --shell /usr/sbin/nologin grocery \
 && mkdir -p /library /data \
 && chown grocery:grocery /library /data

COPY --from=build /grocery /usr/local/bin/grocery

ENV GROCERY_LIBRARY=/library \
    GROCERY_DB=/data/grocery.sqlite \
    GROCERY_LISTEN=0.0.0.0:8080 \
    TZ=Europe/Amsterdam
VOLUME ["/library", "/data"]
EXPOSE 8080
USER grocery

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/api/status || exit 1

ENTRYPOINT ["grocery"]
CMD ["serve"]
