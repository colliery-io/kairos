---
id: core-data-model-and-api-design
level: initiative
title: "Core Data Model and API Design"
short_code: "KAIROS-I-0001"
created_at: 2026-03-04T01:20:05.196799+00:00
updated_at: 2026-07-08T15:03:46.720482+00:00
parent: KAIROS-V-0001
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/completed"


exit_criteria_met: false
estimated_complexity: L
initiative_id: core-data-model-and-api-design
---

# Core Data Model and API Design

## Context

Kairos needs a PostgreSQL data model and API layer that supports the three-level Flight Levels system (Strategy -> Initiative -> Delivery), supporting documents with configurable templates, ADRs at any level, multi-tenant schema isolation, and ABAC authorization. This is the foundational initiative - everything else (MCP server, CLI, GUI) builds on top of the data model and API.

The data model must handle:
- **Organizational entities**: Tenants, teams (with Team Topologies type metadata), delivery streams
- **Workflow items**: Strategies, initiatives, tasks/bugs/tech debt - each with distinct phase lifecycles
- **Supporting documents**: Template-driven, children of any workflow item
- **ADRs**: Decision records, children of any entity, own phase lifecycle
- **Reference documents**: Company Vision (org root), Social Contract, Team Charter
- **Boards**: Strategy board (one per tenant), initiative boards (limited number), delivery boards (one per team)
- **Cross-level linkage**: Strategy -> Initiative -> Task hierarchy with direction-down/information-up flow
- **Bucket initiatives**: Standing capacity tracking items

## Goals & Non-Goals

**Goals:**
- Design the PostgreSQL schema for the public (shared) and tenant-scoped tables
- Define the entity relationships and hierarchy model
- Design the API surface (REST endpoints, request/response shapes)
- Define the ABAC authorization model and default policies per Flight Level
- Define the multi-tenant schema isolation pattern (how `org_{slug}` schemas work)
- Establish the phase state machine rules for each workflow item type
- Design the template system for supporting documents

**Non-Goals:**
- Implementation (this initiative produces the design; implementation is a follow-on)
- MCP server design (separate initiative, consumes the API)
- GUI/CLI design (separate initiatives)
- Migration tooling or deployment automation
- Ceremony/calendar features (phase 2)

## Key Design Questions

These are the questions this initiative needs to answer:

### Data Model
1. What tables exist in the public schema vs tenant schemas?
2. How are the three Flight Levels represented? Separate tables per type, or a polymorphic document table?
3. How does the parent-child hierarchy work for workflow items vs supporting documents vs ADRs?
4. How are boards modeled? First-class entities, or views over workflow items?
5. How does the template system work? Where are templates stored, how are they versioned?
6. How is full-text search implemented across tenant schemas?

### API Design
7. What does the REST API surface look like? Resource-oriented? Board-oriented?
8. How does tenant context flow through requests? Subdomain? Header? Path prefix?
9. How are phase transitions exposed? Explicit endpoint, or PATCH on status?
10. How does the API handle cross-level queries? (e.g., "show me all tasks across delivery teams for this initiative")

### Authorization
11. What attributes drive ABAC decisions? (user role, team membership, flight level, action type)
12. What are the default policies per level?
13. How are custom policies defined and stored?

### Multi-Tenancy
14. How is the tenant schema created and migrated?
15. How does connection routing work (schema selection per request)?
16. How is cross-tenant access prevented at the database level?

## Deliverables

- ERD / schema design for public and tenant schemas → `schema-design.md` (8 public tables, 19 tenant tables + 1 view)
- API endpoint inventory with request/response shapes → `api-design.md`
- ABAC policy model with defaults → KAIROS-A-0006 (board-scoped capabilities, whitelist, glob matching)
- Phase state machine definitions for all workflow item types → KAIROS-A-0002 (configurable boards replace hardcoded state machines)
- Template system design → KAIROS-A-0003 (reusable typed metadata + template association)
- Multi-tenant isolation design → KAIROS-A-0001 (schema-per-tenant, app-level provisioning)

## Design Phase Progress

### ADRs (all RATIFIED → decided, 2026-07-08, walkthrough with Dylan)

Ratification amendments: A-0004 gained the history retention sweeper (hot window → compaction → archive-then-prune); A-0005 gained the `/ws/events` WebSocket channel (LISTEN/NOTIFY fan-out; vision's no-push constraint amended) and OpenAPI via utoipa (`/api/openapi.json`, SDK generation); A-0009 simplified to a single `schema.rs`; A-0010/A-0012/A-0013 switched dev/test OIDC to **Dex** (Keycloak stays the production reference); A-0012 gained the **soak tier** with a synthetic workforce payload; A-0015 consumes **`aurora-dark` from crates.io** as a dependency.

- KAIROS-A-0001: Relational data model, graph relationships, multi-tenant schema isolation, FTS view
- KAIROS-A-0002: Configurable board columns and transitions
- KAIROS-A-0003: Template system with reusable typed metadata
- KAIROS-A-0004: Content versioning with optimistic concurrency
- KAIROS-A-0005: Resource-oriented API with graph query, subdomain tenancy, explicit transitions
- KAIROS-A-0006: Board-scoped capabilities, whitelist with glob matching
- KAIROS-A-0007: Unified search endpoint with composable query
- KAIROS-A-0009: Rust implementation stack — axum + Diesel (diesel-async), embedded migrations, workspace layout (added 2026-07-08)
- KAIROS-A-0010: OIDC/Keycloak integration — single realm, JWKS validation, JIT provisioning, per-client flows (added 2026-07-08)
- KAIROS-A-0011: MCP server — remote streamable HTTP at /mcp, in-process services (added 2026-07-08)
- KAIROS-A-0012: Testing & verification — real-infra tiers via angreal, agent completion gate (added 2026-07-08)
- KAIROS-A-0013: Deployment & ops — single image, reference compose (Caddy/Postgres/Keycloak), env config, health/metrics (added 2026-07-08)
- KAIROS-A-0015: V1 clients — Leptos CSR GUI served at / (Aurora Dark theme), `kairos` CLI over shared client crate (added 2026-07-08)

### Design Artifacts
- `schema-design.md` — Consolidated ERD: public schema (8 tables), tenant schema (19 tables + 1 view)
- `api-design.md` — Full API endpoint inventory with request/response patterns
- KAIROS-S-0006 — MCP tool surface: the frozen tool inventory agents/skills program against (added 2026-07-08)

### Design-completion pass (2026-07-08)
Dylan directed a full-design push so implementation can run via agents with minimal interaction. Gap analysis produced ADRs A-0009–A-0013 and A-0015 plus spec S-0006 (above). Decisions taken by Dylan: **diesel** over sqlx; **remote HTTP MCP** on the service; **full v1 client set** (MCP + CLI + Leptos GUI, Aurora Dark theme, served from server root). Remaining before implementation: human ratification of all draft ADRs (transition to decided), Aurora Dark token source pointer, and decomposition sign-off.