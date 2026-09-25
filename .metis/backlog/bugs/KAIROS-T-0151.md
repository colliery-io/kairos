---
id: archived-work-loses-its-content-a
level: task
title: "Archived work loses its content: a soft-deleted item and its history both 404"
short_code: "KAIROS-T-0151"
created_at: 2026-09-23T10:36:28.343050+00:00
updated_at: 2026-09-23T10:36:28.343050+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"
  - "#bug"


exit_criteria_met: false
initiative_id: NULL
---
# Archived work loses its content: a soft-deleted item and its history both 404

## Objective

Archiving a card is a soft delete (KAIROS-I-0012, Dylan: *"all cards must be archived or moved to delete a team"*). The rationale said history and activity survive. Only the activity log does.

Measured 2026-09-23 against the compose stack, as alice (org admin):

```
GET /api/tasks/DEMO-T-0037           -> 404   (archived)
GET /api/tasks/DEMO-T-0037/history   -> 404   (archived)
GET /api/tasks/DEMO-T-0001           -> 200   (live)
GET /api/tasks/DEMO-T-0001/history   -> 200   (live)
GET /api/activity?limit=200          -> still contains 2 rows naming DEMO-T-0037
```

So once work is put away, "what did that ticket say?" has no answer through any surface. The `item_history` rows are still in the database until the retention sweeper prunes them (A-0004) — nothing serves them.

### Type
- [x] Bug - Production issue that needs fixing

### Impact Assessment

- **Affected Users**: anyone auditing closed work, which is the case archiving exists to serve. A quarter closed the way `uat/journeys/housekeeping.journey.ts` closes one leaves only "DEMO-T-0037 was created, then deleted, by alice".
- **Expected vs Actual**: I-0012 assumed a deleted card keeps its history. It keeps its activity lines; its content and versions are unreachable.

## Implementation Notes

The handlers filter `deleted_at IS NULL` when resolving a short code, so the 404 comes from resolution, not from the history query. Options, roughly in order of cost:

1. **Serve history for archived items to org admins** — smallest change: let the history route resolve soft-deleted items (the read is already admin-shaped), leaving the item route alone.
2. **`?include_deleted=true`** on the item and history routes — consistent with the search filter that already exists, and explicit at the call site.
3. **Accept it and say so** — if "archived means gone from the product, the audit trail is the record", then say that in the A-0004/I-0012 docs and in the GUI's delete confirmation, so nobody plans an audit around content that is not there.

I would not pick (3) silently: the decision was argued on the basis that nothing is destroyed, and a reader of that ADR would expect to be able to read the thing.

## Acceptance Criteria

- [x] A decision recorded: **serve it** — see [[KAIROS-A-0020]].
- [ ] An archived item is retrievable by short code, marked as archived, on every surface (API, MCP, CLI, GUI).
- [ ] Its `/history` is retrievable on the same terms.
- [ ] Default listings are unchanged — boards, queues and directories still hide it.
- [ ] No permission gate: whoever could read the work before can read it after.
- [ ] `uat/journeys/housekeeping.journey.ts` step 3 flips from asserting 404 to asserting the content is retrievable, and its narration and header comment are rewritten.

## Status Updates

**2026-09-23** — Found by the `housekeeping` UAT journey (KAIROS-T-0148), which was written to prove the I-0012 archiving decision holds end to end. The journey asserts today's behaviour and names this ticket, so it will need editing when this is resolved either way.

**2026-09-23 — decided.** Dylan: *"Archived items should still be visible /
grabbable for audit and search. Archived just is intended to say this is old
and we're by default limiting its visibility; nothing more."* Recorded as
**[[KAIROS-A-0020]]**, which also rules out the admin-only option that was on
the table — archiving is a visibility default, not a permission boundary.
[[KAIROS-T-0152]] is the same decision's other half and should be implemented
with this, not after it.

**2026-09-23 — fixed.** Closed by [[KAIROS-I-0015]], implementing
[[KAIROS-A-0020]].

- [[KAIROS-T-0154]] (`7517465`) — `GET /api/{family}/{short_code}` and
  `/history` serve archived work, marked with `archived_at`.
- [[KAIROS-T-0155]] (`2582fa3`) — MCP `get_item` / `get_history` and the CLI
  do the same, with a banner.
- [[KAIROS-T-0164]] (`9a3519b`) — `/items/:code` and
  `/activity/history/:code` render it in the browser, verified for all five
  families in the e2e GUI leg.

The journey that found it (`uat/journeys/housekeeping.journey.ts`) has been
rewritten from asserting 404 to asserting the audit answer.

Note the original finding was half right in a way worth keeping: the
activity trail was never the *only* thing that survived — the rows were
always in the database, and `item_history` was never liveness-filtered at
all. Nothing served them. The fix was resolution, not storage.
