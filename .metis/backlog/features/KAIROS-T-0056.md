---
id: web-client-secret-for-confidential
level: task
title: "Web client secret for confidential OIDC clients (Google Workspace)"
short_code: "KAIROS-T-0056"
created_at: 2026-07-17T03:31:13.855202+00:00
updated_at: 2026-07-17T12:10:26.882698+00:00
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

# Web client secret for confidential OIDC clients (Google Workspace)

## Objective **[REQUIRED]**

Let the GUI's server-side token relay (`POST /api/auth/token`, `web.rs`) present
an optional OAuth **`client_secret`** on the code/refresh exchange. Required for
**confidential** web clients — most importantly **Google / Google Workspace**,
which has no public-SPA client type: a hosted web app with an `https://…/callback`
redirect must be a "Web application" OAuth client, and Google's token endpoint
rejects the exchange (`invalid_client`) without the secret, even with PKCE.

This is the second half of "point Kairos at Google Workspace" — the companion to
[[KAIROS-T-0054]] (id_token bearer). The secret lives ONLY server-side in the
relay (never shipped to the browser), which is exactly where it belongs: the
relay already performs the exchange server-side.

## Backlog Item Details **[CONDITIONAL: Backlog Item]**

### Type
- [x] Feature - New functionality or enhancement

### Priority
- [x] P2 - Medium — unblocks the GUI half of Google Workspace SSO. Without it the
      browser login dead-ends at the token exchange.

### Business Justification **[CONDITIONAL: Feature]**
- **User Value**: Completes browser SSO against Google Workspace (T-0054 alone is
  necessary but not sufficient).
- **Effort Estimate**: S (relay adds one form field when configured; config +
  Helm secret plumbing + tests + docs).

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [ ] New optional `KAIROS_WEB_CLIENT_SECRET` config (unset by default →
      current public-client behavior; Dex/Keycloak public clients unaffected).
- [ ] When set, the relay includes `client_secret` on BOTH grants
      (`authorization_code`, `refresh_token`) sent to the issuer token endpoint.
      When unset, the relay body is byte-for-byte what it sends today.
- [ ] The secret is server-side only — never in `/api/config`, never sent to the
      browser (verify it is not added to `SpaConfig`).
- [ ] Helm: `config.webClientSecret` (inline) OR `webClientSecretExistingSecret`
      renders into the app Secret (NOT the ConfigMap) and reaches the container
      via envFrom. Absent by default.
- [ ] Docs: README Google Workspace runbook updated with the client secret step;
      env surface in `deploy/.env.example` + Helm `values.yaml`/`README.md`.
- [ ] Tests: config parsing (present/absent); a relay unit test that the
      `client_secret` form field is present iff configured, for both grant types.

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
- `config.rs`: `web_client_secret: Option<String>` from env (empty = None).
- `web.rs`: `WebAuth.client_secret: Option<String>`; in `token_relay`, push
  `("client_secret", s)` into `params` when `Some`. Applies to both match arms
  (params vec is shared before the grant-specific branch).
- Do NOT add it to `SpaConfig`/`spa_config` — the browser never sees it.
- Helm: extend `secret.yaml` (or a dedicated secret) with `KAIROS_WEB_CLIENT_SECRET`
  under `stringData` when `config.webClientSecret` set; support an existing-secret
  reference for production. Deployment already `envFrom`s the app secret.

### Dependencies
- Completes the Google Workspace path started by [[KAIROS-T-0054]].

### Risk Considerations
- Secret hygiene: keep it out of values/release history in production (existing
  Secret ref); never log it; never echo it in `/api/config` or errors.

## Status Updates **[REQUIRED]**

### 2026-07-17 — Implemented + full gate GREEN; complete

All 6 ACs met:
- **[x]** `KAIROS_WEB_CLIENT_SECRET` config (`Option<String>`, unset by default)
  in `config.rs`; tests: absent by default, present when set.
- **[x]** Relay injects `client_secret` on both grants iff configured —
  extracted a pure `relay_params()` in `web.rs`; 3 unit tests (present-iff-set
  for auth_code, applies to refresh, unknown grant still rejected).
- **[x]** Server-side only — not added to `SpaConfig`/`/api/config` (verified);
  module doc corrected.
- **[x]** Helm: `config.webClientSecret` (inline → chart-rendered
  `-webauth` Secret) OR `webClientSecretExistingSecret` (+Key); wired via
  `envFrom`-adjacent `env` secretKeyRef in the Deployment; new helpers. Rendered
  all 3 modes: none → 0 artifacts, inline → Secret+ref, existing → ref only (no
  rendered Secret).
- **[x]** Docs: README Google Workspace runbook (two-setting explanation +
  secret step), `deploy/.env.example`, Helm `values.yaml`/`README.md`.
- **[x]** Tests: config + relay unit tests above.

**Gate (all green):** fmt --check clean; clippy (server all-targets) clean;
web-lint clean; unit all crates 0 failures; **integration all 27 targets pass**
(incl. `kairos-server::web` relay + `openapi` live), clean teardown; helm lint +
3-mode render OK.

Together with [[KAIROS-T-0054]] this completes GUI browser SSO against Google
Workspace. (CLI device-flow against Google is a separate client type; the GUI is
the primary path and is now complete end-to-end.)