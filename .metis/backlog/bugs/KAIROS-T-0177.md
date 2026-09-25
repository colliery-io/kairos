---
id: compose-silently-drops-the-two
level: task
title: "Compose silently drops the two variables Google Workspace deployments require"
short_code: "KAIROS-T-0177"
created_at: 2026-09-23T22:58:46.191647+00:00
updated_at: 2026-09-25T00:14:25.711623+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#bug"
  - "#phase/completed"


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

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] `deploy/docker-compose.yaml` forwards `KAIROS_API_BEARER` and
      `KAIROS_WEB_CLIENT_SECRET`, with the same `${VAR:-}` default style the
      other optional variables use.
- [x] Every variable in `deploy/.env.example` is either forwarded by the
      compose file or explicitly marked in the example as not applying to the
      Compose deployment. A test or a check, not an eyeball — this class of
      drift is invisible by construction.
- [x] The same audit run against `deploy/helm/kairos/templates/configmap.yaml`,
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
### 2026-09-25 â fixed, and the audit found a third variable

Two lines were the symptom. The fix is the test, because this class of drift is
invisible by construction: every variable involved is optional, so "dropped on
the floor" and "not set on purpose" produce identical behaviour.

**The two dropped variables.** `docker-compose.yaml` now forwards
`KAIROS_API_BEARER` and `KAIROS_WEB_CLIENT_SECRET` in the `${VAR:-}` style the
other optional variables use, with a comment saying why they are mandatory for
Google Workspace specifically.

**The third variable, found by doing the audit the ticket asked for.**
`KAIROS_OTEL_ENDPOINT` was the same defect pointing the other way: the chart
offered `config.otelEndpoint`, emitted the variable, and documented it in
`values.yaml`, the chart README, a CI values file and the book â and no crate has
ever read it. **Dropped rather than wired**, because wiring OpenTelemetry is a
feature with a dependency tree and a sampling decision, and a dead switch on the
panel is worse than no switch. Filed as [[KAIROS-T-0196]] so the intent behind it
is on the record rather than deleted.

The book had documented it honestly all along â a "Recognised by the Helm chart
but not by the server" section, which is exactly how [[KAIROS-T-0168]] found this
in the first place. That section is now empty and gone.

### The test, and why it is recorded rather than grepped

Three unit tests in `crates/kairos-server/src/config.rs` â unit, not integration,
because they read files and need no services, so CI gate 3 catches this before
Docker is even started.

The set of variables the binary reads is **recorded, not parsed**. All three
config types take a lookup closure (`AppConfig::from_lookup`,
`EmbedConfig::from_vars`, `RetentionConfig::from_lookup`), so a closure that
remembers what it was asked for is authoritative and cannot drift from the code
the way a regex over `config.rs` would. Two wrinkles that would have made the
union short:

- `EmbedConfig` branches on the provider, and the `remote` and `local` arms read
  disjoint variables â so all four provider values are driven and unioned.
- `RetentionConfig` only reads a variable when it is set, so the closure answers
  every lookup.

The three tests:

| test | catches |
|---|---|
| `deployments_set_only_variables_the_binary_reads` | the OTEL direction â a deployment offering a switch wired to nothing |
| `compose_forwards_every_variable_the_env_example_documents` | this ticket's direction â documented, never forwarded |
| `the_variables_google_workspace_requires_reach_the_container` | named separately, so refactoring the generic checks cannot lose the two that actually broke a deployment |

`NOT_THE_BINARYS` carries the legitimate exceptions with a reason each:
`KAIROS_SITE_ADDRESS` (Caddy templates from it), `KAIROS_VERSION` (selects the
image tag; the container never sees it), `POSTGRES_PASSWORD` (composed into
`DATABASE_URL`), and `POSTGRES_DB`/`POSTGRES_USER` â which the test itself found
on first run, and which are the Postgres image's interface rather than ours.

### Verified, not assumed

Reverting the two compose lines fails both set-diff tests, naming
`["KAIROS_API_BEARER", "KAIROS_WEB_CLIENT_SECRET"]` exactly. A test for this
defect that has never been seen to fail would be worth very little.

Gates: `angreal test lint` clean, **390 unit tests** (up from 387), `helm lint`
clean with all five CI value sets rendering, `angreal docs build` green.