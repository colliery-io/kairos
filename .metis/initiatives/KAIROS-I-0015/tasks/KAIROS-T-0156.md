---
id: entity-directory-and-searchable
level: task
title: "entity_directory and searchable_items expose deleted_at; consumers filter explicitly"
short_code: "KAIROS-T-0156"
created_at: 2026-09-23T11:29:48.965493+00:00
updated_at: 2026-09-23T11:29:48.965493+00:00
parent: KAIROS-I-0015
blocked_by: []
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

Move the liveness filter out of the two database views and into their call
sites, so that "show me archived work" has something to widen. This is the
enabling change for search, traverse and the graph — and the highest-risk
task in the initiative for a silent regression.

## Implementation Notes

Both views live in
`crates/kairos-db/migrations/tenant/2026-07-09-000000_create_tenant_schema/up.sql`:

- `searchable_items` — up.sql:364-386, `WHERE deleted_at IS NULL` on every
  UNION branch.
- `entity_directory` — up.sql:396+, the same.

They are upstream of nearly every contradiction in this initiative, which is
why a `?include_deleted=` parameter bolted onto the handlers cannot work:
the parameter would have nothing to widen. That is precisely the bug that
makes `--include-deleted` a silent no-op alongside `--query` today.

Write a new tenant migration that recreates both views **selecting
`deleted_at`** rather than filtering on it. No table changes.

Then make every consumer filter explicitly. Known consumers:

- `api/mod.rs:166-190` (resolution — already modal after T-0154)
- `kairos-db/src/search.rs:266-292` (traverse root), `:350-362`
  (`text_match_ids`), `:431-467` (`partition_by_type`)
- `kairos-db/src/graph.rs:403-419` (`neighbors_of`), and the live-only
  consumers at `:466-470`, `:532-535`, `:598-600`, `:715-729`, `:874`, `:935`

### Guarding against the regression

A consumer that forgets to filter starts leaking archived rows into a
default listing, silently. In order of usefulness:

1. `crates/kairos-db/tests/search.rs:637-679` pins current behaviour — it
   must keep passing **unchanged** for the live-only paths.
2. The `housekeeping` UAT journey asserts end to end that default listings
   hide archived work.
3. After this lands, every `entity_directory` / `searchable_items` reference
   in `crates/` must name a liveness mode. Worth a grep in review.

Performance: the default path is the one that must stay fast. The partial
indexes (`idx_tasks_column` and siblings, up.sql:181/204/225) are
`WHERE deleted_at IS NULL`; check the planner still uses them once the
filter moves into the query.

## Acceptance Criteria

- [ ] A tenant migration recreates both views exposing `deleted_at`.
- [ ] Every consumer names its liveness mode explicitly.
- [ ] `crates/kairos-db/tests/search.rs:637-679` passes unmodified.
- [ ] A test asserts an archived item is absent from every default listing
      (boards, the five family lists, search without the flag, the entity
      directory).
- [ ] `EXPLAIN` on the default board and list queries still uses the partial
      indexes.
- [ ] `angreal test` green; full compose UAT green.

## Status Updates

*To be added during implementation*
