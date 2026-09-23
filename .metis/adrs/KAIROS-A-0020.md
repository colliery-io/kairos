---
id: 001-archived-means-hidden-by-default
level: adr
title: "Archived means hidden by default, not gone"
number: 1
short_code: "KAIROS-A-0020"
created_at: 2026-09-23T11:16:52.496751+00:00
updated_at: 2026-09-23T11:18:17.499558+00:00
decision_date: 
decision_maker: Dylan Storey
parent: 
archived: false

tags:
  - "#adr"
  - "#phase/decided"


exit_criteria_met: false
initiative_id: NULL
---

# ADR-20: Archived means hidden by default, not gone

## Context

Kairos has one deletion: a soft delete that stamps `deleted_at`. Two names
have been used for it — "delete" on the surfaces, "archive" in
KAIROS-I-0012's rule that a team may be wound down once its board holds no
live cards — and the product has never said which one it means.

The UAT arc (KAIROS-I-0014) walked the question and found the answer is
currently "gone", not "hidden":

- `GET /api/tasks/{code}` and `GET /api/tasks/{code}/history` both **404**
  once an item is archived. The rows are still in the database, but nothing
  serves them. "What did that ticket say?" has no answer (KAIROS-T-0151).
- Its metadata values are equally unreachable — `set_metadata` answers
  `no live item`, and no route clears them — **yet they are still counted**
  by the `DEFINITION_IN_USE` guard, so one archived card makes a custom
  field permanently unretirable (KAIROS-T-0152).
- Only the activity trail survives, and it is thinner by design: it records
  *that* work existed and who touched it, never what it said.

So today archiving destroys the audit answer while keeping the audit
question. The two filed bugs are not independent defects; they are both
consequences of never having decided what the state means.

## Decision

**Archived means: this is old, so it is hidden by default. Nothing more.**

Dylan Storey, 2026-09-23: *"Archived items should still be visible /
grabbable for audit and search. Archived just is intended to say this is old
and we're by default limiting its visibility; nothing more."*

Concretely, and these are the rules the implementation must satisfy:

1. **Archived content stays retrievable.** Fetching an archived item by
   short code returns it, marked as archived. So does its version history.
   No 404 for something that exists.
2. **Archived content stays searchable** when asked for explicitly, and that
   must compose with every other filter — including a text query, which
   today it cannot.
3. **Default listings hide it.** Boards, queues, directories and search
   without an explicit opt-in behave exactly as they do now. This is the
   entire user-visible purpose of the state.
4. **Archiving is not a permission boundary.** Whoever could read the work
   before can read it after. No admin-only gate, no separate capability —
   that would be "more than nothing more".
5. **Archived work is not live work.** Guards that ask "is there still work
   here?" — winding down a team, retiring a repository — continue to count
   live rows only. Archiving a board clear is still how you wind a team
   down (KAIROS-I-0012 stands unchanged).
6. **Archived work is still content.** A guard that asks "does anything
   still refer to this definition?" may legitimately count an archived
   carrier, *provided* the refusal names the carriers and the user can then
   reach them and clear the reference. Rule 1 is what makes that honest;
   without it, counting archived rows is a trap.

## Rationale

The point of a system of record is that the record outlives the work. An
organisation asks "what did that ticket say?" precisely about work that is
finished, and finished work is exactly what gets archived — so the current
behaviour loses the content at the moment it becomes audit material.

Framing the state as a *visibility default* rather than a lifecycle also
keeps it cheap. There is no new state machine, no new capability, no
migration of meaning: the `deleted_at` column already carries it. What
changes is who is allowed to ask past it, and the answer is "anyone who
could see it before, if they say so".

It also resolves T-0152 without a policy argument. Once archived items are
reachable, an unretirable field stops being a trap: the admin is refused,
told which archived work carries the field, and can go and clear it. The
guard did not need to change — the visibility did.

## Alternatives Analysis

- **Serve archived content to admins only.** Rejected by the decision
  maker: it makes archiving a permission boundary, which is more than a
  visibility default, and it breaks audit for exactly the people —
  engineers, team leads — who ask the question most.
- **A separate Archived state alongside soft delete.** Rejected: two
  concepts where one will do, plus a migration and a second guard to keep
  in sync with the first. KAIROS-I-0012 deliberately made archiving *be*
  the soft delete.
- **Document the limitation and move on.** Rejected: it leaves a system of
  record that cannot answer a question about its own records, and leaves
  T-0152's trap in place with no way out of it.
- **Hard delete on archive.** Never seriously considered — it would make the
  retention sweeper and copy-forward history (KAIROS-A-0004) meaningless —
  but worth recording, because the current 404 behaviour is
  indistinguishable from it to every caller.

## Consequences

### Positive

- The audit question has an audit answer. `/history` on archived work is
  the strongest version of this: copy-forward history (KAIROS-A-0004) was
  built to reconstruct what a record said at a point in time, and today it
  goes dark exactly when that matters.
- KAIROS-T-0152's trap disappears as a side effect rather than as a second
  project.
- The `--include-deleted` / `--query` incompatibility becomes a bug with an
  owner instead of a curiosity found by a journey.

### Negative

- Every list query in the product now has two modes, and the default is the
  one that must stay fast. Wherever `include_archived` is plumbed, the
  archived branch needs its own index consideration.
- Archived items becoming reachable means they become reachable *by every
  surface* — GUI, API, MCP, CLI. Partial implementation would be worse than
  none, because an auditor who finds the answer on one surface will
  reasonably assume the others agree.
- The retention sweeper was expected to be the backstop that eventually
  destroys content. It is not: `spawn_retention_loop` / `sweep_all_tenants`
  / `sweep_tenant` have **zero references in `crates/kairos-server/`** (only
  the db tests call them), and even when wired it purges `item_history` and
  `activity_log` rows only — never the soft-deleted entity rows themselves.
  So today *nothing* ever destroys archived content, and this decision makes
  that permanent-by-default rather than permanent-by-accident. Wiring the
  sweeper is still an open M2 task and now carries the erasure story too.

### Neutral

- The word "delete" on the surfaces is now actively misleading. Renaming it
  to "archive" is a separate, larger change (wire compatibility, GUI copy,
  CLI nouns) and is explicitly not part of this decision.
- **"Archived" is already taken.** The document editorial lifecycle
  (KAIROS-T-0078) is `draft | review | published | archived`, and that
  `archived` is unrelated to `deleted_at` — a published document can be
  editorially archived while remaining perfectly live. `kairos-web`'s
  `pages/item.rs` uses the word in that sense today. Whatever this state is
  called on the surfaces, the two must not collide; the safest reading is
  that this ADR names a *behaviour*, and the user-facing noun is still open.

## Review Triggers

- A tenant asks for archived work to be genuinely unreachable — a
  regulatory erasure requirement would sit against rule 4 and force a real
  hard delete rather than a visibility flag.
- Default-listing performance regresses once the two-mode queries land.