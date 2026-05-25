# syntax=docker/dockerfile:1.7

# ---- Builder ----
FROM rust:1.83-slim AS builder
WORKDIR /build

# Cache cargo dependencies separately from source.
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY api/Cargo.toml ./api/
COPY worker/Cargo.toml ./worker/
COPY shared/Cargo.toml ./shared/

# Dummy sources so the dep build can happen without real code.
RUN mkdir -p api/src worker/src shared/src \
    && echo "fn main(){}" > api/src/main.rs \
    && echo "fn main(){}" > worker/src/main.rs \
    && echo "" > shared/src/lib.rs \
    && cargo build --release -p api \
    && rm -rf api/src worker/src shared/src

# Real source build.
COPY shared/src ./shared/src
COPY api/src ./api/src
COPY worker/src ./worker/src
RUN cargo build --release -p api

# ---- Runtime ----
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 65532 --shell /usr/sbin/nologin coral

COPY --from=builder /build/target/release/api /usr/local/bin/api

USER 65532:65532
ENV PORT=8080
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/api"]
