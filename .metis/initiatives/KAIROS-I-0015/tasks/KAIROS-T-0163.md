---
id: the-search-page-can-ask-for
level: task
title: "The search page can ask for archived work"
short_code: "KAIROS-T-0163"
created_at: 2026-09-23T11:30:04.957939+00:00
updated_at: 2026-09-23T11:30:04.957939+00:00
parent: KAIROS-I-0015
blocked_by: [KAIROS-T-0157]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0015
---

## Parent Initiative

[[KAIROS-I-0015]]

## Objective

Let someone ask for archived work from the search page. Today the GUI cannot
express the question at all, which makes "visible for audit and search" false
for the surface most people actually use.

## Implementation Notes

**Blocked by [[KAIROS-T-0157]].**

`crates/kairos-web/src/pages/search/data.rs:67-69` — the GUI's `SearchFilter`
mirror is explicitly partial, with a comment saying `team_id` / `is_bucket` /
`include_deleted` "aren't in the T-0042 builder". The builder has to be able
to express it before anything else here is possible.

Then a **visible toggle** on the search page, not a URL-only parameter.
"Visible for audit" means discoverable by someone who did not read the docs.
Label it for what it does ("Include archived work"), and show archived hits
distinctly in the result list rather than mixed in unmarked.

### Vocabulary hazard

`kairos-web/src/pages/item.rs:269,315` already uses "archived" for the
**document editorial lifecycle** (`draft | review | published | archived`,
KAIROS-T-0078), which is unrelated — a published document can be editorially
archived while perfectly live. The GUI must not use one word for two things
on the same screen.

This initiative does not rename anything (see non-goals). Pick copy that
distinguishes them, and leave a comment recording the collision for whoever
does the eventual rename.

Aurora Dark tokens and the existing filter-chip patterns apply; follow the
entity-type chips already on the page.

## Acceptance Criteria

- [ ] The GUI `SearchFilter` can express `include_deleted`.
- [ ] A visible, labelled toggle on `/search`, off by default.
- [ ] Archived hits are visually distinct from live ones.
- [ ] Copy does not collide with the document lifecycle's "archived"; a
      comment records the collision.
- [ ] Default search results are unchanged with the toggle off.
- [ ] `angreal test` green.

## Status Updates

*To be added during implementation*

## Notes carried in from [[KAIROS-T-0164]] — the GUI vocabulary is settled

**2026-09-23.** T-0164 resolved the "archived" collision on the item page,
and this task must match it rather than invent a second answer.

- **Call the ADR-20 state "put away"** in all user-facing copy. Never a bare
  "archived" anywhere a document's editorial `lifecycle:` badge can also
  appear — that is the collision (KAIROS-T-0078's
  `draft|review|published|archived`, which is unrelated to `deleted_at`).
- `.kairos-archived-badge` is the shared class: **solid** gold outline for
  the put-away state; the **dashed** badge is the editorial one.
- Wire names: entity DTOs carry `archived_at` (absent while live). Board
  columns carry `removed_at`, and only when asked for
  (`GET /api/boards/{id}?include_removed_columns=true`).
- Search result DTOs live in `crates/kairos-client/src/types_search.rs`.
  **Confirm a search hit actually carries `archived_at` before designing a
  marker for the result list** — [[KAIROS-T-0157]] is editing that file, and
  whether hits are marked is its acceptance criterion, not an assumption
  this task may make.
- Linking works now: any archived hit can link to `/items/:code` or
  `/activity/history/:code` and get a real page rather than an error.
