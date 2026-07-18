---
id: 001-relational-data-model-with-graph
level: adr
title: "Relational Data Model with Graph-Based Relationships"
number: 1
short_code: "KAIROS-A-0001"
created_at: 2026-03-04T01:26:22.217408+00:00
updated_at: 2026-07-08T15:00:02.039746+00:00
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

# ADR-1: Relational Data Model with Graph-Based Relationships

## Context

Kairos manages multiple entity types across three Flight Levels (strategies, initiatives, tasks/bugs/tech debt), supporting documents with configurable templates, ADRs, teams, delivery streams, and boards. These entities form a hierarchy (strategy -> initiative -> task) but also have cross-cutting relationships (ADRs can be children of anything, documents can attach to any workflow item, items can block each other).

The predecessor system (Metis) used a single polymorphic `documents` table with JSONB for type-specific fields and a `document_relationships` table for parent-child edges. This worked for a single-repo tool but is insufficient for a multi-tenant system with proper relational integrity.

Key constraints:
- No JSONB for domain data. All fields must be properly typed columns.
- No NoSQL patterns. The database schema should be self-documenting and constraint-enforced.
- Entities have distinct fields and phase lifecycles - strategies have hypotheses, initiatives have complexity, tasks have task types.
- Documents and ADRs can be children of any entity type.
- Cross-level queries (e.g., "all tasks under this strategy") must be efficient.

## Decision

**Separate tables per entity type with a shared UUID space and a single graph relationship table.**

### Entity Tables (per tenant schema)

Each entity type gets its own table with fully typed columns:

- `strategies` - id, short_code, title, content, board_id, column_id, hypothesis, version, created_by, updated_by, deleted_at, created_at, updated_at
- `initiatives` - id, short_code, title, content, board_id, column_id, complexity, is_bucket, bucket_type, version, created_by, updated_by, deleted_at, created_at, updated_at
- `tasks` - id, short_code, title, content, board_id, column_id, task_type (task|bug|tech_debt), team_id, version, created_by, updated_by, deleted_at, created_at, updated_at
- `documents` - id, short_code, title, content, template_id, version, created_by, updated_by, deleted_at, created_at, updated_at
- `adrs` - id, short_code, title, content, board_id, column_id, decision_maker, decision_date, version, created_by, updated_by, deleted_at, created_at, updated_at
- `item_history` - id, item_id, version, title, content, edited_by, edited_at (append-only content history, see KAIROS-A-0004)
- `activity_log` - id, actor_id, action, entity_id, entity_type, details, occurred_at (audit trail for transitions, relationship changes, capability grants/revocations, see KAIROS-A-0004)

All entity tables support **soft delete** via `deleted_at` timestamp. Deleting a parent cascades soft-deletion to children (walked via `parent` relationships). A background process handles hard-delete cleanup of expired soft-deleted items.

Note: `board_id` and `column_id` reference the configurable board system (see KAIROS-A-0002). Workflow items no longer have hardcoded phase enums - their current state is determined by which board column they occupy. ADRs have their own board for lifecycle management. Documents use a simple status field as they don't live on boards.

### Organizational Tables (per tenant schema)

- `teams` - id, name, slug, team_type (stream_aligned|platform|enabling|complicated_subsystem), deleted_at
- `team_members` - team_id, user_id, joined_at (which users belong to which teams)
- `delivery_streams` - id, name, slug, description, deleted_at
- `boards` - id, name, slug, board_level (strategy|initiative|delivery|adr), team_id (nullable), deleted_at
- `templates` - id, name, slug, content, is_system_default

### Relationship Table (per tenant schema)

A single table for all entity relationships:

```
item_relationships
  - source_id     (uuid)
  - target_id     (uuid)
  - relationship  (parent|supports|informs|supersedes|blocks)
  CHECK (source_id != target_id)
```

- **parent**: Hierarchical ownership. Source is parent of target. Strategy is parent of initiative. Initiative is parent of task.
- **supports**: A document or ADR that supports a workflow item. Target supports source. A PRD supports an initiative. An architecture doc supports an initiative.
- **informs**: Reference relationship. Source informs target. Company vision informs the strategy board. Social contract informs the initiative level.
- **supersedes**: ADR replacement chain. Source supersedes target.
- **blocks**: Dependency tracking. Source blocks target. Task A blocks task B.

All entity IDs share a UUID space. The relationship table contains only UUIDs and the relationship type - no type discriminators. Entities know what they are in their own tables. Cycle prevention (e.g., A blocks B blocks A) is enforced at the application level.

### Public Schema

Shared across all tenants:
- `organizations` - id, name, slug, settings
- `users` - id, external_id (from OIDC), email, display_name
- `organization_members` - user_id, organization_id, role
- `system_templates` - id, name, slug, content (default templates shipped with the system)

### Hierarchy Traversal

The workflow hierarchy (vision -> strategy -> initiative -> task) is encoded as `parent` relationship edges. Cross-level queries use recursive CTEs on the `item_relationships` table. For example, "all tasks under strategy X" walks `parent` edges from strategy to initiatives to tasks.

### Multi-Tenant Schema Isolation

Each tenant gets a PostgreSQL schema named `org_{slug}`. All entity, relationship, board, and organizational tables live in the tenant schema. The public schema holds only cross-tenant data (`organizations`, `users`, `organization_members`, `system_templates`).

- **Schema creation and migration**: Application-level operations, not dev tooling. When a new tenant is provisioned via the API, the server creates the `org_{slug}` schema and runs all migrations against it.
- **Connection routing**: Axum middleware extracts tenant slug from the `Host` header (subdomain), resolves to org in `public.organizations`, sets `search_path = org_{slug}` on the connection for the request duration. Shared connection pool with per-request search path switching.
- **Cross-tenant prevention**: Queries only run within the tenant's schema via search path. No cross-schema joins. Public schema access is filtered by the current tenant's org ID. The isolation boundary is the schema itself — no application-level row filtering needed.

### Full-Text Search

A PostgreSQL `VIEW` unions searchable content (id, short_code, entity_type, title, content, tsvector) across all entity tables (strategies, initiatives, tasks, documents, adrs). Search queries hit this view with `ts_query` against the computed `tsvector`. Individual entity tables maintain GIN indexes on their `tsvector` columns so the view's queries are index-backed. If performance requires it, the view can be promoted to a `MATERIALIZED VIEW` with its own GIN index and periodic refresh. Search is always tenant-scoped (queries run within a tenant schema).

## Alternatives Analysis

| Option | Pros | Cons | Risk Level | Cost |
|--------|------|------|------------|------|
| **A: Single polymorphic table** | Simple queries, one CRUD path | Requires JSONB for type-specific fields, no DB-level type constraints, unclear schema | Medium | Low |
| **B: Tables per category** (workflow_items, documents, adrs) | Moderate separation, fewer tables | Still polymorphic within workflow_items (strategy vs task in same table), compromise that satisfies neither goal fully | Medium | Medium |
| **C: Separate tables per type + relationship graph** (chosen) | Full type safety, self-documenting schema, proper constraints per type, flexible graph for hierarchy | More tables, hierarchy queries require joins via relationship table, UUID coordination across tables | Low | Medium |
| **D: Separate tables with polymorphic FK columns** | Full type safety, direct FK enforcement | Document/ADR tables need N FK columns (one per possible parent type), CHECK constraints grow with each new type, rigid | Low | High |

## Rationale

1. **Type safety at the database level.** Each entity type has distinct fields (strategies have hypotheses, tasks have task_types). Separate tables with typed columns make the schema self-documenting and let the database enforce constraints that application logic would otherwise need to handle.

2. **The relationship table decouples hierarchy from entity structure.** Adding a new entity type (or a new relationship type) never requires changing existing entity tables. The graph is orthogonal to the entities.

3. **No JSONB.** A single polymorphic table requires JSONB for type-specific fields. This violates the constraint of fully typed, schema-enforced data.

4. **Recursive CTEs on one table are efficient.** PostgreSQL handles recursive CTEs well. Walking `parent` edges in `item_relationships` is simpler than joining across N entity tables with polymorphic FKs.

5. **Option D (separate FK columns) doesn't scale.** Every time a new entity type is added, the document and ADR tables need a new nullable FK column and the CHECK constraint needs updating. The relationship table approach is open for extension without modification.

## Consequences

### Positive
- Schema is self-documenting - each table's columns describe exactly what that entity type needs
- Database constraints enforce type-specific rules (e.g., `task_type` enum on tasks, `phase` enum per type)
- Relationship graph is flexible for future relationship types without schema changes
- Cross-level queries are a single recursive CTE regardless of depth
- No JSONB anywhere in the domain model

### Negative
- More tables to migrate and maintain
- "What is entity X?" requires checking multiple tables (or a union view) if you only have a UUID
- Application must coordinate UUID uniqueness across tables (UUIDs make this trivial in practice)
- Relationship integrity (e.g., "a task's parent must be an initiative") is enforced in application logic, not DB constraints

### Neutral
- Short code generation needs a cross-table sequence or application-level counter per type
- Full-text search indexes are per-table; cross-type search requires querying multiple indexes