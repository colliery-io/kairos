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
#   - cmake + make + libclang-dev: the summarizer of the code index (COLLIERY-T-1853,
#     the server feature `llama`) builds llama.cpp from C++ source with cmake,
#     and bindgen reads its headers through libclang.
# reqwest uses rustls (no openssl), so no libssl-dev is needed.
# The base image tag is pinned; pinning apt point versions across bookworm
# updates is brittle and buys nothing here.
# hadolint ignore=DL3008
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
        libpq-dev \
        g++ \
        cmake \
        make \
        libclang-dev \
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
# `llama` (COLLIERY-T-1853): the builder of the base code index writes its
# summaries with llama.cpp on the CPU (OpenMP). llama.cpp is a set of shared
# libraries, with one CPU backend of ggml for each level of the architecture
# (COLLIERY-T-2526): at run time ggml loads the best one for the host (on
# arm64, dotprod and fp16 when the CPU has them). So the image runs on any
# host of its architecture and still uses its vector instructions.
RUN cargo build --release -p kairos-server --features embed-web,llama \
    && strip target/release/kairos-server \
    && mkdir -p /build/llama/lib /build/llama/backends \
    && cp -P target/release/build/llama-cpp-sys-2-*/out/lib/lib*.so* /build/llama/lib/ \
    && cp target/release/build/llama-cpp-sys-2-*/out/backends/libggml-cpu-*.so /build/llama/backends/

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

# 4) The pinned rust-analyzer and the pinned std source of the code index
#    (COLLIERY-T-1858, COLLIERY-T-1860), for the Rust call edges of the
#    builder. The same rule as the model above: fetched and checked by sha256
#    HERE, so the builder downloads nothing at run time. The std source is
#    unpacked next to its archive.
RUN KAIROS_INDEX_RUST_ANALYZER=/build/kairos-index/bin/rust-analyzer \
    KAIROS_INDEX_RUST_SRC=/build/kairos-index/rust-src/rust-src-1.99.0.tar.gz \
    cargo run --release -p kairos-index --example fetch_rust_analyzer

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
# The builder of the base code index (COLLIERY-T-1853):
#   - libgomp1: llama.cpp runs its CPU threads with OpenMP;
#   - git: the bare clone of each indexed repository.
# hadolint ignore=DL3008  # base image tag is pinned; see builder-stage note.
RUN apt-get update && apt-get install -y --no-install-recommends \
        libpq5 \
        libstdc++6 \
        libgomp1 \
        git \
        ca-certificates \
        curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home --shell /usr/sbin/nologin kairos

# The Rust toolchain of the SCIP run of the code index (COLLIERY-T-1849): it
# reads `rustc --print sysroot` and `cargo metadata`, and links the bin/ and
# lib/ of the sysroot into its stand-in sysroot. It builds nothing. The
# minimal profile (rustc, cargo, rust-std) of the toolchain of this
# repository; the pinned std source replaces rust-src (COLLIERY-T-1860).
# RUSTUP_TOOLCHAIN makes each repository use it, so a rust-toolchain.toml of
# another version installs nothing at run time. cargo keeps its registry
# under the code index folder, which the runtime user can write.
ENV RUSTUP_HOME=/usr/local/rustup \
    PATH=/usr/local/cargo/bin:$PATH \
    RUSTUP_TOOLCHAIN=1.93.0 \
    RUSTUP_AUTO_INSTALL=0
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | CARGO_HOME=/usr/local/cargo sh -s -- -y --no-modify-path \
            --profile minimal --default-toolchain 1.93.0 \
    && rm -rf /usr/local/rustup/downloads /usr/local/rustup/tmp \
        /usr/local/rustup/toolchains/*/share/doc \
    && rustc --version && cargo --version \
    && install -d -o 10001 -g 10001 /var/lib/kairos/code-index

# OCI provenance. `licenses` is the machine-readable answer to "what may I do
# with this image?"; scanners and registries read it, and until 0.2.0 the
# published image answered nothing at all.
LABEL org.opencontainers.image.title="kairos" \
      org.opencontainers.image.description="Flight Levels work management — GUI, REST, MCP and SCIM from one binary" \
      org.opencontainers.image.source="https://github.com/colliery-io/kairos" \
      org.opencontainers.image.licenses="Apache-2.0"

COPY --from=builder /build/target/release/kairos-server /usr/local/bin/kairos-server
# The shared libraries of llama.cpp, and the CPU backends of ggml next to the
# binary, where the summarizer looks for them (COLLIERY-T-2526).
COPY --from=builder /build/llama/lib/ /usr/local/lib/
COPY --from=builder /build/llama/backends/ /usr/local/bin/
RUN ldconfig && ! ldd /usr/local/bin/kairos-server | grep "not found"
# Apache-2.0 section 4(d): a redistributable build carries the licence and the
# NOTICE, so an operator who only ever has the image can still read both.
COPY --from=builder /build/LICENSE /build/NOTICE /usr/share/doc/kairos/
# The embedding model, owned by the runtime user so nothing needs to write here.
COPY --from=builder --chown=10001:10001 /build/models /var/lib/kairos/models
# The pinned rust-analyzer and the pinned std source (unpacked) of the code
# index.
COPY --from=builder --chown=10001:10001 /build/kairos-index /var/lib/kairos/code-index-tools
# The summary model of the code index (COLLIERY-T-1850): Qwen3-4B-Instruct-2507,
# GGUF Q4_K_M from bartowski, about 2.5 GB, at a fixed revision and checked by
# sha256 at build time.
ADD --chown=10001:10001 \
    --checksum=sha256:2fde00ce69dd4899c70d020845e2638353015bba0fdf161b3eb965f2bca4464e \
    https://huggingface.co/bartowski/Qwen_Qwen3-4B-Instruct-2507-GGUF/resolve/ae44f08e1392f39c0e474af10c3ff8355c8b6688/Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf \
    /var/lib/kairos/models/Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf

USER kairos

# The code default is 127.0.0.1:8080 (dev). In a container we must listen on
# all interfaces to be reachable; everything else is deploy-time env only.
# KAIROS_EMBED_CACHE points at the baked model. Downloading stays DISABLED
# (the LocalConfig default): if the model layer above ever failed to copy, the
# operator gets an error naming the directory rather than a container that
# quietly pulls 65 MB from the internet on first use.
# The KAIROS_INDEX_* paths point the builder of the code index at the baked
# model, rust-analyzer and std source; it downloads none of them. The builder
# stays off until an operator sets KAIROS_CODE_INDEX_DIR (for example to
# /var/lib/kairos/code-index, on a volume).
ENV KAIROS_BIND_ADDR=0.0.0.0:8080 \
    KAIROS_LOG_FORMAT=json \
    KAIROS_EMBED_CACHE=/var/lib/kairos/models \
    KAIROS_INDEX_MODEL=/var/lib/kairos/models/Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf \
    KAIROS_INDEX_RUST_ANALYZER=/var/lib/kairos/code-index-tools/bin/rust-analyzer \
    KAIROS_INDEX_RUST_SRC=/var/lib/kairos/code-index-tools/rust-src/rust-src-1.99.0.tar.gz \
    CARGO_HOME=/var/lib/kairos/code-index/cargo

EXPOSE 8080

# Liveness only: /healthz has no dependencies (KAIROS-A-0013). Readiness
# (/readyz) is not implemented yet (KAIROS-T-0049).
HEALTHCHECK --interval=10s --timeout=3s --start-period=20s --retries=5 \
    CMD curl -fsS http://localhost:8080/healthz || exit 1

ENTRYPOINT ["/usr/local/bin/kairos-server"]
CMD ["serve"]
