---
id: close-out-housekeeping-tells-the
level: task
title: "Close out: housekeeping tells the true story and the drift gate reads 18/18"
short_code: "KAIROS-T-0165"
created_at: 2026-09-23T11:30:10.130720+00:00
updated_at: 2026-09-23T11:30:10.130720+00:00
parent: KAIROS-I-0015
blocked_by: [KAIROS-T-0157, KAIROS-T-0160, KAIROS-T-0162]
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

Make the UAT arc tell the true story of archiving, cover the new verb so the
drift gate can go green, and record both full runs.

## Implementation Notes

### `housekeeping` flips

`uat/journeys/housekeeping.journey.ts` was written to assert today's
behaviour as it is, and it names [[KAIROS-T-0151]] in its header comment and
in step 3. That step currently asserts:

```ts
expect(item.status).toBe(404);
expect(history.status).toBe(404);
```

It becomes the opposite: the item and its history are retrievable, marked as
archived, while the board and the queue no longer show the card. The header
comment and the step narration both need rewriting — the journey's claim
changes from "the content is gone, only the trail survives" to "the content
survives, it is just out of the way", which is the whole point of
[[KAIROS-A-0020]].

Also check the closing step, which currently returns
`content_of_archived_work: 'not retrievable (see Status Update)'`.

### The drift gate

[[KAIROS-T-0160]] adds MCP `restore_item`, taking the tool count 17 → 18.
The gate (`uat/checks/zz-surface-coverage.check.ts`) fails until a journey
exercises it, and `ALLOW` is currently empty and should stay that way.

Cover it where a persona would really meet it — an audit story: someone asks
what a retired ticket said, finds it by search with archived included, reads
its history, restores it because the work came back, and the card returns to
the board. That is a real story and it exercises restore, archived search,
archived history and the refusal path in one arc. It belongs late in the
arc, next to `housekeeping`.

### Also touched

- `uat/journeys/operations.journey.ts:197-199` documents the
  `--include-deleted` + `--query` no-op as expected; [[KAIROS-T-0157]] makes
  it work.
- `uat/journeys/board-setup.journey.ts` can drop its defensive
  `set_metadata … null` once [[KAIROS-T-0162]] lands — verify teardown stays
  clean without it.
- `README.md`'s arc table and coverage claim (currently "17/17 tools and
  16/16 nouns"), and `uat/README.md`'s "The arc" section.

### Runs

Both modes, as KAIROS-T-0149 did: `angreal test uat` on a fresh compose seed
and `--server` against a kept stack, plus one `angreal test e2e`. Record run
ids and the Surface coverage line in the initiative's progress log. Check the
report for teardown residue — that is how the last close-out found a leak
two green runs had hidden.

## Acceptance Criteria

- [ ] `housekeeping` asserts retrievability; header comment and narration
      rewritten.
- [ ] A journey covers `restore_item`; the gate reads 18/18 tools with
      `ALLOW` still empty.
- [ ] `operations`' note about the search limitation is corrected.
- [ ] Both READMEs updated, including the coverage numbers.
- [ ] Both full runs green, run ids recorded, **no teardown residue**.
- [ ] `angreal test e2e` green.

## Status Updates

*To be added during implementation*
