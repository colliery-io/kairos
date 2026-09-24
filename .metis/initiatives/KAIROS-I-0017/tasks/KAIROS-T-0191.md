---
id: the-retrieval-surface-hybrid
level: task
title: "The retrieval surface: hybrid fusion, graph distance, and typed proposals"
short_code: "KAIROS-T-0191"
created_at: 2026-09-24T02:27:58.779654+00:00
updated_at: 2026-09-24T02:27:58.779654+00:00
parent: KAIROS-I-0017
blocked_by: [KAIROS-T-0186, KAIROS-T-0190]
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

The thing the initiative is for. An agent asks *"what is related to this, and
what does it quietly depend on?"* and gets typed, cited, bounded **proposals**.
[[KAIROS-A-0021]] rules 5, 6 and 7.

This is a **separate surface** that extends [[KAIROS-A-0007]] rather than
changing it. A-0007's endpoint keeps its at-least-one rule, its ≤5 query bound
and its deterministic sort; callers relying on those keep them.

## Implementation Notes

### Hybrid, fused by rank position (rule 7)

Lexical and vector both run; results are fused by **reciprocal rank** rather than
by comparing a `ts_rank` to a cosine, because those two numbers have no common
scale and any weighted sum of them is a fiction. Rank position is comparable;
score magnitude is not.

Degradation is part of the contract, not an error case: no vectors yet, a stale
model, an unreachable provider — the surface answers lexically and **says it
did**, so a caller can tell a thin answer from a complete one.

### The signal (rule 5)

Similarity alone is not the claim. The claim is a **disagreement between
similarity and the graph**:

| similarity | graph | claim |
|---|---|---|
| high | no path | candidate implicit dependency |
| high | shared parent | likely near-duplicate |
| high | item is completed or put away | prior art |

Same repository, same board and shared parent are **weights, not filters** —
[[KAIROS-A-0019]] makes repository a strong prior for genuine coupling, and a
filter would hide the cross-team duplicate that is the most valuable hit there is.

Prior art in put-away work is the part [[KAIROS-A-0020]] made possible and nothing
yet exploits. It is not an afterthought: *"didn't we try this?"* is exactly the
question an agent cannot answer from memory.

### Graph distance

From `item_relationships` — a depth-bounded recursive CTE, or the existing Rust
BFS (`kairos_core::items::cascade_descendants`). **No graph engine**; A-0021's
alternatives records why AGE is deferred and what would bring it back.

Note the honest caveat: `item_relationships` held 18 rows in the demo tenant.
"No path" is cheap to assert in a sparse graph and means less than it sounds. Say
so in the result's *why* rather than implying the graph was consulted and found
wanting.

### The contract (rule 6)

- typed claim, score, and **why** — which text matched, under which heading, and
  what the graph said
- **bounded**: a handful, never a page. An agent that receives forty related
  items has been given a research project, not an answer
- **proposals, never assertions.** An agent told "this blocks you" when it does
  not will do wrong work confidently, which is worse than being told nothing.
  The response wording carries this, not just the documentation

Both an MCP tool and a REST endpoint, because agents reach Kairos both ways, and
the MCP tool is the one that matters most here.

### There is no threshold. This is measured, not assumed.

[[KAIROS-T-0190]] measured full pairwise cosine over 4,927 real Metis documents
from 19 projects. The result constrains this task hard:

| pair class | mean | p99 |
|---|---|---|
| different project | 0.594 | 0.708 |
| same project, no graph relation | 0.680 | 0.817 |
| direct parent/child edge | **0.800** | 0.916 |

Pairs the graph says are related average 0.800. Pairs with no relation whatsoever
reach 0.817 at p99. **The distributions overlap across their entire useful
range**, and there is no zero point — two documents from unrelated projects still
score 0.594.

**Confirmed a second time, on different data.** [[KAIROS-T-0190]] repeated the
measurement over the seeded `demo` tenant once it had real vectors — Kairos's own
work items rather than Metis documents:

| class | n | mean | min | max |
|---|---|---|---|---|
| shared parent | 13 | 0.727 | 0.661 | 0.787 |
| direct edge | 18 | 0.705 | 0.568 | 0.782 |
| no relation | 159 | 0.580 | 0.424 | **0.752** |

Same gap — about 0.13 against the 0.12 measured across nineteen repositories —
and the same overlap: unrelated pairs reach 0.752 while pairs joined by a real
edge fall to 0.568. There is no value you could put between them, on either
corpus.

So: no `cosine > x` anywhere in this surface. Ranking **within one query** is
meaningful; comparing a score to a constant is not. Take the top few by fused
rank and stop. If the caller wants to know how strong a match is, give them the
rank and the evidence, not a number that looks absolute and is not.

At the extreme tail (>0.90) precision was roughly half by hand inspection — good
enough to propose, nowhere near good enough to assert, which is rule 6 vindicated
on evidence.

### Long documents are the weak spot

T-0190's false positives clustered in initiative-level and specification-level
pairs: long, heavily templated, generically titled documents whose primary vector
is mostly boilerplate. `mimir`'s "Documentation Site" and "Groq Provider" score
0.940 and have nothing to do with each other.

Whatever T-0190 lands on for level-sensitive composition, this surface should
prefer **chunk-level** evidence over primary-vector similarity when both
candidates are long documents, and must never return a claim whose only support
is two primary vectors agreeing.

### Dependencies

[[KAIROS-T-0186]] for lexical ranking — it is the fallback — and
[[KAIROS-T-0190]] for the vectors.

### Risk Considerations

- **A confident wrong "duplicate" is the failure that discredits the feature.**
  Prefer returning nothing to returning a weak claim, and make the bound tight.
- Liveness: put-away items are included **for prior art specifically**, which is
  a different default from A-0020's hide-by-default. The asymmetry is deliberate
  and must be stated where a reader will find it.
- Permissions: retrieval must not surface an item the caller could not fetch
  directly. Reuse the existing authorisation path rather than building a second
  one — filed defects T-0182 and T-0183 are a reminder of what happens when an
  authorisation check is written afresh.

## Acceptance Criteria

- [ ] An MCP tool and a REST endpoint that take an item and return related-work
      proposals
- [ ] Lexical and vector results fused by rank position, not by score arithmetic
- [ ] With no vectors, a stale model or an unreachable provider, the surface
      answers lexically and states that it did
- [ ] Claims are typed: implicit dependency, near-duplicate, prior art
- [ ] Every result carries a score and a *why* naming the matched text and its
      literal heading
- [ ] Results are bounded to a handful, and the wording is proposal-shaped
      throughout
- [ ] Repository, board and parent act as weights, never filters
- [ ] Put-away work is reachable as prior art, and the asymmetry with
      [[KAIROS-A-0020]] is documented
- [ ] Sparse-graph honesty: "no path" is qualified in the *why*
- [ ] Authorisation reuses the existing path, with a test that a caller cannot see
      what they could not fetch
- [ ] [[KAIROS-A-0007]]'s endpoint behaviour is unchanged
- [ ] No absolute cosine threshold appears anywhere in the implementation, and a
      test would fail if one were reintroduced
- [ ] A claim is never supported by two primary vectors alone when both sides are
      long documents
- [ ] `angreal test` green

## Status Updates

*To be added during implementation*
