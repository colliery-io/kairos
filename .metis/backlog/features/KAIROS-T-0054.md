---
id: id-token-as-bearer-mode-for-opaque
level: task
title: "ID-token-as-bearer mode for opaque-access-token IdPs (Google Workspace)"
short_code: "KAIROS-T-0054"
created_at: 2026-07-17T02:15:25.071172+00:00
updated_at: 2026-07-17T02:45:46.861859+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#feature"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: NULL
---

# ID-token-as-bearer mode for opaque-access-token IdPs (Google Workspace)

## Objective **[REQUIRED]**

Let a Kairos deployment use an OIDC issuer whose **access token is opaque** (not a
JWT) — most importantly **Google / Google Workspace**, where the access token is a
`ya29.…` string that cannot be validated by local JWKS. The issuer's **ID token**
*is* an RS256 JWT that Kairos's existing middleware validates correctly (`iss`,
`aud`, `exp`, signature). This task adds a configurable mode where the browser GUI
(and CLI) present the **id_token** as the `/api` bearer instead of the access token,
so Kairos's stateless local-JWKS validation (KAIROS-A-0010) keeps working unchanged.

This is the single real code gap between "Kairos is BYO-OIDC" (A-0016) and "point it
at Google Workspace." Dex and Keycloak mint JWT access tokens, so it never surfaced
in dev/test.

## Backlog Item Details **[CONDITIONAL: Backlog Item]**

### Type
- [x] Feature - New functionality or enhancement

### Priority
- [x] P2 - Medium (nice to have) — unblocks Google Workspace as an IdP; no current user is blocked

### Business Justification **[CONDITIONAL: Feature]**
- **User Value**: Companies on Google Workspace can use their existing corporate
  SSO with Kairos with no extra IdP to run.
- **Business Value**: Removes the biggest practical adoption blocker for the large
  segment of companies standardized on Google Workspace.
- **Effort Estimate**: S–M (server: none required if GUI/CLI switch the bearer;
  GUI/CLI: small; docs + one integration test).

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [x] A config flag (`KAIROS_API_BEARER=id_token|access_token`, default
      `access_token`) selects which token the **GUI** sends as the `/api` bearer.
      When `id_token`, `crates/kairos-web` stores and sends `id_token`.
      → `config.rs` `ApiBearer` + `web.rs` `/api/config` + `kairos-web/auth.rs`.
- [x] The **CLI** device-grant login honors the same selection for its stored
      bearer. → `--bearer` flag, persisted in the credential entry, refresh
      re-selects the same kind (`oidc.rs`/`credentials.rs`/`main.rs`/`provider.rs`).
- [x] Server middleware validates a Google-shaped ID token (`iss =
      https://accounts.google.com`, `aud = <client id>`, RS256) and rejects an
      opaque `ya29.…` string. → new test
      `google_shaped_id_token_validates_and_opaque_access_token_is_rejected`
      (no server change needed — middleware is token-kind agnostic).
- [x] `email` claim present + JIT-upsert unchanged. → the new test asserts
      `claims.email`; JIT path (`jit_upsert_user`) untouched and integration-green.
- [x] Docs: "Google Workspace as your IdP" runbook in `README.md` (OAuth Web
      client, `OIDC_ISSUER_URL=https://accounts.google.com`, `OIDC_AUDIENCE`,
      `KAIROS_API_BEARER=id_token`, `hd` domain restriction, scopes); env surface
      in `deploy/.env.example` and Helm `values.yaml`/`README.md`.
- [x] End-to-end proof: unit tests cover token selection + validation both
      directions; a **documented manual runbook** covers the real-Google path
      (a live Google OAuth client is required, so it cannot run in CI — this is
      the AC's explicit fallback).

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
- Grounded in current code (verified 2026-07-16):
  - `crates/kairos-server/src/middleware/auth.rs` validates **RS256** signature +
    `iss` + `exp` + `aud` (against `OIDC_AUDIENCE`) via a `kid`-keyed JWKS cache with
    discovery + rotation. It does not care whether the JWT is labelled "access" or
    "id" — it just needs a valid JWT with the right `iss`/`aud`. Google's ID token
    satisfies this; Google's access token does not (opaque).
  - `crates/kairos-web/src/auth.rs` stores `access_token` and sends it as the bearer
    ("The bearer sent to `/api` is the **access token**"). This is the line to make
    configurable.
- Prefer a per-deployment config flag over auto-detection; keep the default
  `access_token` so Dex/Keycloak deployments are unchanged.
- Google Workspace specifics: use an **OAuth Web application** client for the GUI
  (Authorization Code + PKCE). Set the `hd` parameter and/or verify the `hd` claim
  to lock logins to the corporate domain. Consumer Gmail should be excluded.

### Dependencies
- Interacts with [[KAIROS-T-0055]] (multi-audience): with Google each OAuth client is
  a distinct `aud`, so a multi-client deployment (GUI + CLI + service accounts) needs
  T-0055 as well. GUI-only Google SSO needs only this task.

### Risk Considerations
- ID tokens are intended as identity assertions, not API bearers; presenting one as a
  bearer is common in practice for local-JWKS validation but document the choice.
  Token lifetime/refresh: Google ID tokens are ~1h — ensure the GUI refresh path
  re-fetches a fresh `id_token`, not only the access token.

## Status Updates **[REQUIRED]**

### 2026-07-17 — Code landed (server + GUI + CLI + middleware test), unit-green

**Implemented:**
- **Server config** (`config.rs`): new `ApiBearer` enum + `KAIROS_API_BEARER`
  env (`access_token` default | `id_token`), invalid values fail fast. Tests:
  default = AccessToken, `id_token` parses, `jwt` rejected.
- **Server `/api/config`** (`web.rs`): `SpaConfig`/`WebAuth` now carry
  `api_bearer` (as_str on the wire). OpenAPI doc-stub updated.
- **GUI** (`kairos-web/auth.rs`): `AuthConfig.api_bearer` (`#[serde(default)]`
  → AccessToken for old servers), `TokenResponse.id_token`, `bearer_for()`;
  `install()` selects the bearer from cached config. Falls back to access
  token if id_token absent on a refresh (keeps session alive). 3 new tests.
- **CLI** (`oidc.rs`/`credentials.rs`/`main.rs`/`provider.rs`): `ApiBearer`
  clap ValueEnum, `--bearer` login flag (default access_token), `id_token`
  parsed from TokenResponse, persisted in the credential entry
  (`#[serde(default)]` = backward compatible), refresh re-selects same kind.
  Login errors clearly if id_token requested but absent. New tests.
- **Middleware test** (`middleware/auth.rs`): proves a Google-shaped ID token
  (`iss=https://accounts.google.com`, `aud=<client id>`) validates and an
  opaque `ya29.…` access token is rejected — no server-side change needed;
  the middleware is token-kind agnostic (validates any RS256 JWT).

**Verified:** `cargo check` (server+cli+web+tests) clean; unit tests green —
server config 6/6, auth 8/8 (incl. new Google test), web auth 6/6, cli 23/23.

**Remaining:** docs (Google Workspace section + env surface in deploy
`.env.example` and Helm `values.yaml`), then full gate (fmt/clippy/web-lint +
integration) + commit + complete.

### 2026-07-17 — Docs + full gate GREEN; complete

- **Docs:** README "Google Workspace as your IdP" runbook added; env surface in
  `deploy/.env.example`, Helm `values.yaml` + `configmap.yaml` (renders
  `KAIROS_API_BEARER` in all 3 CI value sets) + Helm `README.md`; OpenAPI
  `/api/config` doc-stub updated.
- **Gate (all green):** `cargo fmt --check` clean; clippy (server+cli+web,
  all-targets) clean; `angreal web lint` clean; **unit** suite all crates 0
  failures (incl. new tests); **openapi** 5/5; **integration** all 27 targets
  pass, clean teardown. `helm lint` clean, 3-value-set render OK.
- All 6 ACs met. Scope delivered exactly per plan: server + GUI + CLI + a
  server-side proof test + docs. No behavior change for existing Dex/Keycloak
  deployments (default `access_token`), confirmed by the green integration run.

**Deferred (filed):** multi-client Google (GUI+CLI+svc together) needs
[[KAIROS-T-0055]] multi-audience validation. Marking T-0054 complete.