# ==============================================================================
# Multi-stage Dockerfile for Atlantis Home Node (ARM64 / Raspberry Pi & x86_64)
# ==============================================================================

# Stage 1: Build environment
FROM rust:bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    clang \
    cmake \
    git \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/atlantis

# Copy workspace manifest and lock
COPY Cargo.toml Cargo.lock ./

# Copy crates
COPY crates ./crates

# Build release binary for the home_node service
RUN cargo build --release --package home_node --bin home_node

# ==============================================================================
# Stage 2: Minimal runtime image
# ==============================================================================
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    procps \
    curl \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy compiled binary from builder
COPY --from=builder /usr/src/atlantis/target/release/home_node /usr/local/bin/home_node

ENV ATLANTIS_NODE_ID=zero-rpi-home-node
ENV ATLANTIS_DIVIDEND_RATE=0.02
ENV RUST_LOG=info

ENTRYPOINT ["/usr/local/bin/home_node"]
