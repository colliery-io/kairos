---
id: m4-cli-auth-config-whoami
level: task
title: "M4: CLI - auth, config, whoami"
short_code: "KAIROS-T-0036"
created_at: 2026-07-10T22:02:30.030310+00:00
updated_at: 2026-07-14T22:01:17.222065+00:00
parent: KAIROS-I-0004
blocked_by: [KAIROS-T-0024]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0004
---

# M4: CLI - auth, config, whoami

## Parent Initiative

[[KAIROS-I-0004]]

## Objective

The `kairos` CLI's auth and configuration layer per KAIROS-A-0015/A-0010: device-flow login against the deployment's OIDC issuer, credential cache, and a working `whoami`.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] `kairos login --url <deployment>` runs the Device Authorization Grant (prints verification URL + user code, polls), discovers the issuer from the deployment's protected-resource metadata or explicit `--issuer`; tokens + refresh cached in `~/.config/kairos/credentials.json` (0600) keyed by deployment
- [x] Automatic refresh on expiry; `kairos logout` clears; clear errors for unreachable deployment / declined grant
- [x] `kairos whoami` prints user, org, role, teams via kairos-client against /api/whoami
- [x] Exit codes: 0 success, 1 API/validation error, 2 auth error; `--json` on whoami
- [x] Integration test against compose (Dex device flow can be driven headlessly via its login form with reqwest — or document the manual fallback and cover token-cache/refresh/expiry paths with unit tests + a pre-seeded token integration path)

## Implementation Notes

References A-0015 (decided), A-0010 (device grant; Dex enables it), kairos-client (T-0024 completes it). CLI crate exists as a stub from T-0002.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy --workspace --all-targets -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-10: Created at I-0004 decompose (todo).
- 2026-07-13: Active. Read A-0015/A-0010, kairos-client (TokenProvider seam + typed Error), dex config, T-0034 evidence (RFC 9728 metadata at /.well-known/oauth-protected-resource/mcp with authorization_servers). Plan: kairos-cli gains modules error.rs (exit-code mapping 0/1/2), credentials.rs (~/.config/kairos/credentials.json v1, 0600, keyed by normalized deployment URL, KAIROS_CONFIG_DIR test override), oidc.rs (RFC 9728 issuer discovery -> OIDC discovery -> device grant with interval/slow_down handling -> refresh grant), provider.rs (CachedTokenProvider impl of kairos_client::TokenProvider: 30s skew, refresh-on-expiry, persist rotated refresh token, Error::Token actionable re-login message on failure -> exit 2). Subcommands: login --url [--issuer] [--tenant] [--client-id], logout [--url], whoami [--url] [--json]. Deps: workspace tokio/reqwest/serde/serde_json/thiserror only (no workspace Cargo.toml change). Verification: unit tests (store round-trip/perms/corruption, poll classification, expiry skew) + integration test crates/kairos-cli/tests/cli_live.rs (dev-deps kairos-server path + kairos-db/diesel/uuid): scratch DB kairos_cli_m4_test, in-process router on ephemeral port, headless Dex device-flow approval via reqwest POST of user_code + alice credentials to Dex's forms, drives the real `kairos` binary (CARGO_BIN_EXE) through login/whoami/refresh/corrupt-cache/logout. Live-transcript verification with a manually run kairos-server on an ephemeral port to follow.
- 2026-07-14: SHIPPED. `crates/kairos-cli` (my lane only; kairos-client untouched — its TokenProvider seam was sufficient as-is): `src/main.rs` (clap: login/logout/whoami, exit-code mapping via ExitCode), `src/error.rs` (CliError Auth=2/Failure=1; 401+Token→2, 403/404/409/422/transport→1 with actionable messages), `src/credentials.rs` (store v1 keyed by normalized URL; atomic tmp+rename write, file 0600/dir 0700; KAIROS_CONFIG_DIR→XDG_CONFIG_HOME→HOME/.config/kairos; corrupted cache→exit 2 naming the file + fix; missing file=empty store), `src/oidc.rs` (RFC 9728 discovery at /.well-known/oauth-protected-resource/mcp→authorization_servers[0]; OIDC discovery; RFC 8628 device grant with scopes "openid profile email offline_access", verification_uri_complete printed when present, poll honoring interval + slow_down(+5s) + access_denied/expired_token→exit 2; refresh grant), `src/provider.rs` (CachedTokenProvider: re-reads cache per request, 30s skew, refresh-on-expiry persists rotated refresh token, failure→Error::Token with `kairos login --url …` instruction). Deps: workspace-only additions to kairos-cli/Cargo.toml (tokio/reqwest/serde/serde_json/thiserror) + DEV-deps for the live test (kairos-server by path, kairos-db, diesel, uuid, axum, reqwest+cookies, tempfile). Workspace Cargo.toml untouched.
- 2026-07-14: HEADLESS DEVICE FLOW — WORKS (AC5 primary path, no manual fallback needed). Mechanics proven first with curl against live Dex then encoded in the test: POST {issuer}/device/auth/verify_code {user_code} → 302 (followed as GET, browser semantics; curl needed explicit GET, reqwest's default policy already converts) → login form /auth/local/login?back=&state=… → POST {login,password} → 303 chain → 200 at /device/callback?code&state → token poll returns access_token+refresh_token (expires_in 86399; offline_access honored by Dex). Gotcha logged: Dex deviceRequests TTL is 10m — a first probe "Invalid or expired user code" was my own slowness between calls, not a flow defect. Second gotcha: Dex `sub` is base64(userID+connector) (CiQwOGE4… for alice), NOT the raw userID — KAIROS_DEPLOYMENT_ADMINS must carry that form (matches T-0034's evidence).
- 2026-07-14: TESTS. Unit (9, in-crate): store round-trip with 0600/0700 asserts + rewrite keeps 0600, missing=empty + corrupted→exit-2 message, needs_refresh skew boundary, URL normalization/resolution (explicit/single/none→2/ambiguous→1), RFC 8628 poll classification incl. access_denied/expired/slow_down/invalid_grant/garbage, exit-code contract, clap surface. Integration `tests/cli_live.rs` (scratch DB kairos_cli_m4_test, production router in-process on an ephemeral port against live Dex, real `kairos` binary via CARGO_BIN_EXE): login with headless approval (asserts RFC 9728 discovery line, "enter code:", exit 0, cache 0600 keyed by URL with issuer/client_id/tenant/refresh_token/future expiry) → whoami --json (alice/cli_m4/admin/teams[platform]) + human form → forced expires_at=1 → whoami refreshes (new access token, future expiry persisted) → garbage refresh_token → exit 2 with re-login instruction → corrupted cache → exit 2 → logout clears entry (exit 0) → whoami → exit 2 → login vs dead port → exit 1. `cargo test -p kairos-cli`: 10/10 green in ~7s; scratch DB dropped in-test.
- 2026-07-14: LIVE TRANSCRIPTS (scratch DB kairos_t0036_live; `target/debug/kairos-server serve` on 127.0.0.1:18093, OIDC_ISSUER_URL=http://localhost:5558/dex, OIDC_AUDIENCE=kairos-cli, KAIROS_BASE_DOMAIN=kairos.test, KAIROS_DEPLOYMENT_ADMINS=alice's Dex sub; `kairos-server migrate` applied 20260709000000; tenant t0036 provisioned via admin API with alice initial admin per T-0034 recipe). (1) `kairos login --url http://127.0.0.1:18093 --tenant t0036` → "Discovered OIDC issuer: http://localhost:5558/dex … To sign in, open: http://localhost:5558/dex/device?user_code=RSZQ-GZQV … Waiting for approval (polling every 5s; the code expires in 600s)" → headless approval → "Logged in to http://127.0.0.1:18093. Credentials cached in …/credentials.json (mode 0600)." exit 0; `ls -l` shows `-rw-------`. (2) `kairos whoami` → "user: alice <alice@kairos.test> / org: t0036 (role: admin) / teams: (none)" exit 0; `--json` returns the full WhoamiResponse. (3) expires_at forced to 1 → `kairos whoami` exit 0 and cache rewritten with future expiry (1784151906) — refresh-on-expiry live. (4) refresh_token="garbage"+expired → exit 2: "the session … could not be refreshed (the issuer rejected the refresh (400 …)). Run `kairos login --url http://127.0.0.1:18093` to re-authenticate." (5) corrupted cache → exit 2 naming the file + fix. (6) `kairos logout` → "Logged out of http://127.0.0.1:18093." exit 0; whoami after → exit 2 "no cached credentials"; second logout → "No cached credentials; nothing to do." exit 0. (7) `kairos login --url http://127.0.0.1:9` → exit 1 "cannot reach the deployment … Check the URL and your network connection."
- 2026-07-14: GATE: `cargo fmt --check` clean · `cargo clippy --workspace --all-targets -- -D warnings` clean · `cargo test -p kairos-cli` 10/10 green (incl. the live integration flow). Full `angreal test unit|integration` gate deliberately deferred (shared-services mode with a concurrent agent; per-task instruction). Cleanup: server killed, kairos_t0036_live + kairos_cli_m4_test dropped; shared kairos DB, Dex (200), and the concurrent agents' scratch DBs untouched. Declined-grant path covered at unit level (access_denied classification → exit 2) — Dex's skipApprovalScreen leaves no live decline affordance.