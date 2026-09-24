---
id: chunking-the-composed-primary
level: task
title: "Chunking, the composed primary vector, incremental re-embedding and backfill"
short_code: "KAIROS-T-0190"
created_at: 2026-09-24T02:27:55.747474+00:00
updated_at: 2026-09-24T11:39:32.742673+00:00
parent: KAIROS-I-0017
blocked_by: [KAIROS-T-0187, KAIROS-T-0189]
archived: false

tags:
  - "#task"
  - "#phase/active"


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

### 2026-09-24 — the pure half: chunking and the composed primary vector

Docker was down (see [[KAIROS-T-0189]]), so this is the half of the task that
needs no database: `kairos_core::chunk` and `kairos_core::primary`. Both pure per
[[KAIROS-A-0009]]. The storage, incremental re-embedding and backfill still need
Postgres and are not started.

#### `chunk.rs` — heading boundaries as anchors

A faithful port of `scripts/corpus/extract.py`, the chunker the measurement was
taken with, so the figures quoted in its module docs describe **this** code.
Twenty unit tests, and the claim is verified rather than asserted:
`tests/corpus_agreement.rs` runs the Rust chunker over the same nineteen
repositories and checks the result.

Deliberate decisions, each with its own test:

- **Offsets are Unicode scalars, not bytes.** A byte offset is meaningless to a
  reader of a citation. The caveat is recorded: JavaScript indices are UTF-16, so
  a GUI highlighting astral-plane text must convert.
- **ATX headings only.** `---` is a thematic break and a front-matter fence at
  least as often as it is a setext underline, and guessing wrong splits a document
  at a horizontal rule.
- **`#1234 is the issue` is not a heading** — ATX requires the space. Without that
  rule, a line mentioning an issue number splits the document.
- **Empty sections are dropped.** A heading followed by a heading has nothing to
  embed, and an empty vector is worse than no vector: it is a row that matches
  everything equally badly.
- **A pathological overlap still terminates**, rather than allocating forever.

#### The verification test found a real drift, and it was mine

First run: 5,176 documents against Python's 4,927. Two causes, both in the
comparison rather than the chunker — my dedupe keyed on the raw body where
Python hashed the concatenated section texts, and my "windowed" count counted
extra windows where Python counted sections split.

Fixed, the second run gave **4,928 documents, 60,146 sections, 63,303 chunks,
1,309 windowed (2.2%), 0 documents with no headings** — against Python's 4,927 /
60,116 / 63,270 / 1,308 / 2.2% / 0.

One document of difference, because **this session wrote it**: the corpus is
nineteen live repositories and I had authored [[KAIROS-T-0194]] and appended
status updates to others since the figures were taken.

So the test does **not** assert exact counts, and must not: it would report
authorship as a chunker fault and be deleted within a week. It asserts what the
claims actually rest on — **zero** documents without headings, the window
fallback staying near 2.2%, a median of 8–10 sections, and chunks-per-section
barely above 1. Those survive drift.

#### `primary.rs` — and a hypothesis of mine that the corpus rejected

Composition is structural only: type, title, repository, team, parent title,
stamped metadata, bounded opening prose. Ten unit tests.

I expected to implement a **per-entity-type prose budget**. The earlier
measurement found false positives clustering at initiative level, and the
hypothesis was that long templated documents have boilerplate openings, so they
should get less prose. Rather than implement four numbers chosen by intuition, I
measured it — varying one budget across the corpus, scoring recall@1 on known
duplicate tickets against the count of unlinked same-project pairs above 0.93:

| prose chars | recall@1 | pairs ≥0.93 | of those, initiative-level |
|---|---|---|---|
| 0 | 94% | 324 | **102** |
| 150 | 80% | 279 | 33 |
| 300 | 86% | 296 | 38 |
| **600** | **86%** | **305** | **39** |
| 1200 | 86% | 340 | 41 |

**The hypothesis is contradicted.** Initiative-level borderline pairs are 102 with
no prose and 33–41 with it: prose is what *rescues* initiatives, and their generic
titles are what make them look alike. A per-type table cutting prose for
initiatives would have worsened precisely the problem it was meant to fix.

So: **one budget, 600 characters, no table.** Past 600 is strictly worse — 1,200
buys no recall and adds 35 borderline pairs — so the number sits where the curve
stops paying. 300 is within noise and equally defensible.

A caveat I should state rather than let the table imply otherwise: the ground
truth is pairs with identical titles, so a title-only composition scores well on
recall almost by construction. That is why 0 tops the recall column and why it
cannot be chosen on that basis; the pairs column is the counterweight.

Reproduction: `scripts/corpus/prose-budget.rs`.

#### Still to do in this task

Everything needing a database: reading items, writing `item_embeddings` and
`item_chunks`, content-hash skipping, incremental re-embedding off the write path,
the resumable backfill, the vector index on populated tables, and pinning the
column type to the now-known 384 dimensions. None of it is started.

## Acceptance Criteria

## Acceptance Criteria

- [x] Heading-boundary chunking with sliding-window fallback, storing ordinal,
      literal heading text and character range
- [x] No code path keys on a heading's name
- [x] The primary vector is composed from title, type, repository, team, parent
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

### 2026-09-24 — the database half: `kairos_db::embeddings`

The store behind the two tables from [[KAIROS-T-0187]]: what still needs
embedding, writing primary vectors and chunks, and how far behind the vectors
are.

**Raw SQL, and not by preference.** diesel has no `vector` type, so `schema.rs` —
generated by `angreal db schema-sync` from `diesel print-schema` — cannot describe
these columns at all. `crate::search` already carries the same exemption for
`tsvector` under [[KAIROS-A-0009]]. Values are still bound as parameters; only
the vector literal is formatted, and only from `f32`s, which cannot carry SQL.

#### Decisions, each with a test behind it

- **Primary vectors upsert; chunks are replaced wholesale.** Not an
  inconsistency. An item's primary vector is keyed by the item, so a re-embed is
  an update. Chunk ordinals are **positions, not identities**: editing a document
  changes how many sections it has, and an upsert keyed on `(item_id, ordinal)`
  would leave the old tail behind as orphaned chunks that still match queries
  forever. The test shortens a three-chunk document to one and asserts the tail
  is gone.
- **A vector that lies about its width is refused before it reaches the
  database**, and the test asserts nothing was written. A row whose `dimension`
  column disagrees with its vector is worse than no row, because every later
  comparison trusts it.
- **Archiving keeps the vectors.** [[KAIROS-A-0020]] made put-away work
  searchable and prior art in completed work is one of the three claims
  [[KAIROS-T-0191]] exists to make — dropping those vectors would make the most
  valuable answer the one thing unfindable. Only a hard delete forgets, and
  `forget_item` is idempotent because a delete path that runs twice must not
  fail.
- **`pending_primary` takes the model as a parameter and joins on it.** Under a
  different provider/model/dimension every item reads as *never embedded*, so a
  backfill re-does them rather than trusting a content hash from a different
  vector space. Asserted directly.
- **Ordered by `short_code`** so a resumable backfill walks the same sequence
  every run.
- **`counts` separates missing from wrong-model**, because an operator seeing
  thin retrieval needs to know whether the cause is the backlog or the
  configuration. `missing()` clamps at zero: a race between the two subqueries
  must not report a negative backlog.

#### Verified

`crates/kairos-db/tests/embeddings.rs` against real PostgreSQL and pgvector —
vectors round-trip, `<=>` works on the stored column, re-embedding leaves one
row, the shortened document orphans nothing, and the counts mean what they say.
Four unit tests cover the pure helpers, including that the vector literal
round-trips `f32` exactly: a lossy format would silently change every distance
computed against it.

`angreal test integration` is now **44 targets**, all green; fmt and clippy clean
across the workspace.

#### Still to do in this task

The parts that need the provider and the server, not just the database:
composing text and hashing it on the write path, embedding **off** that path,
the `angreal`-driven resumable backfill, the vector index on populated tables,
and pinning the column type to 384 dimensions now the model is settled.

### 2026-09-24 — rule 4's inputs, and the hash that makes skipping possible

`content_hash` moved into `kairos_core::primary`, beside the composition it
hashes — one function for both primary texts and chunk texts, because they
answer the same question and hashing them two different ways would make the two
halves incomparable. Pinned in a test against the **published** SHA-256 of the
empty string rather than against itself, so a change of algorithm cannot pass by
agreeing with the new algorithm.

`pending_primary` now carries the structural inputs rule 4 actually asks for,
which none of `searchable_items` has: repository slug, owning team, and the
parent's title. The parent comes from `item_relationships`, where the edge runs
parent → child, so the item is the **target**. `repository` and `team` join
through `tasks` and are NULL for every other type — which is correct rather than
missing: a document has no repository, and composing a blank `repository:` line
for one would put the same token into every document in the tenant.

That NULL is asserted, because a silently-absent parent fails nothing. It just
quietly makes every child item less findable, which is the kind of bug that is
only ever noticed as "retrieval feels weak".

`metadata_for` fetches a whole batch in one query rather than one per item — a
backfill page of 200 would otherwise be 200 round trips — and orders by
definition name so composition is stable. If the order moved, the content hash
would move with it and every run would re-embed everything. The label is the
definition's **name**, not its slug: rule 4 leans on metadata precisely because
it is where a tenant wrote down what something means, and the name is what they
wrote.

Two assertions in the integration test needed updating when the fixture gained a
parent initiative, because an initiative is itself an item and counts like one.
That is the counts being right, not the test being fragile.

`angreal test integration` green; fmt and clippy clean; 129 `kairos-core` tests.

### 2026-09-24 — the refresh path, and a backfill that actually ran

`kairos_server::embedding::EmbeddingService` joins the three halves:
[[KAIROS-T-0189]]'s provider, `kairos_core`'s composition and chunking, and the
store. It decides what needs doing, which is almost always far less than
everything.

#### Skipping made real, and proven

`sync_chunks` replaced the earlier `replace_chunks`. A chunk's vector is now
`Option<&[f32]>`, where `None` means **unchanged — leave the stored row alone**,
and any stored chunk beyond the end of the new set is deleted. That deletion is
what keeps a shortened document from leaving a tail of chunks that still match
queries forever, citing text the document no longer contains.

The integration test asserts the saving rather than describing it, using
`updated_at` as the witness: after appending to one section of a three-section
document, chunk 0 and chunk 2 have **the same timestamps as before** and only
chunk 1 was rewritten.

End to end, with a real database and a real provider
(`crates/kairos-server/tests/embedding_service.rs`):

| write | texts embedded |
|---|---|
| first pass, nothing stored | 4 (primary + 3 chunks) |
| nothing changed | **0** — no model call at all |
| append to one section | **1**, not 3 |
| retitle only | 1 (the primary), 0 chunks |
| switch model | 4 — everything, from scratch |

That last row is deliberate: a content hash written under one model says nothing
about a vector in a different space, so a model change invalidates rather than
skips.

Whatever does need embedding goes in **one** provider call per item, so a
document refresh is one round trip however many sections moved — which for the
remote provider is one HTTP request rather than nine.

#### The backfill

`kairos-server embed-backfill`, behind `angreal db backfill-embeddings`, with
`--tenant`, `--batch`, `--max-batches` and `--pause-ms`.

**Resumable by construction rather than by bookkeeping.** Each pass asks the
database what is still stale and does a bounded page, so interrupting it loses at
most one page and re-running continues from wherever it really got to. There is
no cursor to persist and therefore none to get out of step with reality.
`--pause-ms` throttles a shared or metered provider; `--max-batches` takes a bite
rather than the whole thing.

Embeddings switched off is **not** an error — it prints that search falls back to
lexical and succeeds, which is rule 7.

#### It ran, against the demo tenant, with the real model

```
backfilling with local/bge-small-en-v1.5-q (384d)
demo: 20 item(s) updated, 41 text(s) embedded in 0.4s — 20/20 current
demo: 0 item(s) updated, 0 text(s) embedded in 0.0s — 20/20 current
```

20 primary vectors, 21 chunks, and the second run does nothing. The nearest
pairs it produces are sensible, which is the first evidence in this initiative
that the substrate does the job on Kairos's own data rather than on Metis
documents:

| distance | | |
|---|---|---|
| 0.213 | Welcome-email trigger | Password-less email auth |
| 0.218 | Portal sign-up flow | Self-serve customer onboarding |
| 0.225 | Invoice webhook handler | Billing provider integration |
| 0.235 | Billing provider spike | Billing provider integration |

#### A bug that only running it could find

The first backfill refused to start: *"the bge-small-en-v1.5-q model is not in
target/embed-cache"* — against a directory that plainly had it.

`LocalProvider::is_cached` stripped `-` and `_` from directory names before
matching, but not `.`, so its needle `bgesmallenv15` was compared against
`modelsqdrantbgesmallenv1.5onnxq` and never matched. A populated cache read as
missing. Fixed, with two regression tests: one on fastembed's actual directory
name, and one through `is_cached` against a directory on disk.

Worth noting what it took to find. Three unit tests covered `is_cached` and all
three asserted the **negative** cases — missing directory, empty directory —
because those were the ones I was thinking about. Nothing asserted that a real
cache reads as present, so the bug sat behind a green suite until something
actually tried to use it.

#### Gates

`angreal test integration` **45 targets**, unit tier green, fmt and clippy clean.

#### Still to do in this task

The vector index on populated tables and pinning the column type to 384 now the
model is settled — both of which want data present, which there now is. Embedding
on the write path (rather than only via the backfill) is the other half.
