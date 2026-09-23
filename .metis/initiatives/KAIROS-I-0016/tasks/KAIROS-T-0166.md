---
id: promote-kairos-s-0008-out-of
level: task
title: "Promote KAIROS-S-0008 out of discovery, so the gate is settled before it is used"
short_code: "KAIROS-T-0166"
created_at: 2026-09-23T22:11:02.981917+00:00
updated_at: 2026-09-23T22:25:18.276609+00:00
parent: KAIROS-I-0016
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/active"


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

## Acceptance Criteria

- [ ] `KAIROS-S-0008` is out of `discovery` and in a settled phase.
- [ ] Rule IDs are confirmed stable, or renumbered now with the change noted.
- [ ] `plugin/references/diataxis.md` matches the spec.
- [ ] Any gap found — generated reference, decision-as-explanation — is
      either addressed in the spec or recorded as a known limit.

## Status Updates

*To be added during implementation*
**2026-09-23 — done.** [[KAIROS-S-0008]] is `published`
(discovery → drafting → review → published).

### 1. Rule IDs are stable — no renumbering

30 IDs across four mode sets plus the structural set: **T1–T6, H1–H6,
R1–R6, E1–E6, S1–S6**. All are referenced consistently in §6's finding
format, and none is duplicated or orphaned. Subsequent tasks can cite them
without risk of a later shift.

### 2. Both questions landed differently than the brief expected

**Generated reference is not in tension with R4 — it is in tension with R1.**
The initiative (D5) and [[KAIROS-T-0170]]'s brief both assumed R4
(completeness) was the rule a generated page would strain against. It is the
opposite: generation satisfies R3 (consistent format), R4 and R6 (accurate
and current) *better than a human can*, precisely because it is mechanical.
The rule it fails is **R1** — "structure mirrors the structure of the product
itself, so location is predictable". A generator emits its source's ordering,
which is rarely the product's own shape, so the page comes out complete and
unnavigable. Added a note to §2.3 saying so, and directing a reviewer to
check R1 first on a generated page. **T-0170 should read that rather than its
own brief on this point.**

**Explanation-about-a-decision needed no accommodation — the mode already
fits it unusually well.** E2 wants rationale and E5 wants alternatives marked
as such, which is exactly what an ADR carries. Added a §2.4 note recording
two cautions rather than any rule change: E6 still binds, so the field names,
defaults and error codes a decision produced belong in reference with the
explanation citing them (a page that is the only home for both argument and
facts has quietly become reference too); and E1 still applies, so the page is
named for the topic a reader is thinking about, not for the decision's
identifier. **Relevant to [[KAIROS-T-0171]]'s `archiving.md`**: it explains
ADR-20's rules, and `reference/glossary.md` / `reference/rest-api.md` hold
`archived_at`, `include_deleted` and `RESTORE_BLOCKED`.

### 3. The rendered copy had not drifted

`plugin/references/diataxis.md` was byte-identical to the spec body apart
from the one intro sentence each carries pointing at the other — which is by
design. Re-rendered through `scripts/render-references.sh` (the mechanism I
had to go looking for: the copy is generated, not hand-maintained, so the
spec is the only file to edit).
