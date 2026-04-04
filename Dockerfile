FROM rust:1-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/

RUN cargo build --release --bin reforge && \
    strip target/release/reforge

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    git \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/reforge /usr/local/bin/reforge

RUN useradd --create-home --shell /bin/sh reforge
USER reforge
WORKDIR /home/reforge

ENTRYPOINT ["reforge"]
