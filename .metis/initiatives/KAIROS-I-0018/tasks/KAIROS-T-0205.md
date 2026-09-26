---
id: the-gui-offers-a-password-form
level: task
title: "The GUI offers a password form when the deployment has local auth on"
short_code: "KAIROS-T-0205"
created_at: 2026-09-26T12:45:25.798830+00:00
updated_at: 2026-09-26T12:45:25.798830+00:00
parent: KAIROS-I-0018
blocked_by: [KAIROS-T-0203]
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

An email/password form on the login page, shown only when the deployment has local
auth on, beside the existing provider button when it has both.

## Dependencies

[[KAIROS-T-0203]].

## Implementation Notes

### The GUI has to be told

The SPA cannot guess. `/api/config` is the public, unauthenticated endpoint the GUI
already reads for its OIDC client configuration (`crate::web`), so it gains a
`local_auth: bool`. That is the whole mechanism: the form renders on that flag.

Three states, all of which must look deliberate:

| Deployment | Login page |
|---|---|
| OIDC only | provider button, as today — **unchanged, and asserted as unchanged** |
| local only | the form, and no provider button |
| both | both, with the provider first and a visible separator |

### The token goes where the OIDC token goes

Per [[KAIROS-A-0015]] the SPA holds its token in memory, not in storage. A session
bearer from `/api/login` goes to the same place by the same path — if it ends up in
`localStorage` "because it is simpler", that is a regression in the deployment's
security posture, not a convenience.

### Getting the small things right

- A real `<form>` with `type="submit"`, so Enter submits and a password manager
  recognises it. `autocomplete="email"` and `autocomplete="current-password"`.
- `PasswordInput` exists in aurora and now has a proper label association
  ([[KAIROS-T-0198]]) — use it rather than a bare `TextInput`.
- The 401 is deliberately uninformative ([[KAIROS-T-0203]]). Render it as-is;
  **do not help** by guessing "no account with that email", which would reintroduce
  the enumeration oracle the endpoint was careful to avoid.
- A pending state on the button. The endpoint is intentionally slow — argon2 —
  so a form with no feedback reads as broken.

### Coverage

`e2e` drives a real browser against a real server. The existing specs log in through
Dex's form; this one logs in through **Kairos's own**, which makes it the first spec
that does not depend on the IdP at all.

## Acceptance Criteria

- [ ] `/api/config` reports whether local auth is enabled
- [ ] The form appears only when it is; the provider button appears only when an
      issuer is configured; both appear when both are
- [ ] An OIDC-only deployment's login page is unchanged — asserted in e2e, not assumed
- [ ] The session token is held exactly as the OIDC token is, in memory
- [ ] Enter submits; `autocomplete` attributes present; a pending state during the
      request
- [ ] The server's 401 is rendered without embellishment
- [ ] An e2e spec logs in with a password and reaches a board
- [ ] `angreal web lint`, `web build --release`, and `angreal test e2e` green

## Status Updates

*To be added during implementation*
