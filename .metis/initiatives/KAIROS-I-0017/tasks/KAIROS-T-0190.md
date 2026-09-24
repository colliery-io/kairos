---
id: chunking-the-composed-primary
level: task
title: "Chunking, the composed primary vector, incremental re-embedding and backfill"
short_code: "KAIROS-T-0190"
created_at: 2026-09-24T02:27:55.747474+00:00
updated_at: 2026-09-24T02:27:55.747474+00:00
parent: KAIROS-I-0017
blocked_by: [KAIROS-T-0187, KAIROS-T-0189]
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

Fill the tables. Split item text into chunks on heading boundaries, compose the
primary vector, re-embed incrementally when an item changes, and backfill
existing tenants. [[KAIROS-A-0021]] rules 3 and 4.

## Implementation Notes

### Chunking (rule 3)

Headings are **anchors, not labels**. The measurements are the whole argument:
across 231 Metis documents every single one has headings, the median has 8
sections, and 94% of sections are under 2,000 characters — so heading boundaries
produce well-sized chunks reliably. But there are **538 distinct heading strings
and 84% appear exactly once**, so nothing may key on a section being called
anything in particular. A citation echoes the literal heading text it found; it
does not recognise it.

- split on markdown headings of any level
- a section over the size ceiling is split again by sliding window
- text with no headings at all gets sliding-window chunking with overlap
- record ordinal, literal heading text (nullable), and character range, so a
  citation can point at a location in the document the reader actually has

### The primary vector (rule 4)

Composed by the system from what is always present: title, entity type,
repository, owning team, parent's title, stamped metadata, and the opening prose.
All structural, none template-dependent, immune to the drift that killed the
named-section design.

Stamped metadata carries real weight here. Where section names cannot be trusted,
metadata definitions are the one place in the product where meaning is
**explicitly labelled** by a tenant that chose the label.

### Incremental re-embedding

This is a cost model as much as a correctness one. Metis instructs agents to
update Status Updates every few tool calls; the product's own agent loop does the
same. An append must re-embed **one chunk**, not a 12 KB document. The content
hash from [[KAIROS-T-0187]] is the mechanism: chunk, hash each chunk, re-embed
only the chunks whose hash changed, and touch the primary vector only when one of
its composed inputs changed.

Embedding happens **off the write path** — queued, not synchronous. A `create_item`
must not wait on a model, and a failed embedding must leave the item created and
retrievable lexically.

### Backfill

Existing tenants have items and empty tables. A backfill walks them, in batches,
resumable, reporting progress, and driven through `angreal` like every other
operation. It must be safe to run twice, and safe to run while the server serves
traffic.

The vector index deferred by T-0187 gets created here, on populated tables, with
its parameters and the measured build time recorded.

### Re-measure the self-similarity question

The earlier vocabulary-overlap measurement (median pairwise Jaccard 0.12) said
agent-authored documents are less self-similar than feared. That was a lexical
proxy. Once real embeddings exist, measure the actual pairwise cosine
distribution over the seeded tenant and record it — **before** [[KAIROS-T-0191]]
picks any threshold. A threshold chosen without that number is a guess.

### Dependencies

[[KAIROS-T-0187]] for the tables, [[KAIROS-T-0189]] for the provider.

### Risk Considerations

- Chunk boundaries shifting on every edit would re-embed everything. Anchor
  ranges to headings so an edit inside one section leaves the others' hashes
  unchanged, and test exactly that.
- A queue that loses work silently is worse than a synchronous one. Un-embedded
  and stale items must be **countable**, so an operator can see how far behind
  retrieval is.
- Backfill on a large tenant is a sustained load on the embedding provider.
  Batch size and rate must be settable.

## Acceptance Criteria

- [ ] Heading-boundary chunking with sliding-window fallback, storing ordinal,
      literal heading text and character range
- [ ] No code path keys on a heading's name
- [ ] The primary vector is composed from title, type, repository, team, parent
      title, stamped metadata and opening prose
- [ ] Appending to one section re-embeds one chunk, proven by a test
- [ ] Embedding is off the write path; a provider failure leaves the item created
      and lexically retrievable
- [ ] `angreal` drives a resumable, idempotent, rate-limitable backfill
- [ ] Un-embedded and stale counts are observable
- [ ] The vector index is created on populated tables, with parameters and build
      time recorded
- [ ] The real pairwise cosine distribution over the seeded tenant is measured
      and recorded in the Status Updates
- [ ] `angreal test` green

## Status Updates

*To be added during implementation*
