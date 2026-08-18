---
id: gui-short-code-opens-item-detail
level: task
title: "GUI: short-code opens item detail + copy-link button on cards and detail header"
short_code: "KAIROS-T-0076"
created_at: 2026-08-16T14:55:42.713276+00:00
updated_at: 2026-08-17T12:25:01.256581+00:00
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

# GUI: short-code opens item detail + copy-link button on cards and detail header

## Objective

UAT feedback: "the ticket identifier when clicked should open up the detail — not the ticket title" and "tickets should have an easy 'copy link' button next to the ticket identifier." Make the short code the navigation affordance on cards, and add a one-click copy-link next to the identifier on both the card and the item detail header.

## Backlog Item Details

### Type
Feature (UX)

### Priority
P1 — direct UAT feedback on the primary board surface.

### Business Justification
- **User Value**: The identifier is the natural "handle" for a ticket — clicking it should open the ticket, and sharing a ticket link should be one click, not a copy-from-address-bar dance.
- **Effort Estimate**: S/M — the click-target swap is small; the clipboard plumbing is new for the WASM GUI.

## Current State

- **Card click behavior**: only the title is clickable — `<a class="kairos-card__title" href=…>` at `crates/kairos-web/src/pages/boards.rs:890–892` with `href = format!("/items/{short_code}")` (boards.rs:850). The card `<article class="kairos-card">` (boards.rs:862) has drag handlers but no click handler.
- **The short code is already its own element on the card**: `<Text mono=true dimmed=true size="xs">{code_text}</Text>` at boards.rs:887 (renders class `cl-mono`) — but it is plain text, not a link.
- **Route**: `app.rs:61` — `items/:code` → `ItemPage` (pages/item.rs:43–61). Navigation is SPA-style; the leptos router intercepts same-origin anchors (app.rs:231–233).
- **Detail header**: the short code is embedded in the `PageHeader` subtitle string (`format!("{} · {}", family.label(), item.short_code)`, item.rs:119) — not a distinct element. The header action row (item.rs:136–152) holds History / New document / Delete; a copy-link button slots into that Group.
- **Clipboard**: zero clipboard code exists anywhere in the repo. `crates/kairos-web/Cargo.toml:36–55` web-sys features do not include `Navigator`/`Clipboard` — those features plus promise plumbing must be added.
- **E2E coupling**: `smoke.spec.ts:154–156` clicks `a.kairos-card__title` to open detail; `smoke.spec.ts:128–130` scrapes the short code from `.cl-mono`. Both need updating in step with the markup change.

## Design Decisions (per UAT direction)

- The **short code becomes the anchor** to `/items/{short_code}` on cards.
- The **title stops being a link** ("not the ticket title"). The title remains plain text; the card stays draggable.
- A **copy-link button** (small icon button) sits next to the identifier on the card AND next to the identifier on the item detail header. It copies the absolute URL (origin + `/items/{code}`) to the clipboard and gives brief visual feedback (e.g. checkmark flash / tooltip "Copied").
- Detail header: render the short code as a distinct element (not just subtitle text) so the copy button has a home next to it.

## Acceptance Criteria

## Acceptance Criteria

- [x] Clicking the short code on a board card navigates to `/items/{short_code}` (SPA navigation — the leptos router intercepts the anchor exactly as it did the title's).
- [x] The card title renders as plain text (`span.kairos-card__title`) — no anchor; hover affordance removed from CSS.
- [x] A copy-link button renders next to the short code on cards; it writes the absolute item URL, flashes ✓ (1.5s), and `stop_propagation`/`prevent_default` keep it out of navigation and drag.
- [x] The item detail header renders the short code as a distinct mono element with the same copy-link button beside it (subtitle is the family label only).
- [x] web-sys `Navigator`/`Clipboard`/`MouseEvent` features added; async Clipboard API (secure contexts; localhost qualifies) — verified in Chromium via e2e; Firefox supports the same API (not e2e-covered; suite is chromium-only by config).
- [x] Copy button is a real `<button>` with aria-label "Copy link" (e2e targets it by role+name).
- [x] `smoke.spec.ts`: step 4 asserts code-is-link/title-is-text/copy-present; steps 7/7b navigate via the code anchor; step 7b grants clipboard permissions, asserts the ✓ flash and clipboard == page URL. Full e2e green (no retries).
- [x] Consistency audit: search result rows (search.rs:459), the relationships panel (item.rs) and explorer (relationships.rs:129,247), and activity (activity.rs:590) all already link the identifier to `/items/{code}` — the board card was the sole outlier, now fixed. Deferred: copy-link buttons on surfaces beyond cards + detail header.

## Resolved Questions

- **Whole-card click target**: identifier-only, per UAT wording. The card body stays reserved for drag.
- **Insecure-context clipboard**: no execCommand fallback — the button renders only when `navigator.clipboard` exists, so plain-HTTP deploys degrade to no affordance rather than a dead control.
- **Sequencing with T-0075**: implemented back-to-back; card layout reworked once, e2e updated in step.

## Status Updates

- 2026-08-16: Created from UAT feedback with investigation findings (design workflow, session ffc0d1f9).
- 2026-08-17: Implementation:
  - New shared `pages/copy_link.rs`: `CopyLinkButton` — async Clipboard API via web-sys (`Navigator`/`Clipboard`/`MouseEvent` features added), absolute URL from `location.origin`, ✓ flash with 1.5s revert, real `<button>` with aria-label "Copy link"; renders nothing when `navigator.clipboard` is undefined (insecure context) — decision: no execCommand fallback, degrade to no affordance.
  - boards.rs `ItemCard`: short code is now the detail anchor (`a.cl-mono.kairos-card__code`, keeps `.cl-mono` scrapeable for e2e) with `CopyLinkButton` beside it; title demoted to `span.kairos-card__title`.
  - item.rs header: subtitle is family label only; short code renders as its own mono element + `CopyLinkButton` in the facts row.
  - app.css: `.kairos-card__code` link styling, title hover removed, `.kairos-copy-link` styling, dead `.kairos-card__actions` rule dropped (leftover from T-0075).
  - e2e: smoke step 4 asserts code-is-link/title-is-text/copy-button-present; step 7 + 7b navigate via `a.kairos-card__code`; step 7b grants clipboard permissions, clicks copy, asserts ✓ flash and clipboard == page.url(); team-lens 5c navigates via the code anchor.
  - Unit tests, WASM build, tokens lint all green. E2E running.