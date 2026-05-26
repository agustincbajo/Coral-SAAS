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

# --- coral binary ---
# Option A (recommended): pull a prebuilt release from upstream Coral.
# When Coral publishes container images via GitHub Container Registry,
# uncomment and pin to a specific SHA digest.
#
#   COPY --from=ghcr.io/agustincbajo/coral:v0.41@sha256:<digest> \
#        /usr/local/bin/coral /usr/local/bin/coral
#
# Option B (current): download the release tarball at build time.
# Replace CORAL_VERSION and the SHA when shipping.
#
ARG CORAL_VERSION=v0.41.0
RUN curl -fsSL "https://github.com/agustincbajo/Coral/releases/download/${CORAL_VERSION}/coral-${CORAL_VERSION}-x86_64-unknown-linux-gnu.tar.gz" \
      -o /tmp/coral.tar.gz \
    && tar -xzf /tmp/coral.tar.gz -C /usr/local/bin/ coral \
    && chmod +x /usr/local/bin/coral \
    && rm /tmp/coral.tar.gz \
    || echo "WARNING: Coral release fetch failed — worker will run in mock-mode (see coral_runner.rs)"

# --- trufflehog (secret scan pre-bootstrap, GAP #68) ---
ARG TRUFFLEHOG_VERSION=3.83.7
RUN curl -fsSL "https://github.com/trufflesecurity/trufflehog/releases/download/v${TRUFFLEHOG_VERSION}/trufflehog_${TRUFFLEHOG_VERSION}_linux_amd64.tar.gz" \
      -o /tmp/trufflehog.tar.gz \
    && tar -xzf /tmp/trufflehog.tar.gz -C /usr/local/bin/ trufflehog \
    && chmod +x /usr/local/bin/trufflehog \
    && rm /tmp/trufflehog.tar.gz \
    || echo "WARNING: trufflehog fetch failed — secret scan will be skipped"

COPY --from=builder /build/target/release/worker /usr/local/bin/worker

USER 65532:65532
WORKDIR /tmp
ENTRYPOINT ["/usr/local/bin/worker"]
