---
id: compose-silently-drops-the-two
level: task
title: "Compose silently drops the two variables Google Workspace deployments require"
short_code: "KAIROS-T-0177"
created_at: 2026-09-23T22:58:46.191647+00:00
updated_at: 2026-09-23T22:58:46.191647+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/backlog"
  - "#bug"


exit_criteria_met: false
initiative_id: NULL
---

# Compose silently drops the two variables Google Workspace deployments require

## Objective

Make `deploy/docker-compose.yaml` forward the variables `deploy/.env.example`
tells an operator to set. Two of them never reach the container, and they are
exactly the two a Google Workspace deployment cannot work without.

## Backlog Item Details

### Type
- [x] Bug - Production issue that needs fixing

### Priority
- [x] P1 - High (important for user experience)

### Impact Assessment

- **Affected users**: anyone deploying the reference Compose stack against an
  IdP with opaque access tokens — Google and Google Workspace by name. They
  follow `.env.example`, set both values, and neither is read.
- **Reproduction**: `deploy/.env.example` documents
  `KAIROS_API_BEARER` (line 30) and `KAIROS_WEB_CLIENT_SECRET` (line 35),
  each with a paragraph of explanation. `deploy/docker-compose.yaml`'s
  `environment:` block (lines 46–59) forwards neither. The server reads both:
  `crates/kairos-server/src/config.rs:194` for `KAIROS_API_BEARER`, and the
  field documented at `config.rs:125` for the client secret.
- **Expected vs actual**: the operator's setting takes effect. Instead the
  server falls back to its defaults — `ApiBearer::AccessToken` and no client
  secret — with no warning, because both variables are legitimately optional
  and absence is indistinguishable from "not set on purpose".

### Why this is P1 rather than cosmetic

`.env.example`'s own text makes these two mandatory for a named IdP:

> `id_token` — issuers with OPAQUE access tokens: Google / Google Workspace
> … Google deployments **MUST** set id_token.

> OAuth client secret for a CONFIDENTIAL GUI client. … **REQUIRED** for Google
> Workspace, whose "Web application" OAuth clients reject the token exchange
> without it even under PKCE.

So the documented configuration for Google Workspace is silently ignored in
the reference deployment. The failure surfaces as a login that does not work —
an opaque `ya29.…` access token failing JWT validation, or a token exchange
rejected for a missing secret — neither of which points anywhere near a
missing line in a compose file. An operator would reasonably conclude their
IdP configuration is wrong.

## Acceptance Criteria

- [ ] `deploy/docker-compose.yaml` forwards `KAIROS_API_BEARER` and
      `KAIROS_WEB_CLIENT_SECRET`, with the same `${VAR:-}` default style the
      other optional variables use.
- [ ] Every variable in `deploy/.env.example` is either forwarded by the
      compose file or explicitly marked in the example as not applying to the
      Compose deployment. A test or a check, not an eyeball — this class of
      drift is invisible by construction.
- [ ] The same audit run against `deploy/helm/kairos/templates/configmap.yaml`,
      which has the inverse problem: it emits `KAIROS_OTEL_ENDPOINT`, and no
      crate reads it (the binary has no OpenTelemetry dependency). Either wire
      it or drop it; a chart value that goes nowhere is the same defect
      pointing the other way.

## Implementation Notes

The general shape is worth noting: **three places must agree** — what the
binary reads (`config.rs`), what the reference deployments forward
(`docker-compose.yaml`, `configmap.yaml`), and what the operator is told to
set (`.env.example`, `values.yaml`). Nothing currently checks that they do,
and all three drifted independently. A test that diffs the sets is the fix
that stops it recurring; forwarding two lines is only the symptom.

`reference/configuration.md` in the documentation book
([[KAIROS-T-0168]]) now documents all three states truthfully, including the
inert ones, so the reference is correct even while the code is not.

## Status Updates

**2026-09-23 — filed.** Found by [[KAIROS-T-0168]] while writing
`reference/configuration.md` for [[KAIROS-I-0016]]: documenting every
configuration value required reading what actually consumes each one, which is
how three separate "this variable goes nowhere" cases surfaced. Verified
independently before filing — the server does read both variables, and the
compose environment block does not contain either.
