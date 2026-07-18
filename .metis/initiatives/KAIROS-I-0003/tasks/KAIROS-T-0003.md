---
id: m0-compose-dev-stack-postgres-16
level: task
title: "M0: Compose dev stack - Postgres 16 + Dex"
short_code: "KAIROS-T-0003"
created_at: 2026-07-08T15:05:58.241063+00:00
updated_at: 2026-07-09T03:20:28.411570+00:00
parent: KAIROS-I-0003
blocked_by: [KAIROS-T-0001]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0003
---

# M0: Compose dev stack - Postgres 16 + Dex

## Parent Initiative

[[KAIROS-I-0003]]

## Objective

Grow `.angreal/docker-compose.yaml` into the dev/test stack per KAIROS-A-0013 dev profile: Postgres 16 + Dex (static-config OIDC issuer per KAIROS-A-0010).

## Acceptance Criteria

## Acceptance Criteria

- [x] Compose runs `postgres:16` (named volume, healthcheck) and Dex with a committed static config: ≥3 test users, clients `kairos-web` (PKCE), `kairos-cli` (device grant enabled), `kairos-svc` (client credentials), long-lived test token settings
- [x] `angreal services up|down|reset|clean` manage the stack end to end
- [x] Dex discovery document reachable (`curl http://localhost:<port>/.well-known/openid-configuration`) and a token obtainable for `kairos-svc` (command documented in the task's Status Updates)

## Implementation Notes

References KAIROS-A-0010 (Dex decision), A-0012 (tests consume this stack), A-0013 (dev profile). Keycloak is NOT part of the dev profile — it arrives with the reference deployment work in M2/M5.

## Verification Gate (KAIROS-A-0012)

`cargo fmt --check` + `cargo clippy -- -D warnings` clean · `angreal test unit` + `angreal test integration` green · every acceptance criterion demonstrated with command + recorded output in Status Updates · new behavior ships with new tests.

## Status Updates

- 2026-07-08: Created at decompose (todo).
- 2026-07-08: **Working constraint (Dylan)**: NO Homebrew on this machine — never `brew install`. Postgres (and all dev tooling) runs container-only. Bcrypt hashes for Dex staticPasswords: use `docker run --rm httpd:2.4 htpasswd -bnBC 10 "" <password> | tr -d ':\n'` (this run used macOS's built-in `/usr/sbin/htpasswd`, no installs; the container command is documented in `.angreal/dex/config.yaml` for portability).
- 2026-07-08: Implemented. `.angreal/docker-compose.yaml` now runs `postgres:16` (named volume `kairos_postgres_data`, `pg_isready` healthcheck — service/container names unchanged so `task_db.py` keeps working) + `dexidp/dex:v2.43.1` with committed static config `.angreal/dex/config.yaml` mounted read-only. Keycloak removed from dev stack per A-0010/A-0013 (returns as production reference in M2/M5). No changes needed to `task_services.py`/`utils.py` — `docker compose up -d --wait` honors the new healthchecks and return codes already propagate.
- 2026-07-08: **Port note**: Dex listens on **5558**, not the customary 5556 — both 5556 and 5557 are occupied by other local Dex instances (cloacina-demo, weir) on this machine. Issuer is `http://localhost:5558/dex`; port is consistent across issuer, listener, compose map, and healthcheck. Anything configuring `OIDC_ISSUER_URL` against the dev stack must use 5558.
- 2026-07-08: **Grant-type caveat (kairos-svc)**: Dex does NOT implement the `client_credentials` grant (it is an identity broker, not a full AS — Keycloak difference). Closest working grant is the resource-owner **password grant** via `oauth2.passwordConnector: local`, using the confidential `kairos-svc` client + dedicated service user `svc@kairos.test` (maps to A-0010's "service account → user row via sub"). `kairos-svc` remains a confidential client with a secret, so the client carries over to Keycloak's true client_credentials in M2/M5.
- 2026-07-08: Verification evidence (all commands run for real on this machine):
  - `angreal services up` → exit 0; `docker compose ps` → `kairos-dex Up (healthy)`, `kairos-postgres Up (healthy)`.
  - Discovery: `curl -s http://localhost:5558/dex/.well-known/openid-configuration` → issuer `http://localhost:5558/dex`, `device_authorization_endpoint` present, `grant_types_supported: [authorization_code, password, refresh_token, urn:ietf:params:oauth:grant-type:device_code]`, `code_challenge_methods_supported: [S256, plain]`.
  - Token for kairos-svc: `curl -s http://localhost:5558/dex/token -u kairos-svc:kairos-svc-secret -d grant_type=password -d username=svc@kairos.test -d password=svc-password -d scope="openid profile email offline_access"` → 200 with `access_token` (RS256 JWT, `aud: kairos-svc`, `email: svc@kairos.test`), `id_token`, `refresh_token`, `expires_in: 86399` (~24h per A-0012 long TTLs).
  - Device flow (kairos-cli): `curl -s http://localhost:5558/dex/device/code -d client_id=kairos-cli -d scope="openid profile email offline_access"` → `device_code` + `user_code` (e.g. `LZGD-SGLM`) + `verification_uri_complete`; completing it needs a browser login, so end-to-end token issuance is proven via the grant above.
  - PKCE (kairos-web): authorize request with `code_challenge`/`S256` → HTTP 302 to `/dex/auth/local` (login page), no client error.
  - User hashes verified: password grant with `alice@kairos.test / alice-password` → 200 + access_token. Test creds (dev-only): alice/bob/carol/svc `@kairos.test`, password `<user>-password`, documented in `.angreal/dex/config.yaml`.
  - Lifecycle: `angreal services down` → exit 0, volume persists; `angreal services reset` → exit 0, both containers healthy again; `angreal services clean` → exit 0, containers gone and `kairos_postgres_data` volume removed. Stack left down/clean.
- 2026-07-08: Verification-gate note: change is infra-only (compose + Dex config + task doc); no Rust surface touched and Cargo/crates are owned by a concurrent agent per orchestrator instruction, so `cargo fmt/clippy` and `angreal test` runs are deferred to the orchestrator's gate for the Rust tasks.