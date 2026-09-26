---
id: rate-limiting-because-a-password
level: task
title: "Rate limiting, because a password endpoint without it is a brute-force target"
short_code: "KAIROS-T-0202"
created_at: 2026-09-26T12:43:41.232716+00:00
updated_at: 2026-09-26T13:54:52.757213+00:00
parent: KAIROS-I-0018
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0018
---

## Parent Initiative

[[KAIROS-I-0018]]

## Objective

Give Kairos rate limiting, and apply it to authentication. **Kairos has none today**
— not on login, not anywhere — and a password endpoint without it is a brute-force
target.

Split out of the login task deliberately: this is new infrastructure, not part of a
handler, and it is the largest piece of genuinely novel work in
[[KAIROS-I-0018]].

## Implementation Notes

### Scope it to what is needed

A general-purpose rate limiter for every endpoint is a bigger product decision than
this initiative needs. What is needed is: **failed authentication attempts, per
identity and per source, with a lockout that decays.** Build that, and leave a note
saying the mechanism could be generalised later rather than pretending it already is.

### Where the state lives

This is the interesting decision and it wants recording.

- **In-process** (a `Mutex<HashMap>` or a small LRU in `AppState`) is simple, costs
  no round trip, and is **per-replica** — so a deployment behind an HPA multiplies
  the allowance by the replica count, and a restart forgets every lockout.
- **In PostgreSQL** is shared and survives restarts, and puts a write on the failure
  path of an endpoint that is being attacked, which is the moment you least want
  extra writes.

Given [[KAIROS-A-0013]]'s one-binary shape and that the target here is small
deployments, in-process is probably right — but say so explicitly, name the
replica-count caveat in the documentation, and make the limiter a trait or a small
module boundary so a shared implementation can replace it without touching handlers.

### Behaviour worth getting right

- **Per-identity and per-IP.** Per-identity alone lets an attacker spray many
  accounts; per-IP alone lets one account be locked out by a stranger and is wrong
  behind a proxy where every request shares a source.
- **A lockout must decay**, or a user's mistyped password is a support ticket and an
  attacker can deliberately lock a known account out.
- **Do not leak.** A rate-limited response must not reveal whether the account
  exists, and should look like the ordinary failure — `429` is honest about *why*
  but must not vary by whether the email was real.
- `X-Forwarded-For` is only trustworthy behind a proxy you control. The reference
  deployment has Caddy in front; a direct deployment does not. Decide what is trusted
  and write it down.

### Observability

There is now a metrics registry and tracing. A lockout is exactly the thing an
operator wants to see, so a counter and a log line are part of this, not a follow-up.

## Acceptance Criteria

## Acceptance Criteria

- [x] Repeated failed authentications from one source, or against one identity, are
      throttled; the threshold and window are configurable with sensible defaults
- [x] The lockout decays; a legitimate user who mistypes is not locked out for long
- [x] A throttled response reveals nothing about whether the account exists
- [x] Where the state lives is a recorded decision, with the replica-count caveat
      documented if it is in-process
- [x] What is trusted for the client address is documented
- [x] A metric, and a log line, for a lockout
- [x] Unit tests for the decay arithmetic and the identity/source split; integration
      coverage that a burst is actually refused
- [x] `angreal test lint`, `unit` and `integration` green

## Status Updates

### 2026-09-26 — done

`crates/kairos-server/src/rate_limit.rs` is the whole mechanism: `AuthThrottle`
(`Mutex<HashMap<Subject, Bucket>>`), and an `Attempt` wrapper that call sites use
instead of touching the throttle directly.

**Time is a parameter.** Every method takes `now: Instant`. Decay is arithmetic on
instants, so all eight unit tests assert expiry and decay without sleeping — a
throttle whose tests have to wait is a slow test that eventually goes flaky.

**Where the state lives: in process.** Recorded in the module docs with the
consequences stated, not softened — lockouts are per replica, so three pods allow
roughly three times the failures, and a restart forgets everything. The alternative
puts a write on the failure path of an endpoint under attack. `Attempt` is the only
type call sites touch, so a shared implementation can replace the internals without
changing a handler.

**What is trusted for the client address.** `X-Forwarded-For` is read only when
`KAIROS_TRUSTED_PROXY` is on, and it is off by default: the header is
caller-supplied, so trusting it on a directly exposed server does not weaken a
source-based throttle, it deletes it. When trusted, the **last** element is used,
not the first — a proxy appends the peer it saw, so the first element is the one
under the caller's control. With the switch on and no header, the answer is `None`
rather than the socket peer, because behind a proxy the peer IS the proxy and
counting it pools every client into one bucket.

The compose stack hard-codes it on, like `KAIROS_BIND_ADDR`, because it is a fact
about that topology rather than a choice — the Kairos service publishes no port, so
Caddy is the only way in. The chart follows `ingress.enabled` via a tri-state
`kairos.trustedProxy` helper, matching `kairos.dexEnabled`.

**The identity/source split, and one deliberate asymmetry.** Both grains exist and
are unit-tested. The API-key path counts by **source only**, never by the key's
tenant slug: the slug is not a secret and every legitimate client of an org shares
it, so an identity bucket there would let one CI job with a stale key lock a whole
organization out of its own API. The identity grain belongs to
[[KAIROS-T-0203]]'s password endpoint, where the account being guessed belongs to
one person.

**No existence leak, structurally.** The throttle is consulted *before* the
credential is checked, so the 429 cannot depend on whether the account exists — and
it also means a locked-out attacker never gets the argon2 work done on their behalf.
The integration test asserts the refusal for an unknown tenant is byte-identical to
the one for a real tenant, and that the *valid* key is refused too.

**Observability.** `kairos_auth_lockouts_total{subject="identity"|"source"}` on
`/metrics`, plus a WARN log. The subject *kind* is recorded and the value is not: an
email address or an IP in a log line outlives the incident it was gathered for.

#### Two defects found by building it

1. **`lockout` inside `window` made one eviction test's arithmetic impossible.** My
   first version asserted a subject was still locked out past the window with a
   30-second lockout — it had already expired, so the code was right and the test
   was wrong. Only a lockout that *outlasts* the window exercises the `||` in
   `evict_expired`, so that test now configures one.
2. **`{{ with }}` would have silently swallowed the disable switch.** Helm treats
   `0` as unset, and `KAIROS_AUTH_MAX_FAILURES=0` is exactly how an operator turns
   throttling off. A `{{ with .Values.config.auth.maxFailures }}` renders nothing for
   `0`, so the operator does the documented thing and the throttle keeps running —
   the worst way for an escape hatch to fail. Fixed with a `kairos.setValue` helper
   and verified by rendering `0` and getting `"0"`.

Eviction is opportunistic (above 1024 tracked subjects, on a recorded failure)
rather than on a timer: the map is keyed by attacker-supplied identities, so
unbounded growth is a memory-exhaustion vector, but a background sweeper is a task
to own and keep alive in tests for no gain.

**Files:** `rate_limit.rs` (new, 8 unit tests), `tests/rate_limit.rs` (new, 2
integration tests), `config.rs` (four variables + `parse_num`), `error.rs`
(`too_many_requests`), `metrics.rs` (the counter), `app.rs` (`AppState.throttle`,
and `into_make_service_with_connect_info` so the socket peer is available at all),
`middleware/auth.rs` (the API-key path), plus compose, `.env.example`, the chart
(values, configmap, two helpers) and `docs/src/reference/configuration.md`.

`angreal test lint`, `unit` (all suites) and `integration` are green.