FROM rust:1.98-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/app/target,sharing=locked \
    cargo build --release --bin log-inbox-collector \
    && cp /app/target/release/log-inbox-collector /tmp/log-inbox-collector

FROM debian:bookworm-slim

ARG APP_UID=1000
ARG APP_GID=1000

RUN groupadd --gid "$APP_GID" app \
  && useradd --uid "$APP_UID" --gid "$APP_GID" --create-home --home-dir /app app \
  && apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /tmp/log-inbox-collector /usr/local/bin/log-inbox-collector
RUN mkdir -p /data && chown app:app /data

EXPOSE 8787
USER app
CMD ["log-inbox-collector"]
