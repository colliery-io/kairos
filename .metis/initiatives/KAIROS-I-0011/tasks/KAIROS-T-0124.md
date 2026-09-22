---
id: fix-web-item-tab-anchors-on-first
level: task
title: "Fix: web — item tab anchors on first render, repo description on the team page, repository picker in New task, swallowed first drop after navigation"
short_code: "KAIROS-T-0124"
created_at: 2026-09-22T12:12:45.354667+00:00
updated_at: 2026-09-22T12:48:16.005584+00:00
parent: KAIROS-I-0011
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0011
---

# Fix: web — item tab anchors on first render, repo description on the team page, repository picker in New task, swallowed first drop after navigation

## Parent Initiative

[[KAIROS-I-0011]]

## Objective

Close UAT findings #2, #6 and #8 in `crates/kairos-web`.

## Implementation Notes

### Technical Approach

- **#2 (`pages/item.rs` `ItemPage`):** `params.read().get("code")` is empty on the first render, so the Details/Graph `<Anchor>` hrefs are built as `/items/?view=graph`. Guard: when `code` is empty render nothing (or a `Loading…`), never the tabs; build the hrefs inside the same closure that already has the resolved code. Add a projection test if the tab-href builder is factored into a pure fn (`tab_hrefs(code, graph_mode)`).
- **#6a (`pages/teams.rs` Repositories panel):** show `repo.description` under the row when non-empty (dimmed `Text size="sm"`, wrapped) — the "how to work here" blurb agents read should be visible to the humans on the team page. Repository DTO already carries it (check `pages/repositories/api.rs` mirror; add the field if the mirror omits it).
- **#6b (`pages/boards.rs` create modal, tasks only):** a `Repository` select listing the board's team's repositories (the board already loads `board_repos` for the lens) with `(none)` default; on Create send `repository: Some(slug)` in the `CreateTaskRequest` mirror (the server routes by it; `board_id` stays set so the column/lane logic is unchanged). Only render the select when the team owns ≥1 repository. Keep the item-page picker.
- **#8 (board drag):** diagnose why the first HTML5 drop after `page.goto('/boards/<slug>')` is swallowed while the second lands (`uat/journeys/cross-team.journey.ts` step 7 hits it every run; the e2e drag spec never sees it because it navigates in-app). Hypotheses to check in order: (a) the column `dragover`/`drop` handlers are attached in an `Effect` that runs after the first paint, so a drop in the first ~100ms has no `preventDefault` and the browser cancels it; (b) the WS-driven refetch after `openBoard` re-keys the column `<For>` and the element the drag started on is replaced mid-drag. Fix the cause (e.g. attach handlers in the view, key columns by id, or debounce the refetch while a drag is in progress). Prove it by removing the retry in `uat/surfaces/gui.ts::dragCard` and running `--journey cross-team,agent-loop,planning` three times green.
- Gates: `angreal test lint` (workspace clippy), `cargo test -p kairos-web --lib`, `angreal web lint`, `angreal web build`, `angreal test e2e`.

### Dependencies

None (T-0125 consumes the results).

## Acceptance Criteria

- [x] `/items/<code>` never emits an href with an empty code (grep the rendered DOM in a kairos-web unit test or an e2e assertion on the Graph anchor).
- [x] Team page Repositories rows show the description when set (e2e `repositories.spec` asserts the seeded payments-api blurb).
- [x] New task modal on a delivery board whose team owns repositories offers a Repository select; a task created with one selected carries the repo chip immediately.
- [x] Root cause of the swallowed first drop written in the status update, fixed, and `dragCard` no longer needs its retry (removed).
- [x] All gates green.

## Status Updates

- 2026-09-22: Implemented in `7a7ca93`. Gates: `angreal test lint` exit 0
  (fmt + workspace clippy `-D warnings`); `cargo test -p kairos-web --lib`
  → `test result: ok. 72 passed; 0 failed`; `angreal web lint` → "kairos-web
  token rule: clean"; `angreal web build` → bundle written; `angreal test
  e2e` → `11 passed`, "E2E PASSED"; `cd uat && npx tsc --noEmit` exit 0.
  Proof for #8: `--grep "@cross-team|@agent-loop|@planning"` against the
  kept compose stack, three runs, each "3 journeys, 3 passed, 0 failed"
  with the retry gone (cross-team step 7 went from 10.6s — two timed-out
  first attempts — to ~0.4s).

  **#8 root cause — not a swallowed drop; the drag never starts.** Traced
  with capture-phase listeners, a MutationObserver and a hooked
  `preventDefault` on the live board: a failing attempt delivers only
  `mousedown` and `mouseup` to the page — no `dragstart`, no `dragover`,
  no `drop`, no DOM mutation, no handler runs; nothing in kairos-web
  executes at all. Both ticket hypotheses are ruled out (handlers are
  attached in the view and were present; the WS subscription refetches
  only on events/reconnects, and no refetch happened mid-gesture). What
  differs between a failing and a landing attempt is a **scroll between
  `mousedown` and the first `mousemove`**: `locator.dragTo` is
  hover(source) → mouse.down → hover(target) → mouse.up, and a hover
  scrolls its element into view (CDP `scrollIntoViewIfNeeded`, centre if
  partially visible). The platform-delivery board overflows the journeys'
  1280×720 viewport both ways (five 260px columns = 1478px in a 996px
  lane; two stacked lanes push the Planned lane below the fold), so
  hovering the target column scrolls the window (Todo: +155px) or the
  lane's `.kairos-board` (Active: scrollLeft +182px) after the press.
  Chromium then hit-tests the drag origin at the stale viewport point
  (`MouseEventManager::HandleDrag` → `DragController::DraggableNode`),
  finds no draggable element there and abandons the drag before
  `dragstart`. The "second attempt lands" pattern follows: the page is
  already scrolled, hover(target) is a no-op, the drag starts. It
  reproduces on a freshly-created card too (every successful drop moves
  the card to a column with a different scroll target), which is why the
  Active→Completed drag in step 7 also needed the retry. Plain-page
  controls (same Playwright, same Chromium, no overflow) land 6/6; the
  board at 1800×1400 lands 3/3 first time with the old driver. Fix: the
  UAT driver scrolls both ends into view first, presses, and moves to a
  point of the target that is on screen; the e2e `lanes.spec`'s
  `toPass` wrappers around `dragTo` exist for the same reason and were
  left as-is. A layout change (fluid columns so five fit at 1280px, or a
  viewport-height board with per-column scrolling) would reduce the
  exposure for drivers, but a human never scrolls mid-press, so it was
  not made under this ticket — Dylan's call.

  **#2** could not be reproduced on today's code (12 timed attempts of
  the T-0119 flow: create from the header → click the code → click Graph
  immediately, plus a full navigation; every tab href carried the code;
  `Family::of_short_code("")` is `None`, so the tabs never rendered for an
  empty param). The guard was still made explicit: `tab_hrefs(code)` is
  `None` for an empty param and the page shows `Loading…` instead, with a
  unit test. **#6a**: `repo.description` rendered under each Repositories
  row (`.kairos-team__repo-description`), e2e asserts the seeded blurb.
  **#6b**: `CreateItemModal` loads the owning team's repositories when it
  opens (tasks only), renders a `Repository` select only when there is at
  least one, and sends `repository: <slug>` beside `board_id`
  (`CreateTaskRequest` mirror + wire-shape test); e2e creates through
  the modal and sees the chip at once. Seed note: the platform team owns
  three repositories (`checkout-api` too), so the e2e asserts
  membership, not the exact list.

  Also observed, not fixed: the journeys' `cross-team` step 1 parses
  `get_repository`'s `- delivery board:` line as a UUID; since T-0123
  (`3cba74b`) it prints the slug, so the journey fails at step 1 on the
  current server — T-0125's to update (the runs above used a local,
  uncommitted widening of that regex).