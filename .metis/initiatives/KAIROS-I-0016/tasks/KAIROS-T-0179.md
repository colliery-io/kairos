---
id: split-scim-md-the-page-assumed
level: task
title: "Split scim.md: the page assumed single-mode is a how-to wearing reference clothes"
short_code: "KAIROS-T-0179"
created_at: 2026-09-23T23:01:57.333861+00:00
updated_at: 2026-09-23T23:28:39.900113+00:00
parent: KAIROS-I-0016
blocked_by: [KAIROS-T-0167]
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
**2026-09-23 — done.**

### The split

- **`how-to/provision-users-with-scim.md`** (new, under `## For operators`) —
  mint a token, point the IdP at Kairos, make the identity join work, push users
  before groups, verify, and the limits to expect.
- **`reference/scim.md`** — the facts that remain: token format and tenant
  resolution, the mapping table, the resource model, groups, the supported
  RFC 7643/7644 subset, the error envelope.

Two things moved rather than being deleted, per S-0008 §4's rule:

- The numbered `## Setup (org admin)` procedure, which was the R2 violation.
- The per-IdP guidance — *"Configure the IdP to send the OIDC `sub` as
  `externalId`"*, with the Okta and Entra ID notes. This is the interesting
  half: as reference it was instruction in the wrong mode, and as a how-to it
  is **exactly what H4 asks for** — legitimate variation handled with explicit
  conditionals for a working reader. The same text is a defect in one mode and
  the best part of the page in the other.

The old `## Purpose` (the ~15% explanation) became a four-line `## Scope`
statement plus a link, which is the applicability statement R6 wants rather
than a new explanation page — the reasoning about *why* Kairos ships no IdP
belongs to A-0016 and is not worth a page of its own here.

### `events.md` assessed and deliberately left

Checked for numbered procedures, imperatives and second person: **zero
matches**. The mild mixing [[KAIROS-T-0169]] noted is cosmetic, so it stays as
it is. Recorded here so the next reviewer does not re-litigate it.

### Links

All inbound references to `reference/scim.md` still resolve — the two Rust doc
comments T-0169 updated, `SUMMARY.md`, and three how-to pages that link to it.

A link check across every page found **one real break, which was mine**:
`tutorials/run-kairos-locally.md` pointed at `deploy-to-kubernetes.md`, which
[[KAIROS-T-0174]] deferred to [[KAIROS-T-0181]]. Replaced with a link to
`how-to/install-with-helm.md` and a sentence saying the tutorial is waiting on
an ARM image — the honest route today. Every page link now resolves, and mdBook
created no stub for the deferred tutorial (its `SUMMARY.md` line is inside an
HTML comment, so mdBook does not see it).

**Review**: independent `diataxis-review` deferred to [[KAIROS-T-0175]] with the
other pages, per the initiative's standing finding that reference pages get an
independent review rather than a self-review.
