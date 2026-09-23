---
id: relationship-lists-stop-silently
level: task
title: "Relationship lists stop silently losing archived neighbours"
short_code: "KAIROS-T-0158"
created_at: 2026-09-23T11:29:53.658068+00:00
updated_at: 2026-09-23T12:55:58.507904+00:00
parent: KAIROS-I-0015
blocked_by: [KAIROS-T-0156]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0015
---

## Parent Initiative

[[KAIROS-I-0015]]

## Objective

Stop archived items vanishing from *live* items' relationship lists. This
one degrades the record of work that is not archived at all: "what did this
initiative contain?" silently returns fewer rows than it should, and no
opt-in can currently recover them.

## Implementation Notes

**Blocked by [[KAIROS-T-0156]].**

`crates/kairos-db/src/graph.rs:403-419 neighbors_of` JOINs `entity_directory`,
so an archived neighbour is dropped from the result — not marked, not
counted, just absent. `graph.rs:55` documents the live-only root resolution
as deliberate, and that comment needs revisiting with this change.

Reached from `api/meta/relationships.rs:295 get_relationships`, and from the
GUI's Relationships panel and graph explorer.

Design call to make here and record: relationships are a property of the
*live* item being viewed, so archived neighbours should be **included and
marked** rather than hidden behind a flag. Hiding them is what produces the
misleading answer. If that proves contentious, a flag defaulting to
*included* is the fallback — but the default matters more than the flag.

Related live-only consumers to check while in here — these are rollups and
progress counts where excluding archived work is correct, so they should
keep their filter and now say so explicitly: `graph.rs:466-470`
(`CHILD_COLUMNS_SQL`), `:532-535` (`board_children_progress`), `:598-600`
(`team_work_documents`), `:715-729` (`item_subgraph`), `:874`/`:935` (forge
rollups).

Note `item_relationships` rows are hard-deleted (no `deleted_at`), so an edge
to an archived item is intact — only the JOIN hides it.

## Acceptance Criteria

- [x] A live item's relationship list includes archived neighbours, marked.
- [x] Progress rollups and child-progress counts still exclude archived work
      and say so explicitly in the query.
- [x] `item_subgraph` / the graph explorer draws archived nodes distinctly
      rather than omitting them.
- [x] A test covers the case that motivated this: an initiative with one
      live and one archived child.
- [x] `angreal test` green.

## Status Updates

**2026-09-23 — the design call, confirmed as written.** Archived
neighbours are **included and marked**, with no flag and no opt-out. The
reasoning survived contact with the code: an item's relationships are a
property of the item being viewed, which is usually live, so dropping an
endpoint does not narrow the answer, it falsifies it. A flag defaulting to
*included* would have been the same behaviour with a knob nobody should
turn, plus a second code path to keep honest. `Neighbor` gains
`archived_at`, `RelatedItem` gains `archived_at` (RFC 3339, skipped when
absent — so its mere presence means "archived").

**The widening, and its exact extent.** Two joins were widened, not one:

1. `graph.rs neighbors_of` — the line T-0156 named.
2. `graph.rs item_subgraph`'s hydration and degree passes — widened
   **deliberately**, against T-0156's default-live-only note, for two
   reasons written into the module docs and the commit. First, the
   explorer and the relationships panel draw the same edges; ADR-20's own
   negative consequence says a half-applied visibility rule is worse than
   none, because a reader who finds the row on one surface assumes the
   others agree. Second — and this is the part that only shows up in the
   code — omitting a node there was not a smaller picture, it was a broken
   one. `item_subgraph`'s walk reads `item_relationships` directly, so it
   already hopped THROUGH archived work; hydration then deleted the middle
   of the path and left the far side of it floating in the node set with
   no route back to the focus. An archived ROOT was worse still:
   resolution admits one (T-0154) and hydration dropped the focus out of
   its own subgraph. `degree` now counts archived neighbours too, because
   it exists so a client can render `+N` for what it is not drawing, and a
   count that disagreed with the node set made `+N` wrong.

Everything else stays live-only and now *says why* rather than relying on
a filter that has moved: `resolve_entity` (types the endpoints of a WRITE
— linking archived work is a mutation of frozen material),
`CHILD_COLUMNS_SQL` / `children_progress` / `board_children_progress` and
`blocks_summary` (ADR-20 rule 5), `team_work_documents`,
`team_link_rollup`, `repository_link_rollup` (default listings, rule 3).
The module's `# Liveness` section now enumerates both lists.

**The distinction, in one line, and it is the one to reuse:** containment
is a fact about the record, progress is a fact about live work. The item's
relationship LIST names its archived children; its progress ROLLUP does
not count them; `blocks_summary` does not count an archived blocker
because archived work cannot block anything.

**Tests.**

- `crates/kairos-db/tests/graph.rs` — new
  `archived_children_are_listed_marked_but_never_counted`: an initiative
  with one live and one archived child, asserting both halves together
  (list names both + marker carries a timestamp + the archived child
  hydrates fully; rollup counts one). It also reads FROM the archived
  side, which is the ADR-20 rule 1 audit answer.
- `focal_subgraph_contract` — its soft-delete leg **rewritten, not
  deleted**: 6 nodes → 7, the archived node asserted present with
  `archived_at` and its real column name, `degree` 3 → 4, edges 6 → 8,
  and the two edges through the archived node asserted present with their
  depths. The old "edges to soft-deleted endpoints excluded" assertion is
  now "the archived child's edge is drawn". Its `blocks_summary` leg is
  untouched and carries a comment naming the contrast.
- `crates/kairos-server/tests/archived_hidden.rs` — **the relationship leg
  only**. It asserted the archived child did not hydrate; it now asserts
  both children come back, archived one marked, `archived_at` parses as
  RFC 3339, and `children-progress` still totals 1. The module docs
  explain that rule 3 is not weakened, because a relationship list is not
  a listing of archived work. Sections 1-3 (boards, five family lists +
  totals, all three search capabilities) and section 5 are unchanged.

**MCP marks what it now serves.** `relationship_lines` tags every entry
`[archived]`, and `parent_chain` was widened to match — that chain IS the
rendering of the incoming `parent` edges (they are skipped in
`relationship_lines` and drawn there instead), so leaving it live-only
would have put the widened query back behind a filter for exactly one
relationship type, and the nearest archived ancestor would have truncated
the chain above it, hiding live grandparents with it.

**Cross-contamination, recorded rather than rewritten.** The
`crates/kairos-server/src/mcp/tools.rs` half of this task landed in
`2afe052` (KAIROS-T-0157's commit) — my blob was staged in the shared
index when that agent committed. The code is correct and in the tree; the
history is not worth editing. T-0158's own commit is the other seven
files.

**Gates.** `angreal test lint`: `cargo fmt --check` and
`cargo clippy --all-targets -D warnings` clean for every file this task
owns (checked per-crate; the workspace run was red on other agents'
in-flight `api/tasks.rs`, `api/strategies.rs`, `api/initiatives.rs`).
`angreal test unit`: green. Integration: a full `cargo test --workspace
--test '*' --no-fail-fast` run had 39/41 targets green with only the two
`kairos-cli` live targets failing on Dex refresh-token and compose
teardown contention from concurrent agents; both then passed in isolation
with this change applied, and `kairos-db::graph` +
`kairos-server::archived_hidden` were re-run green individually. The
shared compose stack was torn down mid-run twice by other agents — worth
knowing for anyone reading a red integration log in this initiative.

**For T-0163 / T-0164 (the GUI).**

- `RelatedItem` carries `archived_at: Option<String>` (RFC 3339, omitted
  when live). The Relationships panel must render it distinctly — an
  unmarked archived neighbour is worse than a missing one, because the
  reader will act on it. The web mirrors in
  `kairos-web/src/pages/item/api.rs` and `pages/search/data.rs` are
  partial structs, so they compile unchanged and silently ignore the
  field until you add it.
- `GraphNode` carries `archived_at` the same way, and `degree` now counts
  archived neighbours, so `+N` arithmetic still balances. The explorer
  must draw a marked node distinctly (ADR-20: never as live).
- The counterpart that must NOT change: `children-progress` and the board
  cards' blocks counts stay live-only. If a panel shows "2 children" from
  the relationship list beside "1 of 1 done" from the rollup, that is
  correct and the GUI should make the difference legible rather than
  reconcile it.

**For T-0165 (UAT).** A journey can now ask "what did this initiative
contain?" of a live initiative whose children were archived, and get the
full answer with markers — over API, MCP (`get_item`'s relationship lines
and parent chain, tagged `[archived]`) and the GUI. The progress rollup is
the counter-assertion worth making in the same step.

## Notes carried in from [[KAIROS-T-0156]]

**2026-09-23.** The line to widen is **`graph.rs:430`**, inside
`neighbors_of`. Every other `entity_directory` join in that file is a
different surface — `resolve_entity`, `team_work_documents`, the focal
subgraph's degree and hydration passes, `blocks_summary`,
`team_link_rollup`, `repository_link_rollup` (`graph.rs:153,620,755,760,847,900,961`)
— and all of them should **stay live-only** unless this task widens one
deliberately and says why.

T-0156 added a `# Liveness` section to the module docs stating that this
task's widening of `neighbors_of` is an intended change rather than a side
effect. Update it to match whatever you actually do.

There is a regression test to keep passing:
`crates/kairos-server/tests/archived_hidden.rs`, which asserts archived rows
are absent from every default listing — including the entity directory, via
relationship neighbour hydration. It was verified to bite when the filter is
removed from `neighbors_of`, so expect to update its relationship leg
deliberately as part of this task.