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
  - "#phase/backlog"
  - "#bug"


exit_criteria_met: false
initiative_id: NULL

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

- [ ] A decision recorded in the I-0012 initiative or A-0004: serve it, gate it, or document the limit.
- [ ] If served: `uat/journeys/housekeeping.journey.ts` step 3 flips from asserting 404 to asserting the content is retrievable, and its narration and header comment are rewritten.

## Status Updates

**2026-09-23** — Found by the `housekeeping` UAT journey (KAIROS-T-0148), which was written to prove the I-0012 archiving decision holds end to end. The journey asserts today's behaviour and names this ticket, so it will need editing when this is resolved either way.
