---
id: 001-rust-implementation-stack-axum
level: adr
title: "Rust Implementation Stack - Axum + Diesel"
number: 1
short_code: "KAIROS-A-0009"
created_at: 2026-07-08T11:28:20.649599+00:00
updated_at: 2026-07-08T15:00:34.567765+00:00
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

# ADR-1: Rust Implementation Stack - Axum + Diesel

## Context

The Kairos service (Rust per KAIROS-V-0001) needs its concrete implementation stack decided before agents build it. Constraints from prior decisions bound the choice:

- Hand-written DDL is the source of truth (KAIROS-S-0004) — no ORM-generated schema
- Tenant provisioning creates schemas and runs migrations **at runtime from application code**, not dev tooling (KAIROS-A-0001)
- Per-request `search_path` switching on pooled connections (KAIROS-A-0001, A-0005)
- Recursive CTEs, tsvector search, and dynamic filter composition (KAIROS-A-0007) require dropping to raw SQL where the ORM ends
- The GUI is Leptos served by the same binary (KAIROS-A-0015), so the whole product is one Rust workspace

## Decision

**axum + tokio + tower for the HTTP layer; Diesel (via `diesel-async`) for database access; `diesel_migrations` with embedded migrations for runtime provisioning.**

### HTTP layer
- `axum` on `tokio`, `tower`/`tower-http` middleware for tenant resolution (Host header → `search_path`), OIDC bearer validation (KAIROS-A-0010), tracing, and CORS
- The server binary serves `/api/*` (JSON), `/mcp` (KAIROS-A-0011), and the Leptos frontend at `/` (KAIROS-A-0015)

### Database layer
- `diesel` with `diesel-async` (`AsyncPgConnection`) and a `bb8` pool — async end-to-end, no `spawn_blocking` around queries
- A single `kairos-db/src/schema.rs` (simplified from a two-module split at ratification, 2026-07-08): tenant tables declared unqualified and resolved through `search_path`; public-schema tables declared schema-qualified (`public.users`, `public.organizations`, …) in the same file. Tenant tables are identical across schemas, so one declaration serves every tenant — the active tenant is selected by `SET search_path` on the checked-out connection (set on checkout, reset on return to pool)
- Typed Diesel queries for CRUD, boards, capabilities; **raw SQL via `diesel::sql_query` is the sanctioned escape hatch** for recursive-CTE traversals, `searchable_items` view queries, and the composed search pipeline (KAIROS-A-0007). The DDL is hand-written anyway — SQL at the edges is consistent, not a smell
- Enums stored as TEXT with CHECK constraints (per S-0004) map to Rust enums via small `FromSql`/`ToSql` impls

### Migrations
- Two embedded migration trees compiled in with `embed_migrations!`: `migrations/public/` and `migrations/tenant/`
- Server startup runs pending public migrations; tenant provisioning (`POST /api/admin/tenants`) creates `org_{slug}`, pins `search_path`, runs the tenant tree; a fleet operation re-runs the tenant tree across all schemas (already anticipated by the `angreal db migrate-tenants` task)

### Workspace layout
```
crates/
├── kairos-core     # domain logic: board rules, transitions, ABAC checks, short codes (no I/O)
├── kairos-db       # diesel schema, models, queries, embedded migrations
├── kairos-server   # axum binary: routes, middleware, MCP endpoint, serves Leptos at /
├── kairos-client   # typed HTTP client for the API (shared by CLI and integration tests)
├── kairos-cli      # `kairos` CLI (clap) over kairos-client
└── kairos-web      # Leptos frontend (KAIROS-A-0015)
```

### Error handling
- `thiserror` enums in `kairos-core`/`kairos-db`; a single `ApiError` in `kairos-server` implementing `IntoResponse`, producing the S-0005 error envelope (`{"error": {"code", "message", "details"}}`) with status mapping: 409 version conflict, 422 invalid transition, 403 capability denied, 404 not found, 400 validation

## Alternatives Analysis

| Option | Pros | Cons | Risk Level | Implementation Cost |
|--------|------|------|------------|-------------------|
| axum + Diesel (chosen) | Mature ORM, strongest compile-time query checking, embedded migrations fit runtime provisioning, decision-maker's preferred stack | Async requires `diesel-async` (community crate); dynamic search composition needs raw-SQL escape hatch | Low | Medium |
| axum + sqlx | Compile-checked raw SQL matches hand-written DDL, async-native | Macro checking needs live DB or offline cache in CI; thinner model layer | Low | Medium |
| axum + SeaORM | Async-native, entity codegen | Heaviest abstraction over hand-designed DDL; weakest dynamic-schema story | Medium | Medium |

## Rationale

1. **Decision-maker preference with no disqualifier.** Diesel is mature, its compile-time guarantees are the strongest of the options, and nothing in the Kairos design rules it out. (Decided by Dylan 2026-07-08 over an sqlx recommendation.)
2. **Embedded migrations satisfy the provisioning ADR.** `embed_migrations!` + programmatic `run_pending_migrations` against a `search_path`-pinned connection is exactly what app-level tenant provisioning needs.
3. **One `schema.rs` fits schema-per-tenant.** Tenant schemas are structurally identical, so Diesel's unqualified table names resolve through `search_path` — multi-tenancy costs nothing at the type level.
4. **The escape hatch is bounded and named.** Search/traverse was always going to be raw SQL under any stack; declaring `sql_query` sanctioned for exactly that module keeps the rest typed.

## Consequences

### Positive
- One Rust workspace covers server, domain, DB, CLI, and GUI — a single toolchain for agents to build against
- Compile-time query verification catches drift between `schema.rs` and queries
- Tenant provisioning and fleet migration are ordinary library calls, testable in integration tests

### Negative
- `diesel-async` is community-maintained — version-pin it; upgrades are deliberate tasks
- `schema.rs` must be kept in sync with the hand-written DDL by convention (regenerate via `diesel print-schema` against a migrated dev DB; wire as an angreal task)
- The search pipeline lives in raw SQL — it carries the heaviest integration-test burden (KAIROS-A-0012)

### Neutral
- Diesel enums-as-TEXT matches the CHECK-constraint style already in S-0004
- `kairos-client` doubles as the integration-test client