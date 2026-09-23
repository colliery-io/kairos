---
id: promote-kairos-s-0008-out-of
level: task
title: "Promote KAIROS-S-0008 out of discovery, so the gate is settled before it is used"
short_code: "KAIROS-T-0166"
created_at: 2026-09-23T22:11:02.981917+00:00
updated_at: 2026-09-23T22:11:02.981917+00:00
parent: KAIROS-I-0016
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0016
---

## Parent Initiative

[[KAIROS-I-0016]]

## Objective

Walk [[KAIROS-S-0008]] out of `discovery` so the standard every page is
reviewed against is settled before any page is written. Re-render
`plugin/references/diataxis.md` from it.

## Implementation Notes

The spec at `.metis/specifications/KAIROS-S-0008/specification.md` is already
substantial — the compass, per-mode contracts with citable rule IDs
(T1–T6 and the how-to / reference / explanation sets), classification
heuristics, and a mode-mixing anti-pattern catalogue. This is a review and a
phase transition, not a rewrite.

Read it against what the initiative is about to do and check three things:

1. **Are the rule IDs stable?** They are cited in every subsequent task's
   Status Update. If any are going to be renumbered, now is the moment.
2. **Does it cover the cases Kairos actually has?** In particular a page
   that is genuinely reference but whose subject is a *decision* (the
   archiving page is explanation about an ADR), and generated pages
   (`reference/rest-api.md` comes from OpenAPI — does R4's completeness rule
   read sensibly for generated content?).
3. **Does `plugin/references/diataxis.md` match the spec?** Its header says
   *"Rendered from KAIROS-S-0008 (source of truth) — do not edit here"*, so
   confirm it has not drifted, and re-render if it has.

Then transition the spec. Specification phases are adjacent-only, same as
the other document types, so it may take more than one hop.

## Acceptance Criteria

- [ ] `KAIROS-S-0008` is out of `discovery` and in a settled phase.
- [ ] Rule IDs are confirmed stable, or renumbered now with the change noted.
- [ ] `plugin/references/diataxis.md` matches the spec.
- [ ] Any gap found — generated reference, decision-as-explanation — is
      either addressed in the spec or recorded as a known limit.

## Status Updates

*To be added during implementation*
