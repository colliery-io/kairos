---
id: close-out-both-full-runs-recorded
level: task
title: "Close out: both full runs recorded, READMEs list the arc, the drift gate still green"
short_code: "KAIROS-T-0149"
created_at: 2026-09-23T03:46:15.200199+00:00
updated_at: 2026-09-23T10:47:58.171230+00:00
parent: KAIROS-I-0014
blocked_by: [KAIROS-T-0137, KAIROS-T-0138, KAIROS-T-0139, KAIROS-T-0140, KAIROS-T-0141, KAIROS-T-0142, KAIROS-T-0143, KAIROS-T-0144, KAIROS-T-0145, KAIROS-T-0146, KAIROS-T-0147, KAIROS-T-0148]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0014
---

## Parent Initiative

[[KAIROS-I-0014]]

## Implementation Notes

- `uat/README.md`: list the journeys as the ARC (new user → mature org) rather than an unordered set, so a reader can see what stretch of an organisation's life each one covers.
- `README.md` "User acceptance runs": update the journey count and the `--journey` id list.
- Runs: `angreal test uat` (compose, all journeys + the gate) and `--server` against a kept stack; record run ids and the Surface coverage line in the initiative's progress log. Re-run `angreal test e2e` once, since journeys share helpers with it.
- If the suite has grown slow enough to be annoying, say so in the status update with the number — do not silently split it.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Both full runs green; run ids and the coverage line in the initiative log.
- [x] READMEs describe the arc; `angreal test e2e` 11/11.

## Status Updates

**2026-09-23 — closed out. All three tiers green, and the close-out found
one real leak that two earlier green runs had hidden.**

### Runs

| Run | Mode | Result |
|---|---|---|
| `mudz86xn` | compose, fresh seed | 20 journeys, 20 passed, 0 failed, 0 steps skipped |
| `mudz23wx` | `--server http://localhost:41080` | 20 journeys, 20 passed, 8 compose-only steps skipped |
| — | `angreal test e2e` | 10/10 API golden path + MCP, then 11 passed GUI smoke |

Surface coverage on the compose run: **MCP 17/17 tools, CLI 16/16 nouns, 0
allow-listed.** The `ALLOW` map is empty and the gate is measuring for real.
Under `--server` it skips and the report says `Not measured` rather than
going quiet, because a deployment run's compose-only steps are not
applicable. Suite runtime is ~32s for the journeys; nothing to split.

### What the close-out found

The first two full runs passed **while leaving residue in the tenant**, in a
report section nobody had read because it sits under the journey that caused
it rather than at the end:

- **A real leak.** `board-setup` stamps a metadata field on a card, then
  retires the card as part of its story. Teardown could never remove the
  definition afterwards: `409 DEFINITION_IN_USE`, one item value, belonging
  to an item that no longer exists. A throwaway probe established the leak
  is unrecoverable — `set_metadata` answers `no live item`, no route clears
  the value (`PUT …/metadata` is 405), and neither `?include_deleted=true`
  nor `?force=true` gets past the guard. **One deleted card makes a field
  immortal.** Filed as **KAIROS-T-0152**; the journey now clears the stamp
  before the delete, the only order that works, with a comment pointing at
  the ticket.
- **The noise that hid it.** Journeys that retire what they created — a card
  archived, a team wound down, a service account deleted — had every such
  delete reported as "left behind", because it 404s. `housekeeping` alone
  contributed five lines. `uat/run/ledger.ts` now treats a 404 as done
  (teardown's contract is "this is not on the deployment", and it is not),
  so the residue list contains only residue. `machine-access` dropped its
  hand-rolled version of the same rule.

Both fixes verified: run `mudz86xn` reports **no teardown residue at all**.

### Docs

`README.md` carries the 20-row arc table and the coverage claim;
`uat/README.md` gained "Coverage: the drift gate" and "The arc". Committed
earlier in `082cf63`.