---
id: close-out-document-retrieval-in
level: task
title: "Close out: document retrieval in the book, a UAT journey, and the drift gate"
short_code: "KAIROS-T-0193"
created_at: 2026-09-24T02:28:04.847090+00:00
updated_at: 2026-09-24T02:28:04.847090+00:00
parent: KAIROS-I-0017
blocked_by: [KAIROS-T-0188, KAIROS-T-0191, KAIROS-T-0192]
archived: false

tags:
  - "#task"
  - "#phase/todo"


exit_criteria_met: false
initiative_id: KAIROS-I-0017
---

## Parent Initiative

[[KAIROS-I-0017]]

## Objective

Close the initiative honestly: the book learns about retrieval, a UAT journey
exercises it as a persona would, and the surface drift gate counts it.

[[KAIROS-I-0016]] established that **writing the documentation finds the defects**
— nine of them, that time. This task is where that happens for retrieval, and
finding bugs here is the expected outcome, not a failure of the earlier tasks.

## Implementation Notes

### The book

Diátaxis, per [[KAIROS-S-0008]], and each mode earns its page:

- **Explanation** — why retrieval is proposals rather than assertions, why
  similarity plus graph distance beats similarity alone, and why the graph being
  authored is what makes this work here. This is the page that matters most,
  because the feature is counter-intuitive: a good answer can look like a weak one.
- **How-to** — configure embeddings: local default, bring-your-own endpoint, what
  the dimension mismatch means, and how to run and monitor the backfill.
- **Reference** — the tool and endpoint, the claim types, what the *why* contains,
  and the bounds. Regenerated REST pages come from `scripts/render-openapi.py`;
  check whether the new endpoint lands in an existing group in `GROUPS` or needs
  a new one, and run `--check` so CI does not find the drift first.
- **Tutorial** — only if it earns its place. Recommendation: extend
  `docs/src/tutorials/run-kairos-locally.md` with a short retrieval moment
  against the seeded tenant rather than adding a third tutorial. The demo seed
  must actually contain two items a reader will find related, which is worth
  checking before promising it.

Screenshots, if any, go through `e2e/tests/capture-docs-images.spec.ts` so a
recapture stays mechanical.

### The UAT journey

**Extend [[KAIROS-T-0194]]'s `multi-repo-agent` journey rather than adding a
second one.** It already stands up an agent across three repositories, which is
the only context in which "find the work nobody linked" means anything — a
one-repo agent has nothing to cross. Adding a separate retrieval journey would
duplicate that setup and split the story in two.

The arc to add to it: the agent, about to start a ticket, asks what is related;
is shown prior art in completed work and an unlinked ticket that overlaps;
proposes an edge; a human confirms it. The journey already ends with the agent
reading its estate to choose next work, which is exactly where that ask belongs.

### The drift gate

`uat/README.md` documents a surface drift gate. Adding tools and endpoints moves
its denominator — it will read short until the journey above exercises them.
Make the numbers agree, and record what the gate reads before and after.

### Dependencies

[[KAIROS-T-0188]], [[KAIROS-T-0191]] and [[KAIROS-T-0192]] — the whole feature
has to exist before it can be documented truthfully.

### Risk Considerations

- Documenting a feature that does not quite work produces documentation that
  lies. If a page cannot be written honestly, **file the defect** and say so in
  the Status Updates — that is the mechanism working, exactly as in I-0016.
- `docs.yml` publishes on pushes touching `docs/**`, so these pages go live
  without a version bump. They must be true of the released version or clearly
  marked as unreleased.

## Acceptance Criteria

- [ ] Explanation, how-to and reference pages for retrieval, added to `SUMMARY.md`
- [ ] The REST reference is regenerated and `--check` passes
- [ ] The tutorial decision is made and recorded; if extended, the seeded tenant
      is verified to contain a genuinely related pair
- [ ] A UAT journey covers ask → prior art → propose → confirm, with a readable
      report
- [ ] The surface drift gate reads complete, and its before/after numbers are
      recorded
- [ ] Any defect found while writing is filed as a backlog task and named in the
      Status Updates
- [ ] The book builds via `angreal docs build`, and `angreal test` green

## Status Updates

*To be added during implementation*
