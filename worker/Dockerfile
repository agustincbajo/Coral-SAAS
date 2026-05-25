# syntax=docker/dockerfile:1.7

# ---- Builder ----
FROM rust:1.83-slim AS builder
WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY api/Cargo.toml ./api/
COPY worker/Cargo.toml ./worker/
COPY shared/Cargo.toml ./shared/

RUN mkdir -p api/src worker/src shared/src \
    && echo "fn main(){}" > api/src/main.rs \
    && echo "fn main(){}" > worker/src/main.rs \
    && echo "" > shared/src/lib.rs \
    && cargo build --release -p worker \
    && rm -rf api/src worker/src shared/src

COPY shared/src ./shared/src
COPY api/src ./api/src
COPY worker/src ./worker/src
RUN cargo build --release -p worker

# ---- Runtime ----
# Worker needs `git` (clone source repos), `coral` binary (vendored from
# upstream Coral release), and trufflehog (secret scan pre-bootstrap).
# Pinned digests should be added before launch — placeholders for now.
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates git curl zstd \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 65532 --shell /usr/sbin/nologin coral

# TODO: copy `coral` binary from Coral upstream release.
#   Example: COPY --from=ghcr.io/agustincbajo/coral:v0.41 /usr/local/bin/coral /usr/local/bin/coral
# TODO: install trufflehog (https://github.com/trufflesecurity/trufflehog)

COPY --from=builder /build/target/release/worker /usr/local/bin/worker

USER 65532:65532
WORKDIR /tmp
ENTRYPOINT ["/usr/local/bin/worker"]
