---
id: semantic-and-graph-retrieval
level: initiative
title: "Semantic and Graph Retrieval - Finding the Edges That Should Exist"
short_code: "KAIROS-I-0017"
created_at: 2026-09-24T02:24:32.365415+00:00
updated_at: 2026-09-24T02:56:53.982666+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/active"


exit_criteria_met: false
estimated_complexity: XL
initiative_id: semantic-and-graph-retrieval
---

# Semantic and Graph Retrieval - Finding the Edges That Should Exist Initiative

## Context

[[KAIROS-A-0021]] decided this; read it first. This initiative implements its
seven rules. The short version: Kairos search is precise and unranked, and the
problem worth solving is not better search but **finding the edges that should
exist** — work that is semantically close yet graph-distant is a duplicate,
prior art, or a dependency nobody drew.

The product is unusually suited to it. The graph is **authored, typed and
ground-truth** rather than extracted from prose, and the communities already
have names — an initiative and its children, a team's board, a repository's
tasks. The expensive half of GraphRAG is already done.

And it matters more here than elsewhere: a person half-remembers *"didn't we
look at this?"*; an agent does not, and will cheerfully re-solve last quarter's
problem. Agents are the usual authors and the usual readers.

### The measurements that constrain the work

From 231 Metis documents in this repository, which — because agents write and
humans edit — is the population rather than a sample:

- A median ticket is **~5 KB (~1,250 tokens)**, 98% exceed 2,000 characters,
  initiatives reach 26 KB. Chunking is mandatory; in-process brute force does
  not scale across tenants.
- **Structure is reliable, labels are not.** Every document has headings and
  94% of sections are under 2,000 characters — but there are 538 distinct
  heading strings and **84% appear exactly once**. Nothing may key on a section
  being called "Acceptance Criteria".
- Agent-authored self-similarity is **lower than feared, not absent** (median
  pairwise vocabulary overlap 0.12). Needs re-measuring with real embeddings
  before any threshold is chosen.

## Goals & Non-Goals

**Goals:**
- An agent can ask *"what is related to this, and what does it quietly depend
  on?"* and get typed, cited, bounded proposals.
- Prior art in put-away work is reachable — which [[KAIROS-A-0020]] made
  possible and nothing yet exploits.
- Ranking exists: hybrid lexical + vector, fused by rank position.
- Retrieval degrades gracefully to lexical when vectors are missing, stale or
  the provider is unreachable.
- The graph improves with use: proposed edges, confirmed by humans.

**Non-Goals:**
- **No graph database.** AGE is deferred with its trigger recorded in A-0021.
- **No community summarisation.** The structural groupings serve that need;
  generated cluster summaries are a later question.
- Not replacing [[KAIROS-A-0007]]. This is a separate surface that extends it;
  the existing endpoint keeps its guarantees.
- Not changing templates. Tenants own them and they evolve.

## Detailed Design

### D1 — Order of work, and why

**Lexical ranking first.** It is useful alone, it fixes "the best answer is on
page eight" without a model, and — the deciding reason — **everything else
falls back to it** (A-0021 rule 7). Building the fallback last would mean the
system has no degraded mode during its own backfill.

Then storage, then the embedder, then chunking and backfill, then the retrieval
surface, then edge proposals. Each layer is useful before the next exists.

### D2 — Storage

`pgvector` in the tenant schema, per [[KAIROS-A-0021]] rule 2. Embeddings are
per-tenant by construction, which schema-per-tenant gives free.

Two tables rather than one column: an item's **primary vector** (composed, D4)
and its **section chunks**. They answer different questions — "what is this
item about" versus "which part of it matched" — and have different invalidation
rates.

### D3 — Chunking

Heading boundaries as **anchors, not labels**, with a sliding-window fallback
for text without headings. A citation echoes the literal heading text it found
rather than recognising it.

This is the cost model as much as the retrieval model: Metis instructs agents
to update Status Updates every few tool calls, so an append must re-embed
**one section**, not a 12 KB document.

### D4 — The primary vector is composed, not promised

Title, entity type, repository, owning team, parent's title, stamped metadata,
opening prose. All structural, present for every item, immune to template
drift. Stamped metadata carries weight: where section names cannot be trusted,
metadata definitions are the one place meaning is explicitly labelled.

### D5 — The signal

Similarity alone is not the answer. The claim is a **disagreement**:

- high similarity + **no path** in the graph → candidate implicit dependency
- high similarity + shared parent → likely near-duplicate
- high similarity + item is put away/completed → prior art

Same repository, same board and shared parent are weights, not filters —
[[KAIROS-A-0019]] makes repository a strong prior for genuine coupling.

Graph distance comes from `item_relationships` via a depth-bounded query or the
existing Rust BFS. No graph engine (A-0021 alternatives).

### D6 — The contract

Typed claims, scores, and *why*; bounded to a handful. **Proposals, never
assertions** — an agent told "this blocks you" when it does not will do wrong
work confidently, which is worse than returning nothing.

### D7 — Edge proposals

Where the signal is strong an agent may propose a `parent` or `blocks` edge.
Proposals are not edges until a human confirms, which fits the division of
labour the product already has, and compounds: each confirmation turns an
implicit dependency into an explicit one.

## Alternatives Considered

Recorded in [[KAIROS-A-0021]] rather than repeated: Apache AGE (deferred, with
trigger), remote embeddings by default (rejected on posture), extending A-0007
in place (rejected — it would weaken guarantees existing callers rely on), one
embedding per document (rejected on measurement), and ranking alone (adopted as
part of the whole rather than as a substitute).

## Implementation Plan

Eight tasks. Three of them ([[KAIROS-T-0186]], [[KAIROS-T-0187]],
[[KAIROS-T-0189]]) have no dependencies and can run in parallel.

1. [[KAIROS-T-0186]] **Lexical relevance** — `ts_rank_cd` on the existing search,
   and the decision about whether relevance becomes the default ordering when
   `q` is present. This is also where A-0007's "no `ts_rank`" sentence gets
   deliberately reversed.
2. [[KAIROS-T-0187]] **pgvector** — the extension, the tenant migration, the two
   tables. Also every Postgres pin in the repository, including the Kubernetes
   tutorial, which has to be re-executed rather than edited.
3. [[KAIROS-T-0188]] **The bundled database** — the chart's disableable Postgres,
   the [[KAIROS-A-0013]]/[[KAIROS-A-0016]] amendment, and the now-false sentence
   in the chart README.
4. [[KAIROS-T-0189]] **Embedding providers** — local model in-process by default,
   OpenAI-compatible endpoint as BYO, deterministic fake for tests.
5. [[KAIROS-T-0190]] **Chunking and backfill** — the heading chunker with
   fallback, the composed primary vector, incremental re-embedding off the write
   path, resumable backfill. Also re-measures self-similarity with real
   embeddings, which is where T-0191's thresholds come from.
6. [[KAIROS-T-0191]] **The retrieval surface** — the MCP tool and REST endpoint,
   RRF fusion, the similarity-versus-graph signal, typed proposals.
7. [[KAIROS-T-0192]] **Edge proposals** — proposing, storing, confirming, and
   somewhere in the GUI a human can act on them.
8. [[KAIROS-T-0193]] **Close out** — the book, a UAT journey, and the drift gate,
   which will read short until a journey exercises the new tools.

Gates per task: `angreal test` green, and anything touching default search
ordering must show the existing endpoint's behaviour unchanged unless the task is
the one deliberately changing it.

Two tasks carry a decision rather than only an implementation, and both are
recorded in the task rather than settled here: whether relevance becomes the
default sort (T-0186) and whether the bundled database defaults on (T-0188). Each
has a recommendation and its reasoning.

## Progress Log

- 2026-09-23: Created from [[KAIROS-A-0021]], which Dylan decided across two
  discussions, and decomposed into eight tasks.

- 2026-09-23: **Measured the design against 19 Metis corpora, not just Kairos.**
  Dylan asked for a wide cosine sample. 4,927 documents across all nineteen
  repositories in `~/Desktop`, embedded with the real candidate model
  (`bge-small-en-v1.5`, 384 dim) and full pairwise cosine computed. Full numbers
  in [[KAIROS-T-0190]]; the spike's cost figures in [[KAIROS-T-0189]];
  reproduction in `scripts/corpus/`.

  Three results changed or hardened the design:

  1. **Rule 3 is stronger than believed.** Zero of 4,927 documents lack headings,
     and only 2.2% of sections need the sliding-window fallback. 85% of heading
     strings appear exactly once, and the tenth most common heading is a template
     marker nobody deleted — so anchors-not-labels is right twice over.

  2. **No absolute similarity threshold can work.** Pairs the graph says are
     related average 0.800 cosine; pairs with no relation at all reach 0.817 at
     p99, and the floor for unrelated *projects* is 0.594. The distributions
     overlap across their whole useful range and there is no zero point.
     [[KAIROS-T-0191]] was amended: rank within a query, never compare to a
     constant, and a test should fail if a threshold reappears.

  3. **The feature works, and hybrid is necessary rather than prudent.** Above
     0.90 there are 270 unlinked same-project pairs out of 1.1M — including ten
     literal duplicate tickets in `brokkr` filed twice under different codes, an
     undrawn Helm-chart dependency in `cloacina`, and a missing ADR-to-initiative
     link in `muninn`. Precision there is roughly half, which is fine for
     proposals and hopeless for assertions — rule 6 on evidence. Lexical finds
     the identical-title duplicates (22% of candidates); the vector finds the
     paraphrases (77%); neither finds both.

  One new problem: fastembed downloads its weights from HuggingFace at runtime, so
  [[KAIROS-T-0189]] must bake the model into the image or air-gapped deployments
  cannot start. And the false positives cluster in initiative- and
  specification-level documents, whose primary vectors are mostly boilerplate —
  rule 4's composition needs a level-sensitive amendment, now recorded in
  [[KAIROS-T-0190]].

- 2026-09-23: Moved to active. Starting [[KAIROS-T-0186]], which has no
  dependencies and is the fallback everything else degrades to.