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

## Notes carried in from other tasks

**2026-09-23, from [[KAIROS-T-0156]].** `uat/journeys/housekeeping.journey.ts`
was **already rewritten** — it had gone red the moment T-0154 landed, since
it still asserted 404 on the item and its history. It now asserts both
halves of ADR-20: off the MCP queue AND off `/api/tasks` (codes *and*
count), but 200 with `archived_at` set on both item and history. Verify
rather than redo, and check the closing step's
`content_of_archived_work: 'not retrievable'` was updated with it.

**2026-09-23, from [[KAIROS-T-0164]] — copy and e2e discipline.**

- Use **"put away"** for the ADR-20 state in any narration, never a bare
  "archived" beside a document's `lifecycle:` badge.
- `e2e/tests/archived.spec.ts` already covers the browser leg (3 specs, all
  five families, the both-senses document, and a removed column). The new
  UAT journey should tell a *story* rather than duplicate it.
- **Fixture hazard, learned the hard way:** `smoke` and `team-lens` each
  assert exactly 5 board tiles, and a second live delivery board on a team
  makes `POST /api/tasks` routing ambiguous (422). An early draft of the e2e
  spec created its own board and broke three unrelated specs. **Create
  nothing that stays visible.** Archived items and removed columns are
  invisible by construction, so they are safe residue.
- A-0001 cascades along **parent** edges only, so a document attached by
  `supports` must be archived in its own right.
- A restore puts back only the item asked for; `still_archived_short_codes`
  names the descendants that stayed away, and the GUI surfaces that in the
  success notice — worth a step in the journey.

**2026-09-23, from [[KAIROS-T-0157]].**

- **A seventh stale "archived = gone" assertion**, already fixed:
  `uat/journeys/operations.journey.ts` asserted `404` on GET for every
  cascaded child, stale since T-0154. Rewritten to `200` + `archived_at`.
  Its note about the `--include-deleted` + `--query` no-op is also corrected
  — that combination now works.
- **A tidy-up for this task**: `archived_marker()` exists in
  `mcp/tools.rs` (T-0157), and T-0158 landed two inline copies of the same
  three lines in the same file concurrently. Collapse them.
- MCP tool count is **17 → 17** after T-0157 (`include_deleted` is a new
  argument on an existing tool, not a new tool). `restore_item`
  ([[KAIROS-T-0160]]) is still the 18th the drift gate is waiting on, so the
  gate reads **17/18** until a journey exercises restore.

## Operational hazards for whoever runs this close-out

**The shared working tree bites in two ways**, both learned during this
initiative:

1. **`git add` does not isolate concurrent agents.** The git index is
   genuinely shared — another agent's staged files can land in your commit.
   It happened twice here. Commit through a private index:
   `GIT_INDEX_FILE=/tmp/x.index git add <paths> && GIT_INDEX_FILE=/tmp/x.index git commit …`
2. **`angreal test integration` and `angreal test uat` tear the compose
   stack down including the volume**, which kills any other run in flight
   (once producing 32 spurious "connection refused" failures). For a
   close-out this is fine — it *should* be the only thing running — but
   confirm no agents are live first.

**2026-09-23, from [[KAIROS-T-0163]] — selectors and copy a journey can assert on.**

| Thing | Selector / text |
|---|---|
| Search toggle | `[data-testid="include-put-away"]`, contains `Include work that has been put away`; on-state is `.cl-switch--on` inside it |
| Put-away search row | `tr.kairos-search__hit--put-away` |
| Put-away badge (shared) | `.kairos-archived-badge` → text `put away` — **no longer unique per page**, always scope it |
| Group caption | `.cl-panel__caption` contains `put away` (e.g. `3 on this page · 1 put away`) |
| Archived graph node | `.kairos-graph__node--put-away`, child `.kairos-graph__put-away` text `put away` |
| Containment/progress line | Relationships panel contains `containment is a fact about the record` |
| Text query input | `.cl-field` with label text `Text query` → `input` (aurora labels are not `for`-associated) |
| Search submit | `getByRole('button', { name: 'Search', exact: true })` |

`e2e/tests/archived.spec.ts` is now 5 specs. The **search → read → audit
path is end-to-end**: with the toggle on, a put-away hit links to
`/items/:code` and renders T-0164's banner. That is the arc a UAT journey
should tell as a story rather than re-assert mechanically.

A journey probe wanting a findable hit should use a **fresh random word per
run**, as the e2e spec does, so the result stays on page 1 however often the
stack is re-driven.
