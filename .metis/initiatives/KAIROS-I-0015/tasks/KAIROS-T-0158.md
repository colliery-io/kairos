---
id: relationship-lists-stop-silently
level: task
title: "Relationship lists stop silently losing archived neighbours"
short_code: "KAIROS-T-0158"
created_at: 2026-09-23T11:29:53.658068+00:00
updated_at: 2026-09-23T11:29:53.658068+00:00
parent: KAIROS-I-0015
blocked_by: [KAIROS-T-0156]
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

- [ ] A live item's relationship list includes archived neighbours, marked.
- [ ] Progress rollups and child-progress counts still exclude archived work
      and say so explicitly in the query.
- [ ] `item_subgraph` / the graph explorer draws archived nodes distinctly
      rather than omitting them.
- [ ] A test covers the case that motivated this: an initiative with one
      live and one archived child.
- [ ] `angreal test` green.

## Status Updates

*To be added during implementation*
