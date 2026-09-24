---
id: embedding-providers-local-by
level: task
title: "Embedding providers: local by default, OpenAI-compatible as BYO, fake for tests"
short_code: "KAIROS-T-0189"
created_at: 2026-09-24T02:27:52.746086+00:00
updated_at: 2026-09-24T10:37:17.768488+00:00
parent: KAIROS-I-0017
blocked_by: []
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

Something that turns text into a vector. [[KAIROS-A-0021]] rule 1: **a local
model by default**, an OpenAI-compatible endpoint as the bring-your-own option.
Default-local because a work-management system whose search requires shipping
every ticket title to a third party is a system a lot of organisations cannot
deploy.

## Implementation Notes

### Technical Approach

A trait in `kairos-core` — text in, vectors out, batched, with the model name and
dimension it produces — and three implementations:

1. **Local, in-process.** A small sentence-embedding model loaded at startup. The
   deciding constraints are that it must be embeddable in the single stateless
   binary the product is (A-0013), must run on both `linux/amd64` and
   `linux/arm64` because the image is multi-arch from v0.1.1, and must not need a
   GPU. Candidate stacks get compared in this task and the choice recorded with
   its binary-size and cold-start cost, because "the image doubled" is a real
   outcome and the person deploying deserves the number.
2. **OpenAI-compatible HTTP.** Base URL, model name, API key, so it reaches
   OpenAI, a local Ollama, a vLLM server, or anything that speaks the shape.
   Configured per deployment, not per tenant.
3. **Deterministic fake**, for tests. Not optional: every test above unit level
   needs embeddings that are stable, free and instant, and a hash-derived vector
   gives all three. Retrieval tests assert on ordering, which a deterministic
   fake can satisfy.

### Dimension is a deployment-level fact

Changing provider or model changes the vector space, and vectors from two spaces
cannot be compared. So the provider and model are recorded per row (T-0187), and
a mismatch between what is stored and what is configured must be **detected and
reported**, not silently compared. The recovery is a re-embed, which is T-0190's
backfill. This task's job is to make the mismatch visible.

### Dependencies

None strictly — the trait and the implementations stand alone. Ships cleanly in
parallel with [[KAIROS-T-0187]].

### Risk Considerations

- **Image size and cold start.** A local model is tens to hundreds of megabytes.
  Measure both, record both, and if the cost is unacceptable for the default the
  answer is a smaller model, not a remote default — rule 1 is a posture decision,
  not a performance one.
- **Remote provider failure must degrade, not fail.** An unreachable endpoint
  means new writes go un-embedded and retrieval falls back to lexical (rule 7).
  It must not fail the write: an agent creating a ticket must not be blocked
  because an embedding service is down.
- The API key is a secret, so it follows the same existing-Secret pattern
  `DATABASE_URL` already uses in the chart.

## Acceptance Criteria

## Acceptance Criteria

- [ ] An embedding trait in `kairos-core` with local, OpenAI-compatible and
      deterministic-fake implementations
- [ ] Local is the default and needs no configuration
- [ ] The chosen local model, its licence, the image-size delta and cold-start
      cost are recorded in the Status Updates
- [ ] Works on both `linux/amd64` and `linux/arm64`, verified on the built image
- [ ] Provider and model are recorded per stored vector; a configuration mismatch
      is reported rather than silently compared
- [ ] A provider that is unreachable never fails a write
- [ ] The remote API key follows the existing-Secret pattern in the chart
- [ ] `angreal test` green

## Status Updates

### 2026-09-23 — fastembed spike: it works, with one blocker

Spiked `fastembed` 7.1.0 (`ort` 2.0.0-rc) with `BGESmallENV15` on arm64 macOS,
embedding 4,927 real Metis documents. Measured, not estimated:

| | |
|---|---|
| dimension | 384 |
| release binary | 33.5 MB (~+28 MB over a plain binary of the same crate) |
| onnxruntime | **statically linked** — `otool -L` shows no `libonnxruntime`, so A-0013's single-binary constraint survives |
| throughput | 46 texts/s, batch 256, CPU only, no GPU |
| cold init from warm cache | 0.1 s |
| first run | 18.4 s, of which nearly all is the model download |

**The blocker: the model is fetched from HuggingFace at runtime.** fastembed
downloaded `Xenova/bge-small-en-v1.5` into a 130 MB `.fastembed_cache` on first
use. A stateless container that must reach huggingface.co before it can serve is
not "local by default" in any sense an operator would accept, and it breaks
air-gapped deployments outright.

So this task must **bake the model into the image** and point fastembed at the
local path rather than letting it resolve from the hub. That is an image-size
decision to state plainly in the chart documentation: roughly +160 MB
(28 MB runtime + 130 MB weights).

### Cost of the backfill, from the same run

46 texts/s means the 4,927-document corpus took 108 s at document level. At chunk
level the same corpus is 63,270 chunks — about 23 minutes single-threaded. That is
[[KAIROS-T-0190]]'s backfill budget on one machine for a corpus of roughly 5,000
documents, and it is why the backfill has to be resumable and rate-limitable
rather than a single transaction.

Throughput is also the argument for embedding off the write path: 22 ms per text
is far too long to sit inside `create_item`.

### 2026-09-24 — the model is chosen, and it was chosen on this corpus

Four candidates, measured on the 4,927-document corpus from [[KAIROS-T-0190]]
with ground truth taken from the corpus itself rather than from a published
benchmark: **same-project pairs whose titles are identical but whose short codes
differ are duplicate tickets by construction**. There are 25 such pairs across
the nineteen repositories.

The metric is **recall@1** — for each duplicate ticket, is its twin the single
nearest neighbour within its own project? That is the question this product
actually asks, measured directly.

| model | dim | texts/s | on disk | recall@1 | mean cos of the true pairs |
|---|---|---|---|---|---|
| `BGESmallENV15` | 384 | 46 | 128 MB | 82% | 0.959 |
| **`BGESmallENV15Q`** | **384** | **42** | **65 MB** | **82%** | **0.959** |
| `BGEBaseENV15Q` | 768 | 12 | 210 MB | 84% | 0.950 |
| `AllMiniLML6V2Q` | 384 | — | 23 MB | rejected, see below | — |

**`BGESmallENV15Q` is the choice.** Static quantization halves the model — 128 MB
to 65 MB — for **no measured loss at all**: identical recall@1, identical mean
similarity on the true pairs, within 10% on throughput.

`BGEBaseENV15Q` is not worth it. Two points of recall is **one pair** out of the
50 trials (25 pairs, both directions), which is noise at this sample size, and it
costs 3.2x the size and 3.5x the throughput. If a later measurement on a larger
ground-truth set shows a real gap, the provider is pluggable and this is a
configuration change.

### Rejecting the 23 MB model, and correcting myself about why

`AllMiniLML6V2Q` is a third the size of the chosen model, and fastembed refuses to
batch it: *"the dynamic quantization process adjusting the data range to fit each
batch, making the embeddings incompatible across batches."* My first reading was
that this is a **throughput** problem, and I was wrong — measured unbatched, it
runs at over 1,000 short texts per second, faster than the chosen model batched.

The real problem is **reproducibility**, and it is worse than a throughput cliff.
Embedding the same text alongside different companion texts:

| model | same text, different company | alone vs in company |
|---|---|---|
| `AllMiniLML6V2Q` (dynamic) | cos **0.992455** | cos **0.991010** |
| `BGESmallENV15Q` (static) | cos 0.999999 | cos 1.000000 |
| `BGESmallENV15` (none) | cos 1.000000 | cos 1.000000 |

A dynamically quantized model does not return the same vector for the same text.
That is fatal here rather than merely untidy, because [[KAIROS-T-0187]]'s
`content_hash` exists to let unchanged text be skipped: if re-embedding
unchanged text yields a different vector, the hash is telling the truth while the
store disagrees with itself, staleness becomes undetectable, and identical
queries return different orders on different days.

Rejected on correctness, therefore, not on speed. The number that makes it
concrete: its self-reproduction error is 0.008, against a **0.12** gap between
related and unrelated pairs — so the noise is a substantial fraction of the whole
signal this initiative is built on.

### Still to build in this task

The trait and the three providers, and the model has to be **baked into the
image** rather than fetched from HuggingFace at first use — the finding from the
earlier spike, unchanged and still the blocker for air-gapped deployments.

### Superseded: the model question as it stood before the measurement


Whether 384 dimensions is the right trade. bge-small is the smallest credible
choice; bge-base is 768 and roughly triple the size. The measurement below
([[KAIROS-T-0190]]) suggests the limiting factor is **not** model capacity but
that long templated documents look alike, so a bigger model likely buys little.
Worth one comparison run before settling.