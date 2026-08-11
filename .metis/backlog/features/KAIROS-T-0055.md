---
id: multi-audience-token-validation
level: task
title: "Multi-audience token validation (accept an OIDC_AUDIENCE list)"
short_code: "KAIROS-T-0055"
created_at: 2026-07-17T02:15:25.071172+00:00
updated_at: 2026-08-10T22:32:45.279305+00:00
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

# Multi-audience token validation (accept an OIDC_AUDIENCE list)

## Objective **[REQUIRED]**

Let the auth middleware accept a **set** of allowed audiences rather than a single
`OIDC_AUDIENCE`. Required for IdPs that mint a distinct `aud` per OAuth client and
cannot add a shared deployment-wide audience — most notably **Google / Google
Workspace**, where the GUI, CLI, and service-account clients each have their own
`client_id` = `aud`. Keycloak solves this today with a client-scope audience mapper
(one shared audience across first-party clients); Google has no easy equivalent.

## Backlog Item Details **[CONDITIONAL: Backlog Item]**

### Type
- [x] Feature - New functionality or enhancement

### Priority
- [x] P2 - Medium (nice to have) — only needed for multi-client deployments on
      per-client-audience IdPs; GUI-only deployments are unaffected.

### Business Justification **[CONDITIONAL: Feature]**
- **User Value**: On Google Workspace, run GUI + CLI + service accounts together
  without each needing a separate Kairos deployment.
- **Business Value**: Completes the "Google Workspace as IdP" story alongside
  [[KAIROS-T-0054]].
- **Effort Estimate**: S (parse a comma-separated list, pass to
  `jsonwebtoken::Validation::set_audience` which already accepts multiple).

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [x] `OIDC_AUDIENCE` accepts a comma-separated list (single value still works,
      backward compatible). Parsed into the `Vec`/slice handed to
      `Validation::set_audience` in `crates/kairos-server/src/middleware/auth.rs`.
- [x] A token whose `aud` matches **any** configured audience validates; a token
      whose `aud` matches **none** is rejected with the existing invalid-token error.
- [x] Env/config docs updated (deploy `.env.example`, Helm `values.yaml`
      `config.oidc.audience`, README) to document the list form and the Google
      multi-client rationale.
- [x] Tests: multi-audience accept (each listed `aud`), reject (unlisted `aud`),
      and single-value backward-compat.
- [x] Helm chart: `config.oidc.audience` renders a list value into the env/ConfigMap
      correctly (no change to the single-value default behavior) — `helm template`
      verified both forms.

**Completed 2026-08-10**: full gate chain green (unit incl. the two new auth
tests, integration, e2e — one pre-existing move-menu flake absorbed by retry).

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
- `jsonwebtoken`'s `Validation::set_audience(&[...])` already supports multiple
  audiences — the change is config plumbing + parsing, not crypto.
- The middleware doc comment already notes "Dex sets `aud` to the requesting OAuth
  client's [id]" and "Keycloak adds a deployment-wide audience via a client-scope
  mapper" — update it to describe the list form for per-client-audience IdPs.

### Dependencies
- Pairs with [[KAIROS-T-0054]]; that task unblocks GUI-only Google SSO, this one
  unblocks multi-client (GUI + CLI + service accounts) Google deployments.

### Risk Considerations
- Keep it a strict allow-list; never fall back to "any audience." An empty/unset
  audience must remain a hard configuration error.

## Status Updates **[REQUIRED]**

- 2026-08-10: Implemented. `middleware/auth.rs`: `Authenticator.audience: String` → `audiences: Vec<String>` with `parse_audiences()` (split on comma, trim, drop blanks — single value = one-element list, fully backward compatible); both constructors parse; `verify()` hands the list to `Validation::set_audience`. Strict allow-list enforced: `discover()` fails at startup if the parsed list is empty (no "any audience" mode; `with_static_keys` stays permissive, test-only). Module doc + config.rs field doc updated with the Google per-client-audience rationale. Unit tests: `any_listed_audience_validates_and_unlisted_is_rejected` (each of three listed auds accepted, unlisted rejected with InvalidAudience) + `audience_parsing_handles_lists_and_blanks` (single, list-with-blanks, effectively-empty); single-value compat covered by the whole existing fixture suite. Docs: deploy/.env.example (list form + Google example), helm values.yaml (list-or-string doc + example), helm README table row, top-level README multi-client note rewritten from "tracked under T-0055" to the working instructions. Helm configmap renders BOTH forms via `kindIs "slice"` → `join ","` — verified with `helm template` (string → "single"; list → "gui-id,cli-id"). Full `angreal test all` + e2e running.