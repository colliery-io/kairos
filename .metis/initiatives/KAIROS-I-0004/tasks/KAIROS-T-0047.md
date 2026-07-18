---
id: m4-release-image-and-publish
level: task
title: "M4: Release image and publish workflow"
short_code: "KAIROS-T-0047"
created_at: 2026-07-11T12:01:02.266183+00:00
updated_at: 2026-07-15T23:56:30.204128+00:00
parent: KAIROS-I-0004
blocked_by: [KAIROS-T-0038, KAIROS-T-0039]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0004
---

# M4: Release image and publish workflow

## Parent Initiative

[[KAIROS-I-0004]]

## Objective

The A-0013 release artifact: single multi-stage OCI image (Leptos assets + server binary) and its publish workflow — closing the deployment story.

## Acceptance Criteria

## Acceptance Criteria

- [x] Multi-stage Dockerfile (repo root): stage 1 builds the Leptos/WASM bundle (trunk) + the release server binary WITH `--features embed-web`; stage 2 = debian:bookworm-slim + libpq5 (documented choice). Serves / (GUI), /api, /mcp, /scim/v2, /healthz. NOTE: /readyz and /metrics are not implemented in the server yet (KAIROS-T-0049) — not served, not claimed.
- [x] Reference compose (`deploy/docker-compose.yaml`): Caddy + kairos image + postgres:16, external IdP (A-0016); dev profile in .angreal/docker-compose.yaml left untouched; documented in A-0013 terms in README + deploy/.env.example + deploy/Caddyfile.
- [x] Release workflow: `image` job in release.yml builds+pushes to ghcr.io/colliery-io/kairos tagged {version}, never latest; actionlint clean (containerized).
- [x] Smoke: built image run against compose Postgres + Dex → /healthz 200 (used in place of the unimplemented /readyz), GUI shell served at /, authenticated /api/whoami round-trip recorded (alice/demo/admin, real Dex token).

## Implementation Notes

References KAIROS-A-0013 (decided: one image, one binary), A-0015 (embedded assets), A-0016 (no IdP in the image or compose prod profile).

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-11: Created (gap identified during program close-out).
- 2026-07-15: Transitioned to active. Read A-0013/A-0015/A-0016, T-0038 release.yml, T-0039 embed-web path (kairos-server `embed-web` feature + rust-embed of ../kairos-web/dist; web.rs SPA fallback), task_web.py (trunk 0.21.14, wasm32 target), docker-compose.yaml (dev: postgres:16 + Dex on 5558). Findings shaping the build:
  - **libpq IS required at runtime.** diesel `postgres` feature links libpq (C); the migrate-on-boot path (main.rs `connect_and_migrate_public`) and the blocking pool use sync `diesel::PgConnection`. diesel-async/tokio-postgres are pure-Rust but don't cover migrations. Runtime base = debian:bookworm-slim + libpq5 (distroless would need libpq + transitive libs hand-copied). Verifying with ldd in-container.
  - reqwest = rustls-tls (no openssl in lockfile); jsonwebtoken 9 = ring. Only native runtime lib is libpq. Builder needs libpq-dev + gcc + pkg-config.
  - **/readyz and /metrics are NOT implemented** — both only appear in web.rs RESERVED_PREFIXES; the sole real health route is `/healthz` (app.rs:156 -> "ok"). A-0013 names /readyz + /metrics but no task built them. Deviation from AC wording ("/readyz green"): using /healthz as the implemented liveness probe for compose healthcheck + smoke; /metrics deferred to T-0049, /readyz likewise unbuilt (flagging as a T-0049-adjacent gap).
  - Issuer/token: Dex issuer is fixed `http://localhost:5558/dex`; auth validates token `iss` == configured OIDC_ISSUER_URL (set_issuer). Smoke networking solved WITHOUT touching the shared stack: run the smoke server + curl sidecar sharing the Dex container netns (`--network container:kairos-dex`), so localhost:5558=Dex (issuer matches, discovery+JWKS reachable) and kairos-postgres:5432=Postgres. whoami tenant: `seed-demo --force` one-shot then serve with KAIROS_SINGLE_TENANT=demo (alice is demo org admin). Token via password grant (kairos-cli client, alice@kairos.test/alice-password).
  - Multi-arch: amd64-only for the published workflow v1 (CI runners are amd64; QEMU cross-build of a Rust+wasm+trunk workspace is slow/flaky) — decision recorded in release.yml with the arm64 enablement path noted. Local smoke builds native arm64.
- 2026-07-15: IMPLEMENTED + VERIFIED. Files: `Dockerfile` + `.dockerignore` (repo root), `deploy/docker-compose.yaml` + `deploy/Caddyfile` + `deploy/.env.example`, `image` job appended to `.github/workflows/release.yml` at the T-0047 anchor, README "Deployment" section.
  Self-checks: hadolint clean (`docker run --rm -i hadolint/hadolint < Dockerfile` — fixed DL3003 via WORKDIR, DL3008 suppressed with rationale); actionlint clean (`docker run --rm -v $PWD:/repo rhysd/actionlint .github/workflows/release.yml`); `docker compose -f deploy/docker-compose.yaml config` valid; `cargo fmt --check` clean (no Rust changed — clippy/unit unaffected; `angreal test integration` deliberately NOT run because it does `compose down -v`, which would destroy the shared stack T-0045 is using).
  Build fix during dev: first build hit `E0463 can't find crate for core` — the wasm32 target was added at `/` (image default toolchain) before COPY, but rust-toolchain.toml pins the toolchain; moved `rustup target add wasm32-unknown-unknown` to AFTER `COPY . .` so it lands on the pinned toolchain.
  SMOKE TRANSCRIPT (image `kairos:smoke`, linux/arm64, against compose Postgres + Dex, sidecars sharing kairos-dex netns):
  - libpq verdict: `ldd` on /usr/local/bin/kairos-server → `libpq.so.5 => /lib/aarch64-linux-gnu/libpq.so.5` — confirms libpq is dynamically linked, debian-slim+libpq5 base is correct (distroless would need the transitive chain).
  - `seed-demo --force` → applied public migration 20260709000000, seeded demo tenant (alice org admin).
  - serve container (KAIROS_SINGLE_TENANT=demo) → /healthz 200 on first poll (migrate-on-boot worked).
  - Real Dex token (alice, password grant, kairos-cli) → `/api/whoami` HTTP 200: user alice@kairos.test, org `demo` role `admin`, team platform. Single-artifact GUI: `/` served embedded index.html; referenced `/kairos-web-<hash>.js` → 200 text/javascript and `/kairos-web-<hash>_bg.wasm` → 200 application/wasm (embed-web proven end-to-end).
  - Image size: **187MB** (`docker images`; `docker image inspect .Size` reports the compressed/attestation-manifest figure ~41MB — 187MB is the real on-disk size). Base: debian:bookworm-slim + libpq5 + ca-certificates + curl.
  Shared stack left UP and healthy; no stray containers (smoke self-cleans). Deviations: (1) /readyz+/metrics unimplemented → smoke/healthcheck use /healthz, T-0049 dependency documented; (2) amd64-only published image v1 (arm64 path documented); (3) integration tier skipped to protect the shared stack (infra-only change, no Rust touched).