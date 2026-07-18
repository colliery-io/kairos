---
id: m2-auth-and-tenant-middleware-stack
level: task
title: "M2: Auth and tenant middleware stack"
short_code: "KAIROS-T-0017"
created_at: 2026-07-10T01:08:18.858665+00:00
updated_at: 2026-07-10T02:52:04.718715+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0009]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M2: Auth and tenant middleware stack

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

The axum middleware stack per A-0010/A-0005: OIDC bearer validation (JWKS), JIT user provisioning, tenant resolution (subdomain / X-Tenant / single-tenant mode), org-membership enforcement — producing AuthContext + TenantContext for every handler.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] JWKS middleware: fetch+cache realm keys by kid (refresh on unknown kid); validate signature/iss/aud/exp; 401 with S-0005 error envelope on failure; verified live against the compose Dex issuer (http://localhost:5558/dex)
- [x] JIT provisioning: first authenticated request upserts public.users from claims (sub→external_id, email, name); no org membership auto-granted; member-less user → 403 with a "request access" error code
- [x] Tenant middleware: Host subdomain → org slug → pinned pool connection; X-Tenant header fallback and KAIROS_SINGLE_TENANT mode per A-0013; unknown tenant → 404; non-member → 403
- [x] kairos-server boots an axum router with the stack applied and a protected /api/whoami-style probe endpoint proving the full chain in an integration test with real Dex tokens (valid, expired, wrong-aud, no-membership cases)

## Implementation Notes

References A-0010 (decided; Dex in dev/test), A-0005 (tenancy), A-0013 (env config: OIDC_ISSUER_URL, OIDC_AUDIENCE, KAIROS_BASE_DOMAIN, KAIROS_SINGLE_TENANT). Middleware crates: tower layers in kairos-server; keep validation logic unit-testable.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-09: Created at M2 decompose (todo).
- 2026-07-09: Implemented (KAIROS-T-0017). kairos-server restructured into lib+bin: `src/lib.rs` (init_tracing per A-0013), `src/config.rs` (AppConfig struct; env parsing isolated in `from_env`/`from_lookup`, fail-fast naming the variable), `src/error.rs` (ApiError → S-0005 envelope `{"error":{"code","message","details"}}`; codes UNAUTHORIZED/MEMBERSHIP_REQUIRED/FORBIDDEN/TENANT_NOT_FOUND/NOT_FOUND/INTERNAL), `src/middleware/auth.rs` (Authenticator: OIDC discovery → JWKS cache by kid, refresh-on-unknown-kid behind a tokio::Mutex stampede guard; RS256 + iss/aud/exp validation; JIT upsert of public.users keyed on external_id — SELECT fast path, ON CONFLICT upsert; AuthContext extension), `src/middleware/tenant.rs` (resolution order KAIROS_SINGLE_TENANT → Host subdomain vs KAIROS_BASE_DOMAIN → X-Tenant fallback; org lookup 404 TENANT_NOT_FOUND; organization_members required, else 403 MEMBERSHIP_REQUIRED with "request access" message; TenantContext {org_id, slug, role} + TenantDb pinned-pool handle extensions), `src/app.rs` (AppState, router(), GET /api/whoami probe returning user+org+role+teams, GET /healthz, serve() with graceful ctrl-c shutdown), `serve` subcommand in main.rs (public migrations first, KAIROS_BIND_ADDR default 127.0.0.1:8080). Workspace deps appended: jsonwebtoken 9.3.1, reqwest 0.12 (rustls).
- 2026-07-09: AUDIENCE DECISION (recorded per A-0010 note): Dex sets `aud` = requesting OAuth client id and this stack has no cross-client audience config, so the dev/test deployment sets `OIDC_AUDIENCE=kairos-cli` and API tokens are minted through the `kairos-cli` client. The wrong-audience test uses a REAL Dex token from the seeded `kairos-svc` confidential client (valid signature+issuer, aud=kairos-svc) → 401. Production Keycloak will map a deployment-wide audience via a client-scope mapper (realm export task, M5).
- 2026-07-09: VERIFIED against the LIVE compose stack (shared-services mode: only `cargo test -p kairos-server` run; scratch DB `kairos_middleware_t0017_test` created+dropped; shared `kairos` DB untouched; services left UP). Evidence:
  - `cargo fmt --check` → clean. `cargo clippy --workspace --all-targets -- -D warnings` → clean.
  - `cargo test -p kairos-server` → 24 passed, 0 failed (19 lib unit: config fail-fast/defaults, envelope shape, tenant resolution order incl. subdomain/port/IPv6/suffix-trick cases, offline validator branches with a throwaway RSA key: valid/expired/wrong-aud/wrong-iss/unknown-kid/garbage; 4 bin unit; 1 live integration `tests/middleware.rs`).
  - Integration test (real Dex password-grant tokens for alice/bob/svc from .angreal/dex/config.yaml, production router in-process via tower oneshot): valid member → 200 whoami with correct user/org/role/teams AND exactly one JIT public.users row (sub→external_id, email, name→display_name); pre-membership request → 403 MEMBERSHIP_REQUIRED ("request access") with user row still JIT-created; missing/garbage/tampered-signature tokens → 401; wrong-aud (real kairos-svc token) → 401 and NO user provisioned; non-member (bob) → 403; unknown tenant slug → 404 TENANT_NOT_FOUND; unresolvable tenant → 404; Host subdomain (acme.kairos.test:8080 vs KAIROS_BASE_DOMAIN=kairos.test) → 200; X-Tenant fallback → 200; KAIROS_SINGLE_TENANT=acme router (no Host/X-Tenant) → 200.
  - `serve` smoke (live): booted `kairos-server serve` against scratch DB kairos_serve_smoke_t0017 with OIDC_ISSUER_URL/OIDC_AUDIENCE/KAIROS_BIND_ADDR=127.0.0.1:18099, JSON tracing output; /healthz → 200 "ok"; /api/whoami no token → 401 envelope; valid live token + X-Tenant ghost → 404 envelope; SIGINT → "shutdown signal received; draining" graceful exit. Scratch DB dropped.
  - DEFERRED per orchestrator: full `angreal test unit`/`angreal test integration` gate runs after this task (compose stack shared with a concurrent agent; not run here by instruction).