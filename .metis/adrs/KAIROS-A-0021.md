---
id: 001-semantic-retrieval-for-agents
level: adr
title: "Semantic retrieval for agents: local embeddings, pgvector, and proposals rather than assertions"
number: 1
short_code: "KAIROS-A-0021"
created_at: 2026-09-24T02:17:45.775796+00:00
updated_at: 2026-09-24T02:19:21.549761+00:00
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

# ADR-21: Semantic retrieval for agents

## Context

Kairos search today is precise and unranked. [[KAIROS-A-0007]] composes three
capabilities — full-text over `title || content`, eleven structural filters,
and graph traverse — and requires at least one, so there is no "return
everything" query. It answers a precise question exactly: *support-lane bugs on
this board, filed last week, carrying `sev1`, reachable from that initiative.*

What it cannot do is answer a vague one. `ts_rank` is never called; results
sort by `created_at`/`updated_at`/`title`, tie-broken on short code. A text
query is a text **filter**, so matches return newest-first rather than
best-first, and the most relevant result can be on page eight.

### The problem this is actually for

Not "search better". **Finding the edges that should exist.**

The graph records *stated* relationships — `parent`, `supports`, `informs`,
`supersedes`, `blocks`. An implicit dependency is by definition one nobody
drew. So the signal is a disagreement between two measures: **semantically
close, but graph-distant.** Two items that read like the same problem with no
path between them are a duplicate, prior art, or a dependency nobody noticed.

Three distinct questions, which this ADR keeps distinct:

- **Prior art** — "has this been solved?" — over completed and put-away work.
  Worth noting this only became answerable with [[KAIROS-A-0020]]: before it,
  archived items 404'd, and prior art lives almost entirely in finished work.
- **Near-duplicates** — "is this already filed?" — over live work, at intake,
  and most valuable on the cross-team filing path ([[KAIROS-A-0019]] §4) where
  an agent files into a team's Backlog without knowing what is there.
- **Implicit dependencies** — the delta above. This is the one needing the
  graph.

### Why this product is unusually suited to it

**The graph is authored, not extracted.** Most GraphRAG spends its effort
inferring a graph from unstructured text — entity and relation extraction that
is lossy, expensive and probabilistic. Kairos already has a typed, authored,
ground-truth graph, plus boards, teams and repositories. The expensive half is
already done, and done better than extraction could manage.

**The communities already exist and have names.** GraphRAG runs Leiden
clustering to *discover* communities and then summarises them. Here an
initiative and its children is a community; a team's delivery board is one; a
repository's tasks are one. The need is summarisation of existing structural
groupings, not unsupervised detection — cheaper, incremental, and explainable
when somebody asks why a result surfaced.

**The producer and the consumer are the same.** Agents write the corpus and
agents read it. Most retrieval systems inherit a corpus they cannot influence.

**And agents have no recall.** A person half-remembers *"didn't we look at this
before?"* An agent does not, and will cheerfully re-solve a problem the
organisation solved last quarter. Nothing in the current surface would stop it.
Prior art matters **more** in an agent-first product, not less.

### What the corpus actually looks like

Measured over 231 Metis documents in this repository — which, because agents
are the usual authors and humans mostly edit, is the population rather than a
sample:

| type | n | mean | p50 | p90 | max |
|---|---|---|---|---|---|
| task | 183 | 5,767 | 5,105 | 9,767 | 18,491 |
| ADR | 20 | 7,603 | 7,461 | 12,471 | 12,636 |
| initiative | 16 | 12,483 | 9,883 | 26,374 | 26,383 |
| specification | 8 | 10,393 | 13,772 | 24,652 | 24,652 |

**98% exceed 2,000 characters.** A median ticket is ~5 KB, about 1,250 tokens —
already 2.5× the 512-token window of a small embedding model, and an initiative
exceeds it sixfold. These are documents, not records. `title` and `content` are
`TEXT` with no length limit and no validation.

Two further measurements shaped the decisions below:

- **Structure is reliable; labels are not.** All 230 documents with body text
  have headings — zero exceptions — and 94% of sections are under 2,000
  characters, median 309. But there are **538 distinct heading strings across
  2,255 headings, and 84% appear exactly once.** Templates ([[KAIROS-A-0003]])
  are tenant-defined and evolve, so any design keyed to named sections would
  work in one tenant and break in the next.
- **Self-similarity is lower than feared.** Agent-authored text shares voice
  and template, so the concern was that everything would look related and the
  useful similarity range would collapse. Median pairwise vocabulary overlap is
  0.12, p90 0.167, and only 13% of a document's vocabulary is corpus-common.
  Lexical overlap is a weak proxy for embedding similarity, so this wants
  re-measuring once embeddings exist — but the risk is smaller than assumed,
  not absent.

## Decision

Kairos gains semantic retrieval as a **separate surface that extends
[[KAIROS-A-0007]]**, built for agents first. Six rules.

### 1. Embeddings are computed locally by default; providers are pluggable

A model ships with the deployment and runs in-process, so a working install
needs nothing external. A deployment may instead point at any
**OpenAI-compatible embeddings endpoint** — which is what Ollama, vLLM, LM
Studio, TEI and the commercial APIs all speak — configured the way identity is
configured.

The default matters more than the option. [[KAIROS-A-0016]] says state and
identity are the operator's; sending every work item to a third party
contradicts that for exactly the customers who chose to self-host. Local-first
also gives deterministic embeddings, which [[KAIROS-A-0012]] needs — it forbids
mocks, so retrieval tests must be reproducible against a real model.

### 2. Vectors live in Postgres via pgvector, and the chart gains an optional bundled database

`pgvector` becomes a requirement of the database Kairos is pointed at. It is
available on RDS, Cloud SQL and Azure, so "bring your own Postgres" survives.

**This amends [[KAIROS-A-0016]].** The chart currently bundles no database, and
`deploy/helm/kairos/README.md` states the reason: *"a stateful dependency in
the chart would contradict A-0016's 'state and identity are the operator's'
posture."* That sentence becomes false. The chart gains a **disableable**
Postgres subchart, on by default for evaluation and off for anyone bringing
their own. The posture is unchanged in substance — state is still the
operator's, and the bundled database is a convenience they can decline — but
the earlier absolute is now a default, and the chart documentation must say so.

In-process brute force was considered and rejected on the corpus measurements
above. Chunked, ~5,000 work items is ~30k vectors (~46 MB) and ~20,000 items is
~180 MB **per tenant**, against one stateless binary serving every tenant in a
deployment. Holding that in memory is the wrong shape.

### 3. Chunking is on heading boundaries, which are anchors rather than labels

Documents are chunked at markdown heading boundaries, whatever those headings
say, with a **sliding-window fallback** for text that has none.

The measurements support the shape and forbid the labels. Every document has
headings and 94% of sections fit comfortably in a small model's window, so
boundaries give semantically coherent units for free. But 84% of heading
strings are unique, and templates evolve, so nothing may key on a heading being
called "Acceptance Criteria". Headings are used the way the web already uses
them — as **anchors**: a citation echoes the literal text it found rather than
recognising it.

This also decides the cost model. Metis instructs agents to update Status
Updates every few tool calls, so edits are continuous and high-rate rather than
occasional. Section-scoped chunking means an append re-embeds **one section**,
not a 12 KB document. That is what makes this affordable, not merely tidier.

### 4. The primary vector is composed by the system, not promised by the document

Alongside section chunks, each item carries a primary vector composed from what
Kairos knows: title, entity type, repository, owning team, parent's title,
stamped metadata, and the opening prose.

An earlier draft required a template section written to be embedded. That
assumed control the product does not have — templates are tenant-defined and
evolve. Everything above is structural, present for every item, and immune to
template drift. Stamped metadata carries particular weight: in a corpus where
section names cannot be trusted, metadata definitions are the one place meaning
is explicitly labelled.

A generated synopsis as primary vector remains available later, as an
optimisation, precisely because nothing here depends on the document's own
structure.

### 5. Results are proposals, not assertions

Every result carries its **claim type** (prior art, near-duplicate, possible
dependency), its score, and *why* — "same repository, 0.83 similar, no edge
between them". Results are bounded to a handful, because the consumer has a
context budget.

An agent told "this blocks you" when it does not will do wrong work
confidently, which is worse than returning nothing. The contract is to surface
a candidate with its evidence and let the caller weigh it.

### 6. Agents may propose graph edges; humans confirm

Where the signal is strong, an agent may propose a `parent` or `blocks` edge
rather than merely reporting similarity. Proposals are not edges until
confirmed.

This fits the division of labour the product already has — agents create,
humans edit — and it compounds: every confirmation turns an implicit dependency
into an explicit one, so the graph improves with use and the next retrieval is
better than the last.

## Alternatives Analysis

**A graph database — Apache AGE — considered and deferred.** Two reasons, and
the second is the one that decided it.

It would undercut rule 2. pgvector is available on RDS, Cloud SQL and Azure, so
bundling a disableable database keeps "use your cloud Postgres" true. AGE is on
none of those, so taking it would mean graph features work on the bundled
database only — "disableable, but you lose half the feature" — which is two
deployment tiers rather than one.

And the scale does not ask for it. `item_relationships` is a single table;
depth-bounded neighbourhood queries over thousands of edges are milliseconds in
a recursive CTE or in the Rust BFS that already exists
(`kairos_core::items::cascade_descendants`, whose module docs record that it was
*"chosen over a recursive CTE so the traversal decision lives in core and is
unit-testable"*). Taking AGE would reverse that deliberately-taken position.

What AGE buys is community detection and variable-length path patterns at
scale. Kairos's communities already exist structurally — an initiative and its
children, a team's board, a repository's tasks — so the expensive thing a graph
engine provides is the thing the authored graph gives free.

**Trigger to revisit:** wanting real community detection beyond the structural
groupings, or path queries the structural relationships cannot express. It can
be added later behind the same retrieval API, so deferring costs nothing and
adopting now costs a deployment tier.

**Ranking alone, without semantic retrieval.** Much cheaper: `ts_rank` is never
called today, and adding relevance scoring to the existing index would fix
"the best answer is on page eight" on its own. Rejected as *sufficient* but
retained as *worthwhile* — hybrid retrieval (lexical + vector, fused) is the
expected implementation, and the lexical half is that improvement. It does
nothing for implicit dependencies, which is the actual goal.

**A remote embedding API by default.** Rejected: see rule 1.

**Extending [[KAIROS-A-0007]] in place** rather than adding a surface.
Rejected: A-0007 promises composable capabilities with an at-least-one rule, a
≤5-query bound and a deterministic sort. Semantic retrieval is ranked,
approximate, and returns typed claims rather than rows. Folding it in would
weaken guarantees the existing endpoint makes to its existing callers.

**One embedding per document, no chunking.** Rejected on measurement: a median
ticket is 2.5× a small model's context window and an initiative is six times
it. One blurry vector per document, and no way to say which part matched.

## Consequences

### Positive

- The question an agent cannot otherwise ask — *has this been done, and what
  does it quietly depend on?* — becomes answerable, and prior art becomes
  reachable at all, which [[KAIROS-A-0020]] made possible only days ago.
- Ranking arrives as a side effect of hybrid retrieval.
- The graph improves with use (rule 6), so retrieval quality compounds instead
  of decaying.
- Nothing depends on template structure, so tenants may evolve templates freely.

### Negative

- **The database contract changes.** pgvector is now required, and
  `deploy/helm/kairos/README.md` and [[KAIROS-A-0016]] both need amending
  rather than quietly contradicting.
- **The image grows.** A local model is ~100–200 MB against a runtime that is
  currently a Rust binary plus libpq, which touches [[KAIROS-A-0013]]'s "one
  image, one binary" economy — still one image and one binary, but a heavier
  one.
- **Write amplification.** Every edit re-embeds at least one section, on a
  workflow that edits constantly. Section scoping bounds it; it does not remove
  it.
- **A new failure mode: confident wrong suggestions.** Rule 5 mitigates by
  contract, not by construction.

### Neutral

- Embedding quality on this corpus is unmeasured. The lexical proxy suggests
  agent-authored documents are more distinct than feared, but that needs
  re-testing with real embeddings before any threshold is chosen.

## Review Triggers

- Wanting community detection or path patterns the structural groupings cannot
  express — revisit AGE.
- pgvector becoming unavailable on a target platform, or per-tenant vector
  counts outgrowing a single Postgres.
- Evidence that proposals are being treated as assertions by callers, which
  would mean rule 5's contract is not enough on its own.