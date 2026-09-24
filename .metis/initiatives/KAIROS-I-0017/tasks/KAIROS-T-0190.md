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

### 2026-09-23 — the measurement, across 19 Metis corpora

Dylan asked for a wide sample rather than Kairos alone. Extracted every Metis
document from all 19 repositories in `~/Desktop`, then embedded with the actual
candidate model from [[KAIROS-T-0189]] (`bge-small-en-v1.5`, 384 dim) and computed
**full pairwise** cosine — exact, not sampled.

**Corpus**, after excluding git worktree copies under `.claude/worktrees/` and
vendored `prior-art/.metis` trees, and deduplicating by content hash (those
copies contributed 3,774 phantom documents and flooded the first run with
cosine-1.000 self-pairs):

4,927 documents, 19 projects, 63,270 chunks, 25.9 MB of body text.

| level | n | p50 chars | p90 | max | over 2000 |
|---|---|---|---|---|---|
| task | 4,036 | 3,957 | 7,821 | 62,849 | 86% |
| initiative | 580 | 6,021 | 15,096 | 44,403 | 97% |
| adr | 175 | 5,442 | 10,870 | 28,272 | 96% |
| specification | 117 | 13,349 | 19,602 | 27,270 | 100% |
| vision | 19 | 6,593 | 14,722 | 15,443 | 94% |

#### Rule 3 is confirmed, more strongly than the Kairos-only sample showed

- **Zero** documents out of 4,927 have no headings. Not "few" — none.
- Median 9 sections per document; chunk p50 205 chars, p90 1,140, p99 1,685.
- **2.2%** of raw sections exceeded the 2,000-char ceiling and needed the
  sliding-window fallback. Heading boundaries handle 97.8% cleanly.

And the anchors-not-labels rule holds: 11,553 distinct heading strings across
63,270 headings, **85% appearing exactly once**, top ten covering only 33%. The
tenth most common heading is `Parent Initiative **[CONDITIONAL: Assigned Task]**`
with 1,439 occurrences — real documents across many projects that never deleted
the template marker. Nothing may key on a heading's name.

#### The cosine distributions, and the problem they expose

Composed primary vectors per rule 4 (level, project, title, parent title, opening
prose), classified by what the graph says about each pair:

| pair class | n | mean | p50 | p90 | p99 | max |
|---|---|---|---|---|---|---|
| different project | 4,000,000 | 0.594 | 0.595 | 0.654 | 0.708 | 0.892 |
| same project, no graph relation | 1,099,522 | 0.680 | 0.678 | 0.745 | 0.817 | 1.000 |
| same project, shared parent | 29,525 | 0.770 | 0.760 | 0.885 | 0.948 | 1.000 |
| direct parent/child edge | 3,627 | 0.800 | 0.809 | 0.877 | 0.916 | 0.961 |
| nearest neighbour, per document | 4,927 | 0.871 | 0.871 | 0.939 | 0.980 | 1.000 |

**No absolute threshold separates related from unrelated.** Pairs the graph says
are related average 0.800; pairs with no relation at all reach p99 0.817. Those
distributions overlap across their whole useful range. There is also no zero
point — the floor for two documents from *different projects* is 0.594, so
"0.68 similar" means nothing on its own.

This is the measurement [[KAIROS-T-0191]] must not ignore: a threshold sweep
cannot work, and anything that looks like `cosine > 0.8 ⇒ related` will be wrong
roughly as often as it is right. Ranking within a single query is meaningful;
comparing a score to a constant is not.

Cross-project 0.594 against same-project 0.680 does say the model picks up
project identity, which supports [[KAIROS-A-0019]]'s repository prior.

#### What survives at the extreme tail

Above 0.90 there are **270** unlinked same-project pairs out of 1,099,522 —
0.025%, a workably small candidate set. Inspected by hand:

**Genuine, and exactly what the initiative promises.** Ten pairs in `brokkr` at
cosine 1.000 are literal duplicate tickets filed twice under different short
codes (`BROKKR-T-0102`/`T-0089`, `T-0095`/`T-0100`, six more). `cloacina`'s
"Distribution strategy — CLI/daemon install script, server Docker image, Helm
chart" against "T-03: Helm chart for cloacina-server" at 0.949 is an implicit
dependency nobody drew. `muninn`'s ADR "Hook + MCP integration model" against its
initiative "Hook + MCP Integration Layer" at 0.938 is a missing ADR-to-initiative
link. `brokkr`'s "stop panicking on `pool.get()`" against "stop panicking on DB
pool exhaustion" at 0.957 is a near-duplicate in different words.

**And roughly half are noise.** `mimir`'s "Documentation Site" against "Groq
Provider" scores 0.940. "Quality Assurance and Developer Experience" against
"Character Creation and Management System" scores 0.967. `crt`'s "Architecture &
Maintainability Improvements" against "CRT Terminal Implementation" scores 0.950.

The false positives **cluster by level**: 27 of the 270 are initiative/initiative
and 19 are specification/specification, and those are the long, heavily templated,
generically titled documents whose primary vector is mostly boilerplate. Tasks,
which are shorter and more specific, behave far better.

So rule 4's composition needs a level-sensitive amendment, to design in this task:
for long documents the opening prose is the least discriminating input, not the
most, and title plus stamped metadata should dominate. Comparing chunk-to-chunk
rather than primary-to-primary is the other candidate, and is probably the better
answer for initiatives and specifications.

#### Hybrid is necessary, not a hedge

Of the 270 candidate pairs, 5% have identical titles and a further 17% have title
token overlap at or above 0.34 — lexical search finds those unaided. The
remaining 77% are where the vector earns its place, and that is also where the
false positives live. Lexical carries the duplicates; the vector carries the
paraphrases; neither carries both. [[KAIROS-A-0021]] rule 7 is confirmed on
evidence rather than on principle.

#### Reproduction

`scripts/corpus/` holds the extractor and the statistics; the embedding and
pairwise analysis were a throwaway crate recorded in [[KAIROS-T-0189]]. Both take
the corpus root as an argument, so the measurement can be re-run against any set
of repositories when the model or the composition changes.
