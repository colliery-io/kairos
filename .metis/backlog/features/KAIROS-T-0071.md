---
id: gui-survive-page-reload-refresh
level: task
title: "GUI: survive page reload — refresh token in sessionStorage with silent restore"
short_code: "KAIROS-T-0071"
created_at: 2026-08-09T21:14:47.546870+00:00
updated_at: 2026-08-09T21:30:17.018100+00:00
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

# GUI: survive page reload — refresh token in sessionStorage with silent restore

## Objective **[REQUIRED]**

A page reload should not end the session. Persist the refresh token in `sessionStorage` and, on app boot, silently run the refresh grant to restore the session before the auth guard redirects — seamless per-tab reloads against ANY IdP, including the dev Dex (whose password connector has no SSO session, so today every reload lands on the login form).

UAT feedback (Dylan, 2026-08-09): "when I refresh the web UI I immediately go back to login." Option 2 of three (live-with-it / sessionStorage restore / BFF cookie) — chosen explicitly.

## Backlog Item Details **[CONDITIONAL: Backlog Item]**

### Type
- [x] Feature - New functionality or enhancement

### Priority
- [x] P1 - High (important for user experience)

### Business Justification **[CONDITIONAL: Feature]**
- **User Value**: Reload/F5 is constant during real use; losing the session every time makes the GUI feel broken even though it's "as designed" (A-0015 in-memory-only tokens + Dex's missing IdP session).
- **Effort Estimate**: S
- **ADR impact**: Amends KAIROS-A-0015's "access token in memory... no long-lived cookies" stance: the ACCESS token stays memory-only; the REFRESH token moves to `sessionStorage` (per-tab, cleared on tab close — not localStorage, not a cookie). Amendment to be recorded on the ADR.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [x] `install()` stashes the (possibly rotated) refresh token in `sessionStorage` on every grant; `logout()` removes it; a failed refresh (`expire()` path) removes it so a dead token can't loop
- [x] On app boot with a stored refresh token, the guard shows a loading state (no premature issuer redirect) while a silent refresh-grant restore runs; success lands the user where they were, failure falls through to the normal login redirect
- [x] Reload after login no longer shows the Dex form on the dev stack
- [x] Explicit logout then reload does NOT silently sign back in
- [x] KAIROS-A-0015 carries the amendment; docs/gui-conventions.md § Auth updated
- [x] Playwright coverage: post-login `page.reload()` stays authenticated (no `#login`); `angreal test e2e` green; unit + `angreal web lint` green

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
`crates/kairos-web/src/auth.rs`: new `KEY_REFRESH` sessionStorage key; `Auth.restoring: RwSignal<bool>` initialized true iff the key exists; `provide_auth` spawns the restore (refresh grant via the existing relay path, reusing `install`); `Shell`'s `GuardFallback` renders `Loading` while `restoring` (instead of redirecting). Refresh-token rotation is handled for free because `install` re-stashes whatever the grant returned.

### Risk Considerations
sessionStorage is same-tab and non-persistent — the standard SPA middle ground. An XSS that could read it could equally exfiltrate the in-memory token; the meaningful delta is offline persistence, which sessionStorage does not add.

## Status Updates **[REQUIRED]**

- 2026-08-09: Created from UAT feedback; approach approved by Dylan (option 2).
- 2026-08-09: FIRST E2E FAILED — root-caused via a headless Playwright trace: the restore's refresh grant succeeded (200) but LOST A RACE. Shell/route LocalResources fire eagerly on mount, BEFORE the restore completes, carrying no bearer; their guaranteed 401s hit `decode_response`'s blanket `auth.expire()`, which stomped the freshly restored session AND wiped the stored refresh token → issuer redirect → Dex form. Fix: `decode_response` now receives the token the request actually carried and expires ONLY when `sent_token.is_some() && auth.token() == sent_token` — a 401 from a token-less or since-replaced token proves nothing about the current session. Verified by rerunning the trace: reload → refresh grant 200 → stays on /boards, storage intact.
- 2026-08-09: COMPLETE. Second `angreal test e2e` fully green — both Playwright specs passed clean (smoke 1.1s, team-lens 773ms incl. the reload and logout-reload steps), no retries. All ACs ticked.
- 2026-08-09: Implemented. auth.rs: `KEY_REFRESH` sessionStorage key; `install()` stashes on every grant (rotation-safe — Dex rotates refresh tokens); `logout()` AND `expire()` clear it (a 401-triggered expire also clears — no UX loss since in-session expiry redirects to the issuer regardless, and it prevents dead-token restore loops); `refresh()` refactored onto shared `refresh_with(token)`; `provide_auth` initializes `restoring=true` iff a token is stored and spawns `restore_session` (refresh grant → install or fall through). app.rs `GuardFallback` waits on `restoring` with a "Restoring session…" Loading state before the signed-out/issuer choice. docs/gui-conventions.md § 6 updated; KAIROS-A-0015 amended in place (dated, attributed). team-lens.spec.ts gains step 1b (post-login `page.reload()` keeps the session, lands on /boards, never sees #login) and step 8 (logout → reload stays on /login — no silent re-signin). cargo check, unit (9 suites), web lint green; full `angreal test e2e` running.