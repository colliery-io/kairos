# syntax=docker/dockerfile:1

# Kairos single-artifact OCI image (KAIROS-T-0047, per KAIROS-A-0013 "one
# image, one binary" and KAIROS-A-0015 "Leptos CSR served from the root of
# the server binary"). One multi-stage build: stage 1 compiles the Leptos
# WASM bundle AND the release server binary WITH the assets embedded; stage 2
# is a minimal Debian runtime carrying only the binary + its lone native
# dependency (libpq).
#
# It also carries the local embedding model (KAIROS-T-0189): baked in at build
# time under /var/lib/kairos/models, so retrieval needs no outbound call and
# works air-gapped.
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
#
# TRIXIE, not bookworm, since KAIROS-T-0189. `ort` links a prebuilt ONNX runtime
# built against libstdc++ 13 or newer, and Debian 12 ships GCC 12 — the build
# fails with `undefined reference to std::__cxx11::basic_string::_M_replace_cold`,
# a symbol that simply does not exist in the older runtime. Verified directly:
# the same crate links in 43 s on trixie (GCC 14.2) and not at all on bookworm.
FROM rust:1.93-slim-trixie AS builder

# Native build dependencies:
#   - libpq-dev + pkg-config: diesel's `postgres` feature links libpq (C).
#   - gcc/cc comes with the rust image (ring, cc-rs).
#   - g++: the ONNX runtime under `fastembed` (KAIROS-T-0189) is C++, and
#     linking it needs `-lstdc++`, which the slim rust image does not carry.
#     Without it the release build fails at the link step with
#     `cannot find -lstdc++` — which is how this was found.
#   - ca-certificates: cargo/trunk fetch over TLS.
# reqwest uses rustls (no openssl), so no libssl-dev is needed.
# The base image tag is pinned; pinning apt point versions across bookworm
# updates is brittle and buys nothing here.
# hadolint ignore=DL3008
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        libpq-dev \
        g++ \
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
# The C++ link ordering `ort` needs lives in `.cargo/config.toml`, so it applies
# here, in CI and to a developer on Linux alike rather than only to this build.
RUN cargo build --release -p kairos-server --features embed-web \
    && strip target/release/kairos-server

# 3) Bake the local embedding model into the image (KAIROS-T-0189, A-0021 rule
#    1). fastembed resolves models from a cache directory and downloads on a
#    miss; a stateless container that must reach huggingface.co before it can
#    serve is not "local by default" and breaks air-gapped deployments, so the
#    fetch happens HERE, at build time, and the runtime stage carries the files.
#    `fetch-model` drives LocalProvider itself, so the cache layout is
#    fastembed's own rather than a reproduction of it, and it embeds a probe
#    string — a model that cannot load fails THIS build rather than the first
#    request in production. ~65 MB (bge-small-en-v1.5, statically quantized),
#    chosen by measurement: see crates/kairos-embed/src/local.rs.
RUN cargo run --release -p kairos-embed --bin fetch-model -- /build/models

# ---------------------------------------------------------------------------
# Stage 2 — runtime: minimal Debian + libpq only
# ---------------------------------------------------------------------------
# debian:trixie-slim (not distroless): the binary dynamically links libpq, whose
# own transitive deps (libgssapi-krb5, libldap, libsasl2, ...) make a
# hand-assembled distroless image fragile. slim + libpq5 is the smallest base
# that satisfies the linker cleanly and matches the builder's libpq major
# version.
#
# It must match the BUILDER's Debian release, not merely be recent: the binary
# now carries a C++ runtime (KAIROS-T-0189) and an older libstdc++ here would
# fail at startup rather than at build, which is the worse of the two.
FROM debian:trixie-slim AS runtime

# Native runtime dependencies — two of them since KAIROS-T-0189:
#   - libpq5: sync diesel migrations + the blocking pool link libpq. (The async
#     pool is pure-Rust tokio-postgres and reqwest is rustls.)
#   - libstdc++6: the ONNX runtime behind the local embedding model is C++.
#     Named explicitly rather than relied on as a transitive of the base image,
#     because a base-image change that dropped it would fail at startup rather
#     than at build.
# ca-certificates: OIDC discovery over TLS against the customer IdP
# (KAIROS-A-0016). curl: HEALTHCHECK + an ops probe tool.
# hadolint ignore=DL3008  # base image tag is pinned; see builder-stage note.
RUN apt-get update && apt-get install -y --no-install-recommends \
        libpq5 \
        libstdc++6 \
        ca-certificates \
        curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home --shell /usr/sbin/nologin kairos

# OCI provenance. `licenses` is the machine-readable answer to "what may I do
# with this image?"; scanners and registries read it, and until 0.2.0 the
# published image answered nothing at all.
LABEL org.opencontainers.image.title="kairos" \
      org.opencontainers.image.description="Flight Levels work management — GUI, REST, MCP and SCIM from one binary" \
      org.opencontainers.image.source="https://github.com/colliery-io/kairos" \
      org.opencontainers.image.licenses="Apache-2.0"

COPY --from=builder /build/target/release/kairos-server /usr/local/bin/kairos-server
# Apache-2.0 section 4(d): a redistributable build carries the licence and the
# NOTICE, so an operator who only ever has the image can still read both.
COPY --from=builder /build/LICENSE /build/NOTICE /usr/share/doc/kairos/
# The embedding model, owned by the runtime user so nothing needs to write here.
COPY --from=builder --chown=10001:10001 /build/models /var/lib/kairos/models

USER kairos

# The code default is 127.0.0.1:8080 (dev). In a container we must listen on
# all interfaces to be reachable; everything else is deploy-time env only.
# KAIROS_EMBED_CACHE points at the baked model. Downloading stays DISABLED
# (the LocalConfig default): if the model layer above ever failed to copy, the
# operator gets an error naming the directory rather than a container that
# quietly pulls 65 MB from the internet on first use.
ENV KAIROS_BIND_ADDR=0.0.0.0:8080 \
    KAIROS_LOG_FORMAT=json \
    KAIROS_EMBED_CACHE=/var/lib/kairos/models

EXPOSE 8080

# Liveness only: /healthz has no dependencies (KAIROS-A-0013). Readiness
# (/readyz) is not implemented yet (KAIROS-T-0049).
HEALTHCHECK --interval=10s --timeout=3s --start-period=20s --retries=5 \
    CMD curl -fsS http://localhost:8080/healthz || exit 1

ENTRYPOINT ["/usr/local/bin/kairos-server"]
CMD ["serve"]
