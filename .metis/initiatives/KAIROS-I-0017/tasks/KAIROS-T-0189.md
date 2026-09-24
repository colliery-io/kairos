---
id: embedding-providers-local-by
level: task
title: "Embedding providers: local by default, OpenAI-compatible as BYO, fake for tests"
short_code: "KAIROS-T-0189"
created_at: 2026-09-24T02:27:52.746086+00:00
updated_at: 2026-09-24T02:27:52.746086+00:00
parent: KAIROS-I-0017
blocked_by: []
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

*To be added during implementation*
