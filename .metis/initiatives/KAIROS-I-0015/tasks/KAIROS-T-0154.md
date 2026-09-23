---
id: archived-aware-resolution-item-get
level: task
title: "Archived-aware resolution: item GET and history stop 404ing"
short_code: "KAIROS-T-0154"
created_at: 2026-09-23T11:29:44.212632+00:00
updated_at: 2026-09-23T11:50:44.956719+00:00
parent: KAIROS-I-0015
blocked_by: [KAIROS-T-0153]
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

Serve an archived item and its version history instead of 404ing. This is
the heart of [[KAIROS-A-0020]] and it closes the read half of
[[KAIROS-T-0151]]: "what did that ticket say?" gets an answer again.

## Implementation Notes

**Blocked by [[KAIROS-T-0153]]** — ABAC must resolve first, or archived work
becomes org-admin-only.

The chokepoint is `crates/kairos-server/src/api/mod.rs`:

- `mod.rs:166-190 resolve_short_code` queries `entity_directory` (live-only).
- `mod.rs:211-215 short_code_not_found` — its message reads *"no live
  {entity_type} with short code"*, which is currently accurate and is about
  to become a lie.

Introduce an explicit mode rather than a bool, so call sites read as
intentions:

```rust
enum Liveness { LiveOnly, IncludeArchived }
```

Thread it through the five per-family loaders: `api/tasks.rs:114`,
`api/strategies.rs:52`, `api/initiatives.rs:53`, `api/documents.rs:111`,
`api/adrs.rs:49`.

**Every existing call site passes `LiveOnly`.** The only ones that change to
`IncludeArchived` are the GET-by-short-code handlers (`tasks.rs:184-192` and
its four siblings) and history. A reviewer should be able to verify "no
default listing changed" by checking that every *other* call site still
names `LiveOnly`.

History is pure resolution and comes free: `api/meta/history.rs:60` calls
`resolve_family_item` (`api/meta/mod.rs:205-219`), and the 404 comes from
there. The `item_history` rows themselves are never liveness-filtered
(`history.rs:63-88`), so once resolution succeeds the history is already
correct.

Archived rows must be **marked, not disguised**: add `archived_at` to the
item payload (the existing `deleted_at`, renamed for the wire — check
`kairos-client` DTOs). An auditor must never mistake archived for live.

Note `entity_directory` is still live-only at this point; this task resolves
against the base tables. [[KAIROS-T-0156]] fixes the view.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [ ] `GET /api/{family}/{short_code}` returns an archived item with
      `archived_at` set, for all five families.
- [ ] `GET /api/{family}/{short_code}/history` returns its versions.
- [ ] Every mutating handler still refuses an archived item (read-only by
      construction — D5).
- [ ] List endpoints, board items and search are **unchanged** — a test
      asserts an archived item is absent from each.
- [ ] `short_code_not_found` no longer claims "live".
- [ ] `angreal test` green; `angreal test uat --journey housekeeping` still
      green against its current assertions, or updated in the same commit if
      it cannot be.

## Status Updates

*To be added during implementation*
**2026-09-23 — done.** Commit `7517465`.

`Liveness` is an enum in `api/mod.rs`, not a bool, so every call site names
its intention. Threaded through `resolve_short_code`, `resolve_family_item`
(`api/meta/mod.rs`) and the five `load()` fns. `short_code_not_found` no
longer claims "live".

**Exactly six call sites pass `IncludeArchived`**, all reads: the five
GET-by-short-code handlers and `/history`. Everything else names
`LiveOnly` — which is the reviewable form of "no default behaviour
changed".

`entity_directory` is still live-only, so `IncludeArchived` resolution runs
against a `DIRECTORY_UNION` const over the five base tables.
**[[KAIROS-T-0156]] should collapse that back into the view** once the view
exposes `deleted_at` — the const exists only because the view cannot answer
yet.

### Two decisions taken while implementing

- **Reading an archived item's metadata and relationships is included**
  (`api/meta/metadata.rs:61`, `api/meta/relationships.rs:76/131/195/303`),
  though the task listed only item GET and history. Serving the item while
  its metadata 404s is precisely the partial rollout the initiative warns
  about. Writing them stays `LiveOnly`. What the graph *returns* (archived
  neighbours) is still [[KAIROS-T-0158]]'s question — this is only about
  the focal item resolving.
- **`cascade-preview` stays `LiveOnly`.** It answers "what would this delete
  take out of circulation?", and archived work is already out.

### Wire

`archived_at` (RFC 3339, `skip_serializing_if = "Option::is_none"`) on all
five DTOs in `kairos-client/src/types.rs`, filled from `deleted_at` in
`api/convert.rs`. Absent while live.

### Tests

Two existing assertions encoded the old contract and were **rewritten, not
deleted** — they are the clearest statement of what changed:

- `crates/kairos-server/tests/entities.rs` asserted *"soft-deleted rows
  404"*. Now: retrievable, `archived_at` set, history non-empty, absent from
  the default list, and PATCH refused.
- `crates/kairos-server/tests/cascade_preview.rs` asserted a cascaded
  descendant *"is gone"*. Now: retrievable and marked. This is the more
  important of the two — a cascade archives work nobody chose to archive.

`cargo test -p kairos-server --no-fail-fast` → all 24 binaries green.

### Note for whoever runs tests during this initiative

`org_endpoints` failed once mid-run with `items.columns.len() == 5` while
[[KAIROS-T-0161]] was mid-edit in the same working tree. It passed on the
next run. If a board-columns-shaped assertion fails, check whether T-0161 is
in flight before chasing it.