---
id: fix-web-item-tab-anchors-on-first
level: task
title: "Fix: web — item tab anchors on first render, repo description on the team page, repository picker in New task, swallowed first drop after navigation"
short_code: "KAIROS-T-0124"
created_at: 2026-09-22T12:12:45.354667+00:00
updated_at: 2026-09-22T12:12:45.354667+00:00
parent: KAIROS-I-0011
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/todo"


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

- [ ] `/items/<code>` never emits an href with an empty code (grep the rendered DOM in a kairos-web unit test or an e2e assertion on the Graph anchor).
- [ ] Team page Repositories rows show the description when set (e2e `repositories.spec` asserts the seeded payments-api blurb).
- [ ] New task modal on a delivery board whose team owns repositories offers a Repository select; a task created with one selected carries the repo chip immediately.
- [ ] Root cause of the swallowed first drop written in the status update, fixed, and `dragCard` no longer needs its retry (removed).
- [ ] All gates green.

## Status Updates

*To be added during implementation*
