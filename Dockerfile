# syntax=docker/dockerfile:1

# Kairos single-artifact OCI image (KAIROS-T-0047, per KAIROS-A-0013 "one
# image, one binary" and KAIROS-A-0015 "Leptos CSR served from the root of
# the server binary"). One multi-stage build: stage 1 compiles the Leptos
# WASM bundle AND the release server binary WITH the assets embedded; stage 2
# is a minimal Debian runtime carrying only the binary + its lone native
# dependency (libpq).
#
# The image serves everything from one process on 8080: / (GUI SPA), /api,
# /mcp, /scim/v2, and /healthz. (/readyz and /metrics from KAIROS-A-0013 are
# not implemented yet — tracked under KAIROS-T-0049.)
#
# Configuration is 12-factor env vars only (KAIROS-A-0013); identity is
# external (KAIROS-A-0016): point OIDC_ISSUER_URL / OIDC_AUDIENCE at the
# customer IdP. Migrations run on boot (KAIROS-T-0007), so the container is
# self-provisioning against DATABASE_URL.

# ---------------------------------------------------------------------------
# Stage 1 — builder: wasm GUI bundle + release server binary (embed-web)
# ---------------------------------------------------------------------------
# Pinned to the workspace toolchain (rust-toolchain.toml: 1.93.0) so the
# image build uses the exact compiler the repo builds with.
FROM rust:1.93-slim-bookworm AS builder

# Native build dependencies:
#   - libpq-dev + pkg-config: diesel's `postgres` feature links libpq (C).
#   - gcc/cc comes with the rust image (ring, cc-rs).
#   - ca-certificates: cargo/trunk fetch over TLS.
# reqwest uses rustls (no openssl), so no libssl-dev is needed.
# The base image tag is pinned; pinning apt point versions across bookworm
# updates is brittle and buys nothing here.
# hadolint ignore=DL3008
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        libpq-dev \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# The Leptos CSR toolchain (KAIROS-A-0015 / .angreal/task_web.py): trunk,
# pinned to the same version the angreal task installs so the image and dev
# builds produce identical bundles. Installed with the image's default
# toolchain (no repo present yet) so this layer caches independently of source.
ARG TRUNK_VERSION=0.21.14
RUN cargo install trunk --version "${TRUNK_VERSION}" --locked

WORKDIR /build
COPY . .

# Add the wasm32 target AFTER the copy: the repo's rust-toolchain.toml pins
# the toolchain, and the target must be installed for THAT toolchain (adding
# it earlier, at /, lands on the default toolchain and trunk's cargo build
# then can't find `core` for wasm32 — E0463).
RUN rustup target add wasm32-unknown-unknown

# 1) Build the GUI wasm bundle (release: opt-level z + wasm-opt) into
#    crates/kairos-web/dist — exactly what `angreal web build --release`
#    produces. 2) Compile the server WITH the embed-web feature so rust-embed
#    bakes crates/kairos-web/dist into the binary (KAIROS-A-0013 single
#    artifact). Order matters: the embed macro reads dist at compile time.
WORKDIR /build/crates/kairos-web
RUN trunk build --release
WORKDIR /build
RUN cargo build --release -p kairos-server --features embed-web \
    && strip target/release/kairos-server

# ---------------------------------------------------------------------------
# Stage 2 — runtime: minimal Debian + libpq only
# ---------------------------------------------------------------------------
# debian:bookworm-slim (not distroless): the binary dynamically links libpq,
# whose own transitive deps (libgssapi-krb5, libldap, libsasl2, ...) make a
# hand-assembled distroless image fragile. bookworm-slim + libpq5 is the
# smallest base that satisfies the linker cleanly and matches the builder's
# libpq major version. Verdict verified with `ldd` post-build (see task doc).
FROM debian:bookworm-slim AS runtime

# libpq5: the ONLY native runtime dependency (sync diesel migrations + the
# blocking pool link libpq; the async pool is pure-Rust tokio-postgres and
# reqwest is rustls). ca-certificates: OIDC discovery over TLS against the
# customer IdP (KAIROS-A-0016). curl: HEALTHCHECK + an ops probe tool.
# hadolint ignore=DL3008  # base image tag is pinned; see builder-stage note.
RUN apt-get update && apt-get install -y --no-install-recommends \
        libpq5 \
        ca-certificates \
        curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home --shell /usr/sbin/nologin kairos

COPY --from=builder /build/target/release/kairos-server /usr/local/bin/kairos-server

USER kairos

# The code default is 127.0.0.1:8080 (dev). In a container we must listen on
# all interfaces to be reachable; everything else is deploy-time env only.
ENV KAIROS_BIND_ADDR=0.0.0.0:8080 \
    KAIROS_LOG_FORMAT=json

EXPOSE 8080

# Liveness only: /healthz has no dependencies (KAIROS-A-0013). Readiness
# (/readyz) is not implemented yet (KAIROS-T-0049).
HEALTHCHECK --interval=10s --timeout=3s --start-period=20s --retries=5 \
    CMD curl -fsS http://localhost:8080/healthz || exit 1

ENTRYPOINT ["/usr/local/bin/kairos-server"]
CMD ["serve"]
