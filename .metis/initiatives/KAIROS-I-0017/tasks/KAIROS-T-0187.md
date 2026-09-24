---
id: pgvector-the-extension-the-tenant
level: task
title: "pgvector: the extension, the tenant migration, and the two embedding tables"
short_code: "KAIROS-T-0187"
created_at: 2026-09-24T02:27:46.765540+00:00
updated_at: 2026-09-24T10:36:52.535495+00:00
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

Somewhere to put vectors. `pgvector` in each tenant schema, and the two tables
[[KAIROS-A-0021]] rule 2 implies: an item's **primary vector** and its **section
chunks**. No embedding is produced by this task — it builds the box.

Measured constraint: stock `postgres:16` ships `pg_trgm` and `plpgsql` and
nothing else. pgvector is an image change, not a `CREATE EXTENSION` away.

## Implementation Notes

### Technical Approach

A new tenant migration under `crates/kairos-db/migrations/tenant/`, following the
schema-per-tenant pattern the existing migrations use, creating:

- `item_embeddings` — one row per item: `item_id`, the composed primary vector,
  the provider and model that produced it, the dimension, the hash of the text
  that was embedded, `created_at`/`updated_at`. The hash is what makes
  re-embedding skippable and staleness detectable.
- `item_chunks` — many rows per item: `item_id`, ordinal, the literal heading
  text the chunk sat under (nullable — sliding-window chunks have none), the
  character range, the chunk text, its vector, and the same provider/model/hash
  columns.

Both are per-tenant by construction, which schema-per-tenant gives free and
which is the right isolation for embeddings.

**Two tables rather than one column** because they answer different questions —
"what is this item about" versus "which part of it matched" — and invalidate at
different rates. A Status Updates append invalidates one chunk; a title change
invalidates the primary vector.

`CREATE EXTENSION IF NOT EXISTS vector` belongs in the shared/public setup rather
than per tenant: an extension is database-scoped, and running it per tenant would
be a hundred no-ops and one race.

### Index choice

Do **not** add an HNSW or IVFFlat index in this task. Both need tuning against
real vector counts, and an IVFFlat index built on an empty table is worse than
none. Exact search is correct and fast enough at the corpus sizes measured here;
the index arrives with T-0190's backfill, when there is data to build it on, and
its parameters get recorded there.

### Dependencies

Requires a Postgres image carrying pgvector. That means:
- `angreal services up` — the compose Postgres becomes `pgvector/pgvector:pg16`
  or equivalent
- the integration and e2e test services, wherever they pin Postgres
- the CI workflow's service container
- the tutorial in `docs/src/tutorials/deploy-to-kubernetes.md`, which tells the
  reader to `create deployment postgres --image=postgres:16` and would now
  produce a deployment that cannot start

That last one matters: the tutorial was executed against a real cluster and is
promised to work. It has to be re-run, not just edited.

### Risk Considerations

- **Readiness.** `/readyz` reports database reachability. Decide whether a
  missing `vector` extension makes a tenant unready or merely un-embeddable, and
  state it. Recommendation: unready is too harsh — Kairos without vectors should
  still serve boards and lexical search, per rule 7's graceful degradation. So:
  log loudly at startup, refuse embedding work, serve everything else.
- Existing tenants get the migration on boot like any other; the tables arrive
  empty and T-0190 fills them.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] A tenant migration creates `item_embeddings` and `item_chunks` with
      provider, model, dimension and content-hash columns
- [x] `CREATE EXTENSION vector` runs once at database scope, idempotently
- [x] Every Postgres pin in the repository carries pgvector: compose, integration
      services, e2e, CI, and the Kubernetes tutorial
- [x] The Kubernetes tutorial is **re-executed**, not merely edited
- [x] ~~A database without the extension leaves Kairos serving boards and lexical
      search, with a loud startup log — not unready~~ — **dropped**, and the
      reasoning is in the Status Updates. [[KAIROS-A-0021]] rule 2 makes pgvector
      a requirement, not an option; what it gets instead is a pre-flight check
      that names the extension and what to install, before any migration runs
- [x] No vector index yet, and the reason is recorded
- [x] `angreal test` green, including `angreal test integration` against the new
      image

## Status Updates

*To be added during implementation*