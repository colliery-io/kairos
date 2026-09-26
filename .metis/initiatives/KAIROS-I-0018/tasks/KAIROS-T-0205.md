---
id: the-gui-offers-a-password-form
level: task
title: "The GUI offers a password form when the deployment has local auth on"
short_code: "KAIROS-T-0205"
created_at: 2026-09-26T12:45:25.798830+00:00
updated_at: 2026-09-26T16:40:09.805454+00:00
parent: KAIROS-I-0018
blocked_by: [KAIROS-T-0203]
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

## Acceptance Criteria

- [x] `/api/config` reports whether local auth is enabled
- [x] The form appears only when it is; the provider button appears only when an
      issuer is configured; both appear when both are
- [x] An OIDC-only deployment's login page is unchanged — asserted in e2e, not assumed
- [x] The session token is held exactly as the OIDC token is, in memory
- [x] Enter submits; `autocomplete` attributes present; a pending state during the
      request
- [x] The server's 401 is rendered without embellishment
- [x] An e2e spec logs in with a password and reaches a board
- [x] `angreal web lint`, `web build --release`, and `angreal test e2e` green

## Status Updates

### 2026-09-26 — done

`LoginPage` now reads `/api/config` and splits into `ProviderButton` and
`PasswordForm`, rendering whichever the deployment offers. `auth::password_login`
posts to `/api/login` and installs the bearer through the same `install` the OIDC
path uses.

#### The three states, and how "unchanged" is proven

The e2e harness runs the GUI server with local auth on **alongside** the Dex issuer, so
every spec now exercises the both-paths state — the one easiest to get wrong, and the
one all the existing specs previously walked straight past.

"An OIDC-only login page is unchanged" is asserted in three pieces rather than one,
because asserting it in a browser would need a second GUI server:

- `tests/web.rs` asserts an OIDC-only deployment's `/api/config` reports
  `local_auth: false` with a real issuer and authorization endpoint;
- `kairos_web::auth`'s tests assert `can_sso()` / `local_auth` over all three shapes,
  including a pre-T-0208 payload, and the page's branch is a pure function of them;
- 18 existing specs still drive the provider button. That is not a formality — it is
  what caught the naming collision below.

#### The defect that mattered

I named the form's button "Sign in", the same as the provider button. Two buttons with
the same accessible name on one page is ambiguous for a person choosing between them
and for anything addressing the page by role and name — **it broke `smoke` and
`team-lens`** with a strict-mode violation.

Renaming it to "Sign in with password" did **not** fix it: Playwright's accessible-name
matching is substring by default, so `{ name: 'Sign in' }` still matched both. The
button is now **"Log in"**, which collides with nothing and pairs with the header's
"Log out". Worth recording as a design rule, not just a test fix: on a page offering two
sign-in paths, the two controls need names that differ at the start, not at the end.

#### Raw elements, deliberately

The form uses a real `<form>` with `type="submit"` and aurora's CSS classes rather than
`TextInput`/`PasswordInput`. Those components expose no `autocomplete`, no `<form>` and
no `id` the caller controls, and a login form needs all three: Enter must submit, and a
password manager must recognise the fields or people choose weaker passwords. This page
already rendered its button as a raw `cl-btn`, so raw-with-aurora-classes is the
established shape here rather than a new exception. An `autocomplete` prop on the aurora
inputs would be a reasonable addition later; it was not worth a design-system release for
one form. The `autocomplete`, `type` and label associations are asserted in e2e, because
they are invisible and nothing else would notice their absence.

#### In memory, with a cost stated out loud

The session bearer goes through `Auth::install` with no refresh token and no expiry, so
it lands in the in-memory signal and **nowhere else**. Not `localStorage`, and not the
`sessionStorage` slot KAIROS-T-0071 uses for refresh tokens either: that token can only
be redeemed at the issuer, while a session bearer is a working API credential.

The consequence is real — **a page reload ends a password session** — and there is a
test asserting it, so nobody "fixes" it later by persisting the bearer. If reload
survival is wanted, the answer is a refresh mechanism for sessions, not moving the
credential into storage.

#### Two smaller things

`RedirectToIssuer` now asks `/api/config` first and lands on `/login` when the
deployment cannot SSO. Without that branch an unauthenticated deep link on a
local-only deployment would render "Sign-in unavailable" on a deployment that can
perfectly well sign people in — the worst possible first impression, one click from the
page that works. A failed `/api/config` still falls through to the redirect, because
that is what happened before the page could ask.

`take_return_to` / `stash_return_to` share the PKCE path's `sessionStorage` key, so a
deep link lands where the person was headed whichever way they signed in. Both ends
check the value starts with `/`, or the login page would be an open redirect.

#### Harness

`_prepare_gui_stack` runs `kairos-server set-password` **after** the reseed — doing it
before would leave the outcome depending on whether `seed-demo` happens to preserve the
column, which is not something this harness should have an opinion about. The password is
a fixture in `task_test.py` and nothing outside that file knows it.

Two other test-only mistakes of mine, both caught by running it: the header shows a
display name rather than an email, and `.kairos-board-grid` is per-board so the bare
class matched five elements.

**Files:** `pages.rs` (`LoginPage`, `ProviderButton`, `PasswordForm`), `auth.rs`
(`password_login`, `login_error_message`, `take_return_to`, `stash_return_to`, public
`config_cached`), `app.rs` (the fallback branch), `app.css`, `web.rs` (the
`local_auth` assertion), `e2e/tests/local-login.spec.ts` (new, 2 tests),
`.angreal/task_test.py`.

`angreal web lint`, `web build --release`, `angreal test lint`, `unit` and
`angreal test e2e` (19 specs) are all green.