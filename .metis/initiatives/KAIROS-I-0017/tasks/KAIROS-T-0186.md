---
id: lexical-relevance-rank-text-search
level: task
title: "Lexical relevance: rank text search instead of returning it unordered"
short_code: "KAIROS-T-0186"
created_at: 2026-09-24T02:27:43.243689+00:00
updated_at: 2026-09-24T02:27:43.243689+00:00
parent: KAIROS-I-0017
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0017
---

## Parent Initiative

[[KAIROS-I-0017]]

## Objective

Make the `q` capability of unified search return results **in order of how well
they match**. Today `text_match_ids` throws the score away — it returns a
`HashSet<Uuid>` — and the combined sort offers only `created_at`, `updated_at`
and `title`. A user searching forty tasks gets the forty, newest first, and the
best answer may be nineteenth.

This ships first because [[KAIROS-A-0021]] rule 7 makes lexical the fallback
everything else degrades to. Building it last would leave the system with no
degraded mode during its own backfill.

## Implementation Notes

### Technical Approach

`crates/kairos-db/src/search.rs`:

- `text_match_ids` returns scores as well as ids — `ts_rank_cd(tsv,
  websearch_to_tsquery('english', $1))` alongside the existing predicate. The
  query stays index-served; ranking is computed on the matched rows only.
- Carry the score through step 4's intersection into the hydrated rows, so
  `AnyItem` can expose it.
- Add `SortField::Relevance` in `crates/kairos-core/src/search.rs`, and
  `sort_items` orders by score descending, tie-broken by `short_code` ascending
  as every other field already is.

### The decision this task has to make and record

**Is relevance the default ordering when `q` is present?** Arguments both ways:
`created_at DESC` is what callers get today and changing it silently changes
every existing `q` search; but a ranked search that must be asked for is a
ranked search nobody uses, and the agent callers this initiative exists for will
not know to ask.

Recommendation: **yes, default to relevance when `q` is present and no explicit
`sort` was given**; `created_at DESC` stays the default when `q` is absent,
where relevance is meaningless. Record the choice in the Status Updates with the
reason, and note it in the A-0007 reference page, because A-0007 currently
promises deterministic sort and says *no `ts_rank`* — this task is the
deliberate reversal of that sentence and the ADR text has to stop saying it.

Relevance requested without `q` is a validation error, not a silent fallback.

### Dependencies

None. This is useful on its own and ships alone.

### Risk Considerations

- `ts_rank_cd` on a large match set is the cost, not the predicate. Measure
  against the seeded tenant and record the number; if it is material, ranking
  applies after the other capabilities have narrowed the set rather than before.
- Determinism must survive: equal scores must still produce one stable order, or
  pagination tears. The `short_code` tie-break is what guarantees it, and a test
  must cover two documents with identical scores.

## Acceptance Criteria

- [ ] `q` searches return a relevance score per hit, and `sort.field =
      relevance` orders by it
- [ ] Relevance is the default ordering when `q` is present and no `sort` was
      given; `created_at DESC` remains the default when `q` is absent
- [ ] `relevance` without `q` is rejected as a validation error
- [ ] Equal scores are broken deterministically by `short_code`, with a test
      that proves it
- [ ] The A-0007 reference page and the ADR's "no `ts_rank`" sentence are
      corrected, with the reason
- [ ] `angreal test` green

## Status Updates

*To be added during implementation*
