---
id: the-oidc-issuer-becomes-optional
level: task
title: "The OIDC issuer becomes optional when local auth is on"
short_code: "KAIROS-T-0208"
created_at: 2026-09-26T14:45:09.254997+00:00
updated_at: 2026-09-26T15:16:45.795174+00:00
parent: KAIROS-I-0018
blocked_by: [KAIROS-T-0203]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0018
---

# The OIDC issuer becomes optional when local auth is on

## Parent Initiative

[[KAIROS-I-0018]]

## Objective

With `KAIROS_LOCAL_AUTH` on, `OIDC_ISSUER_URL` and `OIDC_AUDIENCE` become optional,
so a deployment can run with **no identity provider at all**.

This is [[KAIROS-I-0018]]'s exit criterion *"a ten-person team can run Kairos with no
IdP at all"*, and after [[KAIROS-T-0203]] nothing else delivers it: local login works,
but both OIDC variables are still `required()` in `config.rs`, so the server refuses
to start without an issuer it will never use. [[KAIROS-T-0200]]'s bundled Dex covers
`helm install`; the compose stack bundles none, and telling a ten-person team to run
Dex anyway is the setup burden this initiative exists to remove.

Filed after [[KAIROS-T-0203]] rather than during it: the gap was found while building
the login endpoint, and it is a decomposition gap rather than a defect in that task.
Dylan chose to make the issuer optional (2026-09-26) over keeping it required and
amending the exit criterion.

## Dependencies

[[KAIROS-T-0203]]. It comes BEFORE [[KAIROS-T-0204]], because a first-boot bootstrap
admin is the way into a deployment that has no issuer — building it against a shape
where an issuer is mandatory would be building it twice.

## Implementation Notes

### What "optional" has to mean, precisely

Not "may be empty and everything still runs". Three things must hold:

1. **`local_auth && no issuer` is valid**, and boots.
2. **`!local_auth && no issuer` is still refused, at boot, with the message it has
   today.** This is the case that must not regress: a deployment that intends OIDC and
   typo'd its issuer must still fail loudly rather than come up with no way in.
3. **An issuer that is named is still validated at boot.** Making it optional must not
   make a broken issuer a runtime surprise.

The condition is therefore "local auth is on", not "the variable is empty".

### Startup: do not discover what is not there

`build_state` calls `Authenticator::discover(...)`, which fetches discovery + JWKS and
fails fast. With no issuer there is nothing to fetch.

The change wants to avoid `Option<Arc<Authenticator>>` in `AppState`: that field is
threaded through `state_with`, which about thirty integration tests call. An
`Authenticator` that knows it has no issuer and refuses every token — a
`disabled()`-style constructor — keeps the field shape, keeps the tests untouched, and
puts the refusal where the OIDC branch already looks for it. Its 401 should say *no
OIDC issuer is configured*, which is a genuinely different fact from *this token is
bad* and the one case where the two SHOULD differ: it is a deployment
misconfiguration, not a credential, and nothing about it is guessable.

### The GUI and `/api/config`

`/api/config` tells the SPA how to start a login. With no issuer there is no
authorization endpoint to redirect to, so it must say so and the GUI must not render
an SSO button that cannot work. [[KAIROS-T-0205]] builds the password form; this task
owns the *signal* — the config payload — so that T-0205 has something to branch on.

The CLI has the same problem: `kairos login` performs an OIDC flow. It should say
plainly that this deployment has no issuer and point at the session path rather than
failing in the middle of a browser redirect.

### The chart, and a contradiction to catch

`kairos.dexEnabled` is currently "on unless you name an issuer". That was right when an
issuer was mandatory; it is not right now, because "no issuer and local auth" is a
legitimate combination that would silently get a Dex it did not ask for. The tri-state
needs a third input, and `helm template` needs to refuse the genuinely contradictory
combinations rather than picking one.

Also: the chart's `required` on `config.oidc.issuerUrl` must relax by the same rule as
the server's, and for the same reason.

## Acceptance Criteria

## Acceptance Criteria

- [x] `KAIROS_LOCAL_AUTH=true` with no `OIDC_ISSUER_URL` boots, and `/api/login` works
- [x] `KAIROS_LOCAL_AUTH=false` with no issuer is still refused at boot, with the
      message it gives today — asserted, not assumed
- [x] A named issuer is still discovered and validated at boot
- [x] With no issuer, an OIDC bearer gets a 401 that names the misconfiguration; the
      session and API-key paths are unaffected
- [x] `/api/config` tells the SPA there is no issuer, and the GUI offers no SSO button
      it cannot honour
- [x] `kairos login` says plainly that the deployment has no issuer
- [x] The chart no longer bundles a Dex nobody asked for, and refuses contradictory
      combinations at render time
- [x] `angreal test lint`, `unit`, `integration` green

## Status Updates

### 2026-09-26 — done

`OIDC_ISSUER_URL` and `OIDC_AUDIENCE` are now `Option<String>`, required unless
`KAIROS_LOCAL_AUTH` is on. A ten-person team can run Kairos with no identity provider,
no ingress, no DNS and no TLS.

#### Three properties, not one

"Optional" was made to mean exactly three things, each with a test:

1. `local_auth` with no issuer boots and `/api/login` works.
2. **No local auth and no issuer is still refused at boot**, with the message it always
   gave plus a pointer to the new escape hatch. This is the case that must not regress:
   a deployment that means to use OIDC and mistyped its issuer has to fail here, because
   coming up with no way for anyone to log in is a far worse failure than refusing to
   start.
3. A named issuer is still discovered and validated at startup, so making it optional did
   not turn a broken issuer into a runtime surprise.

Also added: **half an issuer is refused.** Setting one of the pair without the other is
nobody's intention, and its symptom — every token rejected — points at the token rather
than at the configuration.

#### `Authenticator::disabled()` rather than `Option<Arc<Authenticator>>`

`AppState.auth` is threaded through `app::state_with`, which around thirty integration
tests call. A constructor whose `verify` returns `VerifyError::NoIssuer` keeps the field
shape, leaves every test untouched, and puts the refusal where the OIDC branch already
looks for it instead of spreading `if let Some` across call sites. The `enabled` flag is
explicit rather than inferred from an empty `issuer`, because "the issuer is an empty
string" and "there is no issuer" must not be the same condition.

`NoIssuer`'s message deliberately **names the cause** — the one exception to the uniform
401 rule established in [[KAIROS-T-0203]]. There is no secret here: no amount of guessing
turns "this server has no issuer" into access, and a caller told only "invalid token"
would search their own token for a fault that is not there.

#### The signal for the GUI

`/api/config` now returns `issuer: null`, `authorization_endpoint: null` and a new
`local_auth` boolean, and with no issuer it does **not** attempt discovery. Returning 502
`IDP_UNREACHABLE` there would have been a lie — nothing is unreachable, there is simply
no IdP — and it would have stopped the SPA rendering the login page it *can* offer.

`kairos-web`'s mirror takes `Option` fields with `#[serde(default)]` plus a `can_sso()`
helper, so a payload from an older server still parses. `begin_login` refuses **before**
touching `sessionStorage` or the location bar: a half-started flow leaves a stale
verifier behind and the person on a blank page, which is a worse way to learn this than a
sentence. [[KAIROS-T-0205]] renders the form; this task owns the signal it branches on.

The MCP protected-resource metadata lists an **empty** `authorization_servers` rather
than `[null]` — a client reads it to find out where to get a token, and would otherwise
try to fetch a discovery document from the string "null".

#### The chart

`kairos.dexEnabled`'s tri-state gained a clause: unset now means on unless an issuer is
named **or local auth is on**. Before this, "no issuer" meant "the operator forgot, give
them a Dex", because a Kairos with no issuer could not start. Now it can be deliberate,
and an operator who asks for password accounts must not silently receive an identity
provider complete with a static password in their values file and their release history.

A new render-time guard refuses "no Dex, no issuer, no local auth" — the server refuses
to start in that state, so without it the operator would learn from a CrashLoopBackOff
instead of from `helm install`. `ci/local-auth-values.yaml` covers the new shape and
needs no ingress, which is the visible contrast with `bundled-dex-values.yaml`.

#### A defect found in the chart's own gate

`helm lint` **does not fail on a template `fail`.** It reports it and exits 0. The release
workflow's lint step carried a comment claiming "lint is a real gate here: it fails on
template errors", and it passed on default values the chart is supposed to refuse. Worse,
nothing in the repository rendered the seven `ci/*-values.yaml` sets at all — they were
decorative.

Added **Gate 7** to `ci.yml`: `helm template` over every values set, plus three
assertions that the guards actually refuse what they claim to (no login path, two
issuers, both tenancy modes). Both halves matter — the values sets prove each supported
shape renders, and the refusals prove the guards still fire, because a guard that has
stopped firing looks exactly like one that was never needed. Verified locally: seven
renders pass, three refusals refuse.

#### Follow-on, not done here

**The CLI cannot authenticate against a no-issuer deployment at all.** `kairos login` now
says so clearly and points at a service-account API key, which works — but there is no
way to hand the CLI a session bearer. That belongs with [[KAIROS-T-0204]]'s break-glass
CLI work or its own task; it is outside this task's criteria and is recorded rather than
quietly left.

**Files:** `config.rs` (conditional requirement, the pair check, 5 new tests),
`middleware/auth.rs` (`disabled()`, `enabled`, `VerifyError::NoIssuer`), `app.rs`
(discover only what is there), `web.rs` (`SpaConfig`, no discovery without an issuer),
`mcp/mod.rs` (empty list), `kairos-web/src/auth.rs` (mirror + `can_sso` + 3 tests),
`kairos-cli/src/oidc.rs` (an honest message), `tests/local_login.rs` (a fifth test),
`_helpers.tpl`, `configmap.yaml`, `ci/local-auth-values.yaml`, `ci.yml` Gate 7, and the
two reference docs.

`angreal test lint`, `unit`, `integration`, `web lint` and `web build --release` are all
green.