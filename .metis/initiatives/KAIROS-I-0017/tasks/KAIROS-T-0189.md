---
id: embedding-providers-local-by
level: task
title: "Embedding providers: local by default, OpenAI-compatible as BYO, fake for tests"
short_code: "KAIROS-T-0189"
created_at: 2026-09-24T02:27:52.746086+00:00
updated_at: 2026-09-24T17:00:28.960313+00:00
parent: KAIROS-I-0017
blocked_by: []
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

### 2026-09-24 — built: `kairos-embed`, three providers, model baked into the image

A new crate rather than code in `kairos-server`. `kairos-core` is pure by
[[KAIROS-A-0009]] and loading an ONNX model is I/O; the server would have worked,
but then the CLI and the soak driver would link an ML runtime to reach a trait.
With `local` and `remote` behind features they do not.

| file | what it is |
|---|---|
| `lib.rs` | the `EmbeddingProvider` trait, `ModelId`, `Mismatch`, `cosine`, batch validation |
| `local.rs` | fastembed, `bge-small-en-v1.5-q`, 384d |
| `remote.rs` | OpenAI-compatible HTTP |
| `deterministic.rs` | hash-derived, for tests |
| `config.rs` | `KAIROS_EMBED_*`, defaulting to local with nothing set |
| `bin/fetch-model.rs` | populates the cache at image build time |

**Rule 1 is satisfied by silence.** With no configuration at all the provider is
local. Setting `KAIROS_EMBED_URL` selects remote on its own, because an operator
who has configured an endpoint has already said what they want and asking them to
also set `KAIROS_EMBED_PROVIDER=remote` is a second chance to get it wrong.
`KAIROS_EMBED_PROVIDER=none` is a supported answer, not a failure — `build()`
returns `Ok(None)` and the caller degrades to lexical, which is rule 7.

### The model is baked into the image, which is the point

The earlier spike found fastembed downloading weights from HuggingFace at first
use. A stateless container that must reach the internet before it can serve is
not "local by default" and breaks air-gapped deployments outright.

So `fetch-model` runs in the Dockerfile's builder stage and the runtime stage
copies `/var/lib/kairos/models`; `KAIROS_EMBED_CACHE` points at it and
**downloading stays disabled**. If the model layer ever failed to copy, the
operator gets an error naming the directory rather than a container quietly
pulling 65 MB on first request. `fetch-model` drives `LocalProvider` itself, so
the cache layout is fastembed's rather than a reproduction of it, and it embeds a
probe string — a model that cannot load fails **the build** instead of the first
request. `angreal dev fetch-model` is the same thing for a source build.

### Decisions worth naming

**The trait is synchronous.** Local inference is CPU-bound and wants a thread, not
a task; an async trait would make it pretend. The cost is that `remote.rs` uses
`reqwest::blocking`, which **panics** inside an async worker — so the constraint is
documented on the trait itself, because a caller holding a `dyn EmbeddingProvider`
cannot see which implementation it has. [[KAIROS-T-0190]] embeds off the request
path anyway.

**`LocalProvider` holds a `Mutex`.** fastembed's `embed` needs `&mut self`; the
trait gives `&self` so callers can share one behind an `Arc`. Serialising callers
costs nothing that parallelism would have won — the batch inside is what saturates
the cores.

**Remote discovers its width** with one probe request at startup rather than
having it configured. Asking an operator to type `1536` invites a wrong answer
that would not surface until vectors were stored at the wrong width.

**Remote places vectors by the response's `index`**, not arrival order. The API
does not promise sorted output, and getting it wrong attaches every vector to the
wrong item — retrieval that is confidently incorrect rather than broken. Tested
directly with a deliberately shuffled response.

**Every provider's output is validated** — one vector per text, each the
advertised width — because a silently short batch misaligns everything after it.

### The test that earned its place immediately

`tests/local_model.rs` loads the real model and asserts the property it was
**chosen** for: the same text embedded alongside different companions must come
back at cosine > 0.99999. That is the check the rejected dynamically quantized
model fails at 0.992, and without a test it would be a paragraph in a document
rather than something a future model change has to survive.

Its first version built a provider per test and **four of five failed** with
`Failed to retrieve model file 'model_optimized.onnx'` — cargo runs tests in
parallel threads and they were all downloading into one cache directory at once.
Fixed with a shared `LazyLock`, which is also how the server will hold it. The
product avoids the race differently: the image is built with the cache already
populated, so nothing downloads at runtime.

### Verified

- `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`: clean
- `cargo test -p kairos-embed`: **36 tests** — 31 unit, 5 against the real model
- `angreal test unit`: green across the workspace
- `angreal dev fetch-model`: `local/bge-small-en-v1.5-q (384d) ready`

### NOT verified — the machine stopped, and this is what is outstanding

**The Docker image was not built, so "the model is baked into the image" is
currently a code claim rather than a demonstrated one.** Nor did the
integration, e2e or UAT tiers run after the crate landed.

Docker Desktop's backend was SIGKILLed mid-session and has opened an error dialog
that needs dismissing by hand. The likely cause is mine: `target/` had grown to
**131 GB** across this session's builds and the host filled, which also produced a
transient `cc` linker failure. Deleting `target/debug/incremental` freed 39 GB —
so the disk is fine now, but Docker needs a human to restart it.

`cargo clean` freed **119.3 GiB** (137 GiB now free) and everything re-verified
from scratch afterwards: fmt clean, clippy clean, `angreal test unit` green,
`cargo test -p kairos-embed` 36 green including the five against the real model,
and `angreal dev fetch-model` re-populated the cache the clean removed. So the
disk cause is dealt with and the code is unaffected — but Docker Desktop's
daemon socket is still absent, because the crash dialog wants dismissing by hand.

Outstanding, in order, once Docker is back:

1. `docker build .` — confirm the model layer copies and the image starts with
   `KAIROS_EMBED_CACHE` populated and downloading disabled.
2. `angreal test integration`, `angreal test e2e`, `angreal test uat`.
3. Deploy that image to a `kind` cluster — which is also [[KAIROS-T-0188]]'s work
   and the natural place to do it once.

This task is **not** transitioned to completed on that account. The code and its
own tests are done; the image claim is not yet evidence.

### Deliberately not in this task

Nothing in `kairos-server` consumes the provider yet — no wiring into app state,
no `/readyz` reporting. That is [[KAIROS-T-0190]]'s, which is the first thing that
needs a vector. Adding the plumbing here would have meant writing a caller before
knowing what it wants.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] An embedding trait with local, OpenAI-compatible and deterministic-fake
      implementations — in a new `kairos-embed` crate, **not** `kairos-core`, which
      is pure by [[KAIROS-A-0009]] and may not do I/O
- [x] Local is the default and needs no configuration
- [x] The chosen local model, the image-size delta and cold-start cost are
      recorded in the Status Updates
- [x] Works on both `linux/amd64` and `linux/arm64` — the arm64 image built and
      ran the baked model end to end; amd64's link verified under emulation, with
      the full native image left to `release.yml`'s per-arch runners
- [x] Provider and model are recorded per stored vector (`ModelId`); a mismatch is
      reported rather than silently compared (`Mismatch`, with `is_fatal`)
- [x] A provider that is unreachable never fails a write — satisfied by
      [[KAIROS-T-0190]] and now true **by construction** rather than by care: no
      write path calls a provider at all. Embedding is a background sweep, so an
      unreachable provider cannot fail a create; and `build_embedding_service`
      logs at warn and starts the server anyway
- [x] The remote API key follows the existing-Secret pattern in the chart —
      **moved to [[KAIROS-T-0188]]**, which is the task that owns the chart. It
      was never this task's to do: there is no chart change here to hang it on,
      and splitting one `values.yaml` edit across two tasks would leave the
      chart half-configured in whichever landed first
- [x] `angreal test unit`, fmt and clippy green; the Docker-dependent tiers are
      **outstanding**, see the Status Updates

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

### 2026-09-24 — Docker came back: the tiers are green, the image build is not

All three Docker-dependent tiers now pass with the crate in place:

| tier | result |
|---|---|
| `angreal test integration` | 43 targets, all ok |
| `angreal test e2e` | 16 passed |
| `angreal test uat` | 22 journeys + the surface drift gate |

**The image build is still outstanding, and the reason is not the Dockerfile.**
It fails at step 2, resolving the `docker/dockerfile:1` frontend, with
`DeadlineExceeded: context deadline exceeded`. Docker Hub answers fine from the
host — `curl https://registry-1.docker.io/v2/` returns its expected 401 — but the
daemon cannot pull anything: `docker pull hello-world` times out too. The VM's
outbound network did not come back with it after the crash. A Docker Desktop
restart is the fix and it needs a person.

The compose tiers were unaffected only because `pgvector/pgvector:pg16` and
`dexidp/dex:v2.43.1` were already in the local image cache.

**A correction about the previous run.** I reported that build as having
succeeded. It had not: my command ended `docker build … ; echo "exit=$?"`, which
reports the *echo's* status, so a failed build looked clean. The log had nothing
in it but a connection error. This run writes the real exit code to a file first,
which is how the failure above was caught rather than repeated.

So the single outstanding item for this task is unchanged: build the image and
confirm `/var/lib/kairos/models` is populated and the server starts with
downloading disabled.

### 2026-09-24 — the image builds, and the model really is baked into it

The outstanding claim is now evidence. It took **four** builds, and the first
three each failed on a different real defect hidden behind the one before it.

#### Three defects, in the order they surfaced

1. **`cannot find -lstdc++`.** The ONNX runtime under `fastembed` is C++ and
   `rust:slim` carries `cc` but not `g++`. Adding this crate changed the image's
   native *build* dependencies and nothing said so until the linker did.

2. **Undefined references to `std::__cxx11::basic_string`.** `ort` does ask for
   the C++ runtime, but static linking is order-sensitive: `-lstdc++` passed
   before the archive that needs it is discarded. `-C link-arg=-lstdc++` appends
   it at the end.

   That fix went into **`.cargo/config.toml`, not the Dockerfile**, because CI
   compiles the server on `ubuntu-latest` and would have hit exactly the same
   wall. One mechanism for the image, CI and a developer on Linux, rather than
   three places to remember.

3. **`undefined reference to _M_replace_cold`.** A symbol that only exists in
   libstdc++ 13 and later: `ort`'s prebuilt ONNX runtime is built against it and
   Debian 12 ships GCC 12. **Verified rather than guessed** — the same crate
   links in 43 seconds inside `rust:1.93-slim-trixie` (GCC 14.2) and not at all
   on bookworm — so both stages moved to **trixie**.

   The runtime stage has to match the builder's Debian release rather than merely
   be recent, because the binary now carries a C++ runtime and an older
   libstdc++ there would fail at **startup**, in production, rather than at build.
   `libstdc++6` is installed explicitly for the same reason: the comment claiming
   `libpq5` was the only native runtime dependency stopped being true the moment
   a C++ runtime entered the binary.

#### What the finished image actually does

| | |
|---|---|
| image size | **369 MB** |
| `/var/lib/kairos/models` | **65 MB**, owned by the `kairos` user |
| `KAIROS_EMBED_CACHE` | `/var/lib/kairos/models` |
| downloading | **disabled** (`LocalConfig` default) |

And end to end, using nothing but the image and a real pgvector database:

```
$ docker run … kairos:t0189-verify seed-demo
seeded tenant 'demo' … 20 short codes

$ docker run … kairos:t0189-verify embed-backfill
backfilling with local/bge-small-en-v1.5-q (384d)
demo: 20 item(s) updated, 41 text(s) embedded in 0.4s — 20/20 items now current

$ docker run … kairos:t0189-verify embed-backfill
demo: 0 item(s) updated, 0 text(s) embedded in 0.0s — 20/20 items now current
```

That is the whole claim proven in one line: **a provider that reports ready with
downloading disabled can only have read the baked files.** Air-gapped operation
is not asserted, it is the only way that output can exist.

The image's own migrations also applied cleanly against pgvector on the way,
including [[KAIROS-T-0187]]'s `20260924000000`.

#### What is still not verified

`serve` itself was not exercised from the image, for a reason that is not a
product defect: the dev Dex advertises its issuer as `http://localhost:41558/dex`,
so a container fetching the discovery document is told to fetch JWKS from its own
loopback. That is a dev-fixture limitation. The Kubernetes tutorial
([[KAIROS-T-0187]]) already covers `serve` from a published image against a
reachable issuer.

**amd64** is checked separately — see below — because this machine is arm64 and
the release workflow builds each architecture on its own native runner.

### 2026-09-24 — amd64

Checked separately, because this machine is arm64 and the release workflow builds
each architecture on its own native runner rather than by emulation.

The risk worth checking is the link, not the logic — the three defects above were
all toolchain-shaped — so the check is the crate that links `ort`, built under
`--platform linux/amd64`:

```
x86_64
gcc (Debian 14.2.0-19) 14.2.0
    Finished `release` profile [optimized] target(s) in 2m 18s
```

Both architectures link on trixie. The full amd64 *image* is built natively by
`release.yml`'s two-runner matrix, which is where it belongs — an emulated
release build of the whole workspace would take hours and prove less than the
native one CI already does.