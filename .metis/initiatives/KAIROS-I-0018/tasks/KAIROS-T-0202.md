---
id: rate-limiting-because-a-password
level: task
title: "Rate limiting, because a password endpoint without it is a brute-force target"
short_code: "KAIROS-T-0202"
created_at: 2026-09-26T12:43:41.232716+00:00
updated_at: 2026-09-26T12:43:41.232716+00:00
parent: KAIROS-I-0018
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/todo"


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

- [ ] Repeated failed authentications from one source, or against one identity, are
      throttled; the threshold and window are configurable with sensible defaults
- [ ] The lockout decays; a legitimate user who mistypes is not locked out for long
- [ ] A throttled response reveals nothing about whether the account exists
- [ ] Where the state lives is a recorded decision, with the replica-count caveat
      documented if it is in-process
- [ ] What is trusted for the client address is documented
- [ ] A metric, and a log line, for a lockout
- [ ] Unit tests for the decay arithmetic and the identity/source split; integration
      coverage that a burst is actually refused
- [ ] `angreal test lint`, `unit` and `integration` green

## Status Updates

*To be added during implementation*
