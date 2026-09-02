FROM rust:1.98-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock* ./
COPY crates ./crates
RUN cargo build --release --bin log-inbox-collector

FROM debian:bookworm-slim

RUN useradd --system --create-home --home-dir /app app \
  && apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/log-inbox-collector /usr/local/bin/log-inbox-collector
RUN mkdir -p /data && chown app:app /data

EXPOSE 8787
USER app
CMD ["log-inbox-collector"]
