---
id: the-archived-item-page-read-it
level: task
title: "The archived item page: read it, read its history, restore it"
short_code: "KAIROS-T-0164"
created_at: 2026-09-23T11:30:07.479107+00:00
updated_at: 2026-09-23T11:30:07.479107+00:00
parent: KAIROS-I-0015
blocked_by: [KAIROS-T-0154, KAIROS-T-0160]
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

The screen this whole initiative exists for: open a piece of archived work,
read what it said, read its history, and put it back. Closes
[[KAIROS-T-0151]].

## Implementation Notes

**Blocked by [[KAIROS-T-0154]] and [[KAIROS-T-0160]].**

Routes are in `crates/kairos-web/src/app.rs:55-80`. Both relevant pages
consume API routes that 404 on archived work today, so both render an error:

- `/items/:code` — the item page.
- `/activity/history/:code` — **the audit answer**, and the single most
  valuable screen here. Copy-forward history (KAIROS-A-0004) was built to
  reconstruct what a record said at a point in time, and it currently goes
  dark exactly when that matters.

Needed:

- an unmistakable banner on an archived item — when it was put away and by
  whom (the activity trail has the actor);
- the **Restore** action, wired to [[KAIROS-T-0160]]'s endpoint, shown only
  to someone who holds the capability, and surfacing its refusals (missing
  board / column / team / repository) as a readable message naming what is
  gone rather than a bare error;
- write affordances hidden or disabled, since archived work is read-only —
  disabled with an explanation beats absent, so the page does not look
  broken;
- history renders normally, since the rows were always intact.

No archive/trash *route* is required — archived work is reached by short
code and by search ([[KAIROS-T-0163]]), which matches "hidden by default"
better than a recycle-bin page would. Note this decision on the task so it
is not re-litigated.

The same vocabulary hazard as T-0163: `item.rs:269,315` uses "archived" for
the document editorial lifecycle. On the item page both words can appear at
once — an editorially-archived document that is also put away. They must
read as different things.

## Acceptance Criteria

- [ ] `/items/:code` renders an archived item with a clear banner instead of
      an error, for all five families.
- [ ] `/activity/history/:code` renders an archived item's history.
- [ ] Restore is present for a capable user, absent otherwise, and its
      refusals name what is missing.
- [ ] Write affordances are visibly disabled, not silently broken.
- [ ] The two senses of "archived" are distinguishable on one screen.
- [ ] [[KAIROS-T-0151]] can be closed.
- [ ] `angreal test` green; `angreal test e2e` green.

## Status Updates

*To be added during implementation*
