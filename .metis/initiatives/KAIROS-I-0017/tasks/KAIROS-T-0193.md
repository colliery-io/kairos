---
id: close-out-document-retrieval-in
level: task
title: "Close out: document retrieval in the book, a UAT journey, and the drift gate"
short_code: "KAIROS-T-0193"
created_at: 2026-09-24T02:28:04.847090+00:00
updated_at: 2026-09-24T22:09:43.459533+00:00
parent: KAIROS-I-0017
blocked_by: [KAIROS-T-0188, KAIROS-T-0191, KAIROS-T-0192]
archived: false

tags:
  - "#task"
  - "#phase/completed"


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

## Acceptance Criteria

## Acceptance Criteria

- [x] Explanation, how-to and reference pages for retrieval, added to `SUMMARY.md`
- [x] The REST reference is regenerated and `--check` passes
- [x] The tutorial decision is made and recorded — **no tutorial page**, because
      retrieval has no CLI or GUI surface to demonstrate it with, filed as
      [[KAIROS-T-0195]]. The seeded tenant *was* verified to contain genuinely
      related pairs, which is why the gap is the surface and not the data
- [x] A UAT journey covers ask → prior art → propose → confirm, with a readable
      report
- [x] The surface drift gate reads complete, and its before/after numbers are
      recorded
- [x] Any defect found while writing is filed as a backlog task and named in the
      Status Updates
- [x] The book builds via `angreal docs build`, and `angreal test` green

## Status Updates

*To be added during implementation*

## Status Updates

### 2026-09-24 — done, and writing it found a defect and a gap

[[KAIROS-I-0016]] established that writing the documentation finds the defects.
It did again: one real bug that no smaller run could have exposed, and one product
gap that only the tutorial could have revealed.

### The bug: a sweep that never reached its second page

Running the **whole** UAT suite — not the filtered single journey — the retrieval
step timed out waiting for vectors. The cause:

`pending_primary` returned the first page ordered by `short_code`, and the
background sweep called it with no offset. So every tick asked for *the same*
page. Once those items were current the sweep reported "nothing to do" for ever,
and **a tenant's twenty-sixth item was never embedded at all.**

The backfill had the same shape of bug: it stopped as soon as one page needed no
work, which says nothing about the pages after it.

It survived every earlier test because **every earlier fixture was smaller than
one page**. Only running 22 journeys against one tenant created enough work to
get past it.

Fixed in three parts, and each part is load-bearing:

- `pending_primary` takes an `offset`, and orders **never-embedded rows first**.
  New work is picked up promptly rather than waiting for a cursor to come round.
- The refresher keeps a per-tenant cursor, advances it, and **wraps** — so an edit
  anywhere in the tenant is seen on the next time round rather than never. The
  cursor lives in memory on purpose: it is a fairness hint, not state, and losing
  it on restart costs one extra pass over an already-current page.
- The backfill does a **full paged scan, repeated while anything changes**, which
  terminates and also catches staleness that reordering shifted past.

Regression test: `a_sweep_reaches_items_beyond_the_first_page` builds three pages'
worth and asserts both that repeated offset-0 passes converge and that a third
page is disjoint from the first.

### The gap: retrieval has no human surface

The Diátaxis tutorial step **could not be written**, and that is the finding
rather than an obstacle to route around. Every other feature in
`run-kairos-locally.md` is shown with the CLI or the browser. Retrieval is MCP and
REST only: there is no `kairos related …` verb, and the GUI can *act on* proposals
an agent made but cannot *ask* the question that produces them.

So a person can only ever react to what an agent thought to look at. "Didn't we
try this?" is at least as much a human question — it is the one the initiative is
named after.

Filed as [[KAIROS-T-0195]] rather than papered over with a `curl` step, which
would not have been a tutorial step but an apology. **Decision recorded: no
tutorial page in this task**, because a tutorial that demonstrates a feature
through a hand-extracted bearer token teaches the wrong thing about the product.

### The book

Three pages, each earning its mode:

- **`explanation/finding-related-work.md`** — the one that matters most, because
  the feature is counter-intuitive: *a good answer here often looks like a weak
  one*. It gives the measurement rather than asserting the design — related pairs
  average 0.80 similarity, unrelated pairs reach 0.82, so there is no number you
  could put between them and about half the strongest matches are wrong. That is
  why everything says "possible", and saying so is more convincing than saying
  "we chose to be cautious".
- **`how-to/configure-retrieval.md`** — it already works; fill in existing work;
  build the index; use a hosted model; turn it off; and a
  **When it answers from text only** section with the three real causes in the
  order worth checking.
- **`reference/mcp-tools.md`** — `related_work` and `propose_edge`, with the three
  claim types tabulated and an explicit note that there is deliberately **no**
  confirm tool.

The REST endpoints landed together in the generated *Across any work item* page,
which is the right home — they are addressed by short code and work across all
five families.

### The journey

`multi-repo-agent` gained the arc, rather than a second journey: a one-repo agent
has nothing to cross, so this is the only place "find the work nobody linked"
means anything.

Four steps — the agent asks before starting and gets proposals with wording
asserted (`PROPOSALS, not findings`); proposes an edge and is told it is **not an
edge yet**; alice sees the agent's own words on the card and confirms, after which
the relationship exists; and the agent is refused when it tries to decide,
asserted twice over — **no confirm tool exists** for it to reach for, and the REST
route answers **403** to its key.

Two fixes the journey forced:

- The UAT server ran with no model cache, so `related_work` answered *"not enabled
  on this deployment"* while every assertion about proposals would have passed for
  the wrong reason. The test server now runs the **real local model**; downloading
  is allowed there and nowhere else, because it is a harness rather than a
  deployment.
- My first assertion scanned the whole response for the subject's short code and
  found it — **in the header**, which names the item asked about. The retrieval
  excludes the subject by id and always had; the assertion was wrong. Now it reads
  the proposal lines only.

And the refresher is set to two seconds rather than zero, deliberately: embedding
is off the write path, so a journey that created an item and expected an instant
vector would be testing a mechanism the product does not have. The journey waits,
as a real caller does.

### The drift gate

Before: two uncovered tools, carried as **pending** ALLOW entries naming this
ticket. After, with the journey exercising both:

```
## Surface coverage   ✅
MCP 20/20 tools, CLI 16/16 nouns, 0 allow-listed.
```

The allow-list is empty again, which is the state it should be in.

### Gates

fmt, clippy, `angreal web lint`, unit, integration **47 targets**, e2e 16, uat 22
journeys + the drift gate, the REST drift check, and `angreal docs build` — all
green.