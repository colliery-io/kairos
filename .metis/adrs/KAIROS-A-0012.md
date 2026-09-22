---
id: 001-testing-and-verification-strategy
level: adr
title: "Testing and Verification Strategy"
number: 1
short_code: "KAIROS-A-0012"
created_at: 2026-07-08T11:28:44.418375+00:00
updated_at: 2026-07-08T15:00:39.077479+00:00
decision_date: 
decision_maker: 
parent: 
archived: false

tags:
  - "#adr"
  - "#phase/decided"


exit_criteria_met: false
initiative_id: NULL
---

# ADR-1: Testing and Verification Strategy

## Context

Most Kairos implementation will be executed by AI agents working through Metis tasks with minimal human interaction. That is only safe if "verified" is defined mechanically: agents need deterministic feedback loops and non-negotiable completion gates, or plausible-but-broken code lands. The risk core of the system is exactly the part types can't check — `search_path` tenant isolation, recursive-CTE traversal, tsvector search, capability glob matching against real SQL `LIKE`, and OIDC token validation. The repo already scaffolds an angreal harness (`test unit|integration|all`, `services up|down|reset|clean`, `db migrate|create-tenant|...`) and a compose file for backing services.

## Decision

**Four test tiers against real infrastructure, driven exclusively through angreal tasks, with a mechanical completion gate for every agent-executed task.** (Two further tiers were added by amendment: soak, 2026-07-08; UAT, 2026-09-22.)

### Tiers
1. **Unit (`angreal test unit`)** — `cargo test` on pure logic, no I/O. Lives primarily in `kairos-core`: transition rule evaluation, ABAC capability matching, short-code generation, search-request validation, error mapping. Fast enough to run on every loop iteration.
2. **DB integration (`angreal test integration`)** — against real Postgres from the compose stack. **The database is never mocked.** Covers: migrations (public + tenant trees from empty), tenant provisioning, schema isolation (negative tests proving tenant A cannot reach tenant B's rows through any endpoint), relationship graph + recursive traversal, the full search pipeline, optimistic-concurrency 409s, soft-delete cascade, capability checks including glob edge cases.
3. **API integration (same task)** — boots the axum app against the compose stack (including the **Dex** test issuer, per A-0010) and exercises it through `kairos-client` with real tokens: auth middleware (expired/wrong-audience/no-membership), every endpoint family in S-0005, WS event delivery, and MCP tool round-trips through `/mcp` per S-0006.
4. **E2E smoke (`angreal test e2e`, new task)** — full compose up, provision tenant, seed demo data, run the golden path (create strategy → initiative → decompose to tasks → transition → search → MCP session), plus a thin Playwright suite for the Leptos GUI's critical paths (login via Dex, board view, item create/transition, live WS update). Kept deliberately small; it gates releases, not every task.
5. **Soak (`angreal test soak`, added at ratification 2026-07-08 per Dylan)** — a long-running run against the compose stack driven by a synthetic **workforce payload**: a configurable simulated organization (N teams, humans and agent service-accounts) executing a realistic operation mix — creates, edits (with deliberate 409 collisions), transitions, searches/traversals, MCP sessions, and WS subscribers — at sustained rate for hours. Pass criteria asserted continuously: flat error rate, p95 for common ops within the vision's 50ms budget, stable memory and connection-pool metrics, retention sweeper keeping history bounded (A-0004), and tenant-isolation invariants holding under concurrency. Runs nightly and pre-release; never part of the per-task gate.
6. **UAT (`angreal test uat`, added 2026-09-22 per Dylan, KAIROS-I-0011)** — persona-driven user-acceptance journeys in `uat/`: a coding agent (a service account) over MCP and the real `kairos` CLI, engineers in the browser, admins over the CLI/API, each journey a story told once (serial, no retries) whose every acceptance step is narrated and records what was observed. Runs against the compose stack with a fresh seed by default, or against any deployment with `--server <url>` — journeys name everything they create `uat-<run>-…` and delete it in teardown; steps that need a fresh tenant or a deployment-admin token are skipped and say so. Every run renders `uat/reports/<run>/report.md` (+ `.json`): per journey a persona / step / observed / status table with screenshots and traces on failure — the artefact a product owner reads to accept a release. Journeys mirror the plugin skills' documented call sequences, so they double as the acceptance test of that documentation. A release/milestone gate like e2e, never part of the per-task gate; also run nightly in CI with the report uploaded.

### Fixtures
A `seed-demo` operation (exposed as `angreal db seed`) provisions a demo tenant with users, teams, boards, and representative items. Integration tests and skill verification both consume it, so "a running Kairos with known data" is one command.

### The agent completion gate
An agent may transition a Metis task to completed **only** when all of the following pass, with output recorded in the task document:
1. `cargo fmt --check` and `cargo clippy -- -D warnings` clean
2. `angreal test unit` and `angreal test integration` green
3. Every acceptance criterion on the task demonstrated (command + observed output)
4. New behavior carries new tests — a diff that adds endpoints/queries/tools without tests fails review by definition

### Skills verification
Plugin skills are verified as scenario runs against the compose stack with seeded data: the skill's documented workflow is executed end-to-end and the resulting board state asserted via `kairos-client`. The Phase D acceptance test (KAIROS-I-0002) is the composed version: bootstrap → to-initiative → decompose → implement → code-review against a live instance.

## Alternatives Analysis

| Option | Pros | Cons | Risk Level | Implementation Cost |
|--------|------|------|------------|-------------------|
| Compose-backed real-infra tiers (chosen) | Tests the actual risk core (SQL, isolation, auth); harness already exists; agents and CI share one entry point | Integration tier needs running services; slower than mocks | Low | Medium |
| testcontainers-rs per test | Hermetic, parallel-safe | Duplicates the compose harness; container-per-test slow; Keycloak-in-testcontainer flaky | Medium | Medium |
| Mock the DB layer | Fast | Worthless against the actual risks (search_path, CTEs, LIKE globs, tsvector); false confidence for agent-built code | High | Low |
| No mechanical gate (trust review) | Less ceremony | Unsupervised agents with no hard gate is exactly how mud ships | High | None |

## Rationale

1. **The risk lives in SQL and auth, not in Rust logic** — only real Postgres and a real OIDC issuer exercise it. Mocks would test the code agents were least likely to get wrong.
2. **Angreal as the single entry point removes drift.** Agents are instructed to run tasks, not raw commands; CI runs the same tasks; "works on my loop" equals "works in CI."
3. **The gate converts 'unleash' from hope to procedure.** A task isn't done because the agent says so; it's done because the gate passed and the evidence is in the task doc.
4. **Isolation gets negative tests** because schema-per-tenant is the product's hardest promise — it must be continuously proven, not assumed from design.

## Consequences

### Positive
- Agent loops (metis-ralph) get fast unit feedback and authoritative integration feedback with two commands
- Tenant isolation, authorization, and search — the three scariest subsystems — are permanently regression-tested
- Test infrastructure doubles as the local dev environment and the skills-verification harness

### Negative
- Integration suite needs Docker locally and in CI; runtime grows with coverage (mitigate: parallel test DB schemas per test where possible)
- The soak tier needs somewhere to run for hours (nightly CI machine or a standing dev box) and a maintained workforce-payload driver — it's a small product of its own

### Neutral (testing infra)
- Test/dev OIDC is **Dex** (A-0010): static-config users and clients, long-lived test tokens, instant startup. Per KAIROS-A-0016 (2026-07-10) no IdP is bundled in the product at all — Dex is a pure test fixture and doubles as the standing proof that Kairos works against any spec-compliant issuer

### Neutral
- `angreal test e2e` and `angreal db seed` are new tasks to add to the existing harness
- Playwright is the only non-Rust test dependency; it's confined to the E2E and UAT tiers (amended 2026-09-22)