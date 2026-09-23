---
id: split-scim-md-the-page-assumed
level: task
title: "Split scim.md: the page assumed single-mode is a how-to wearing reference clothes"
short_code: "KAIROS-T-0179"
created_at: 2026-09-23T23:01:57.333861+00:00
updated_at: 2026-09-23T23:01:57.333861+00:00
parent: KAIROS-I-0016
blocked_by: [KAIROS-T-0167]
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

Split `docs/src/reference/scim.md` into a how-to and a pure reference
remainder. Measured mode distribution: **~65% reference, ~20% how-to, ~15%
explanation.**

## Why this task exists: the initiative was wrong about this page

[[KAIROS-I-0016]] D3 says, of `docs/api/scim.md` and `docs/api/events.md`:

> move to `reference/` **as-is** — they are already single-mode and correct,
> which is why they are the only existing pages that need no
> reclassification.

That was my assumption when writing the plan, and it does not hold. `scim.md`
has a `## Setup (org admin)` section that is a **numbered imperative procedure
with per-IdP conditionals** — textbook how-to inside a page declared as
reference, violating R2 outright and matching S-0008 §4.3 ("reference that
instructs"). [[KAIROS-T-0169]] was told to resist improving those pages, which
was the right instruction, so it reported the finding rather than acting on it.

The lesson worth keeping: **"this page is fine" is a claim, and the only two
pages in the plan exempted from review were the two nobody had reviewed.**
Exemptions granted on the strength of a glance are where mode-mixing survives
a documentation project.

## Implementation Notes

**Blocked by [[KAIROS-T-0167]].** Read `plugin/references/diataxis.md` §4.3
(reference that instructs) and §2.2 (H1–H6) — the general rule in §4 applies:
never fix mode-mixing by deleting content; relocate it.

- **`how-to/provision-users-with-scim.md`** — the setup procedure, under
  `## For operators` in `SUMMARY.md`. Keep the per-IdP conditionals: H4 wants
  legitimate variation handled with explicit conditionals, so what made the
  section wrong *as reference* is exactly what makes it right *as a how-to*.
- **`reference/scim.md`** — the remainder: endpoints, schema mappings,
  filters, the SCIM error envelope. No steps, no "you should".
- The ~15% explanation is small enough to reduce to an orienting sentence
  plus a link, rather than a new page. Judge when you see it.

**`events.md` is also mildly mixed but cosmetically so** — leave it, and say
in the Status Update that it was assessed and left deliberately, so the next
reviewer does not re-litigate it.

## Acceptance Criteria

- [ ] `how-to/provision-users-with-scim.md` exists, listed under
      `## For operators`, and keeps the per-IdP conditionals.
- [ ] `reference/scim.md` is reference only — no numbered procedure, no
      second person.
- [ ] Nothing from the original page is lost; content relocated, not deleted.
- [ ] Inbound links to `reference/scim.md` still resolve (the two Rust doc
      comments [[KAIROS-T-0169]] updated, plus anything in the book).
- [ ] `events.md` assessed and the decision to leave it recorded.
- [ ] `diataxis-review` passes on both pages — **independent review, not
      self-review** (see [[KAIROS-T-0169]]'s finding below).
- [ ] `angreal docs build` clean.

## Status Updates

*To be added during implementation*
