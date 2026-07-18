---
id: service-implementation-foundations
level: initiative
title: "Service Implementation - Foundations, Data Core, API"
short_code: "KAIROS-I-0003"
created_at: 2026-07-08T15:03:56.965834+00:00
updated_at: 2026-07-11T01:48:33.089621+00:00
parent: KAIROS-V-0001
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/completed"


exit_criteria_met: false
estimated_complexity: XL
initiative_id: service-implementation-foundations
---

# Service Implementation - Foundations, Data Core, API Initiative

## Context

Implements the Kairos service from the ratified design corpus (all decided 2026-07-08): schema/DDL per KAIROS-S-0004, API surface per KAIROS-S-0005, MCP tool surface per KAIROS-S-0006, under ADRs KAIROS-A-0001..A-0013. Covers roadmap milestones **M0 (Foundations), M1 (Data core), M2 (API surface)**. Successor to the design initiative KAIROS-I-0001 (completed).

Execution model: tasks are run primarily by agents (metis-ralph loops). **Every task completion is gated by KAIROS-A-0012**: `cargo fmt --check` + `cargo clippy -- -D warnings` clean, `angreal test unit` + `angreal test integration` green, and each acceptance criterion demonstrated with the command + output recorded in the task document. Design questions are settled — an agent that believes it needs to deviate from an ADR must stop and surface it, never improvise.

## Goals & Non-Goals

**Goals:**
- M0: repo foundations — git, cargo workspace (6 crates per A-0009), compose dev stack (Postgres 16 + Dex per A-0010/A-0013), extended angreal harness, CI, plugin manifests + rendered references
- M1: data core — migrations, tenant provisioning, `schema.rs`/models, board rules, ABAC, optimistic concurrency + history, relationship graph, search/traverse pipeline, retention sweeper, isolation negative tests
- M2: API surface — auth/tenant middleware, all S-0005 endpoint families, unified search, `/ws/events`, OpenAPI via utoipa, `kairos-client`, full API integration suite

**Non-Goals:**
- MCP endpoint and skills (KAIROS-I-0002, starts after M2)
- CLI and GUI (KAIROS-I-0004, blocked by this initiative)
- Kubernetes/Helm, horizontal scaling, durable event streams (post-v1 per ADRs)

## Detailed Design

All design lives in the ratified corpus — this initiative adds none:
- **A-0009** stack & workspace: axum/tokio/tower, diesel + diesel-async + bb8, single `schema.rs`, embedded migration trees, `thiserror` → `ApiError` envelope
- **A-0001/S-0004** schema, **A-0002** boards, **A-0003** templates/metadata, **A-0004** versioning + retention sweeper, **A-0006** ABAC, **A-0005/A-0007/S-0005** API + WS events + OpenAPI, **A-0010** OIDC (Dex in dev/test), **A-0012** testing tiers + gate, **A-0013** deployment/config/observability

## Testing Strategy

Per KAIROS-A-0012 (decided): unit in `kairos-core`; DB + API integration against the compose stack (Postgres + Dex), database never mocked; E2E smoke + Playwright at release gates; nightly soak with the workforce payload (harness built in M5/KAIROS-I-0002 timeframe). The per-task agent gate is non-negotiable.

## Alternatives Considered

None at initiative level — alternatives were adjudicated in the ADRs (all decided 2026-07-08).

## Implementation Plan

Tasks created at decompose (2026-07-08), sequenced by `blocked_by` edges:

- **M0 (6 tasks)**: git init/corpus commit → workspace scaffold ∥ compose stack → angreal harness extensions → CI skeleton; plugin manifests + references (independent after git)
- **M1 (10 tasks)**: public migrations → tenant migrations + provisioning → schema.rs/models → {board rules, ABAC, concurrency+history+short codes} → relationship graph → search/traverse pipeline → retention sweeper → isolation negative suite
- **M2 (~8 tasks, decomposed when M1 is underway)**: middleware stack, endpoint families, search endpoint, WS events, OpenAPI, kairos-client, integration suite completion

## Progress Log

- **2026-07-08**: Initiative created post-ratification. M0+M1 decomposed into tasks with acceptance criteria and A-0012 gates; M2 decomposition deferred until M1 is in flight.
- **2026-07-10**: **M1 COMPLETE (10/10)** and M2 underway (2/8). Serial+paired agent waves: T-0007 ✓ public migrations · T-0008 ✓ tenant provisioning (spec fix: S-0004 says 21 tenant tables, not 22) · T-0009 ✓ diesel layer (single schema.rs; board_member_capabilities UNIQUE→composite PK, S-0004 annotated) · T-0010 ✓ board rules · T-0011 ✓ ABAC (LIKE-escape hardening) · T-0012 ✓ write path (v1 baseline snapshots; prefix = sanitized slug) · T-0013 ✓ graph (informs restricted to doc/adr→items in v1; board refs = future modeling decision) · T-0014 ✓ search (websearch_to_tsquery; ≤5-query hydration asserted; 200-item fan-out ~3.5ms) · T-0015 ✓ retention sweeper (S3 target = typed stub) · **T-0016 ✓ isolation suite: NO leaks across the full attack battery** · T-0017 ✓ middleware (Dex aud=client-id documented; prod Keycloak uses audience mapper) · T-0018 ✓ entity endpoints (document create requires parent_short_code; sync-bridge r2d2 pool in server). Full gate green: 12 integration targets. Docker outage mid-wave recovered cleanly.
- **2026-07-08/09**: Execution began. **T-0001 ✓** (git repo, initial commit `199e958`). Wave 1 ran as three parallel agents: **T-0002 ✓** (`d7b8b4e` — six-crate workspace, toolchain 1.93.0, all gates green; note: aurora-dark ships as `colliery-io-aurora` on crates.io, wired via package rename), **T-0003 ✓** (`37d3cf3` — postgres:16 + dex v2.43.1; **Dex issuer is `http://localhost:5558/dex`** — 5556/5557 occupied on this host; **Dex lacks client_credentials** → password grant with `svc@kairos.test` documented until Keycloak in M2/M5), **T-0006 ✓** (`04e1a08` — plugin manifests validate, references rendered via `scripts/render-references.sh`; validator warnings carried forward to I-0002). Working constraint recorded: **no Homebrew; services and tooling containerized or rustup-native**. T-0004 (angreal harness) launched.