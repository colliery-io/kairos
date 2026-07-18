---
id: 001-api-design-resource-oriented-with
level: adr
title: "API Design - Resource-Oriented with Graph Query"
number: 1
short_code: "KAIROS-A-0005"
created_at: 2026-03-04T03:06:09.398362+00:00
updated_at: 2026-07-08T15:00:22.606682+00:00
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

# ADR-5: API Design - Resource-Oriented with Graph Query

## Context

Kairos needs an HTTP API that serves multiple client types (CLI, GUI, MCP servers for AI agents). The API must support:

- CRUD operations on all entity types (strategies, initiatives, tasks, documents, ADRs)
- Board views (list all items on a board, grouped by column)
- Phase transitions (moving items between board columns)
- Cross-level graph traversal (e.g., "all tasks under this strategy", "everything blocking this initiative")
- Multi-tenant isolation (each request scoped to a tenant)
- Search across all entity types within a tenant

Four key decisions: API structure, tenant context, transition model, and cross-level queries.

## Decision

### 1. API Structure: Resource-Oriented with Board Views

Resource endpoints are the primary API surface. Each entity type has standard CRUD endpoints. Board endpoints exist as views over items.

**Resource endpoints (primary):**

```
# Strategies
GET    /api/strategies                    # List (filterable)
POST   /api/strategies                    # Create
GET    /api/strategies/{short_code}       # Read
PATCH  /api/strategies/{short_code}       # Update content
DELETE /api/strategies/{short_code}       # Delete

# Same pattern for /initiatives, /tasks, /documents, /adrs
```

**Board endpoints (views):**

```
GET    /api/boards                        # List all boards
GET    /api/boards/{id}                   # Board detail (columns, transitions)
GET    /api/boards/{id}/items             # All items on board, grouped by column
```

**Nested resource endpoints (common traversals):**

```
GET    /api/strategies/{short_code}/initiatives     # Initiatives under this strategy
GET    /api/initiatives/{short_code}/tasks           # Tasks under this initiative
GET    /api/initiatives/{short_code}/documents       # Documents supporting this initiative
GET    /api/{entity_type}/{short_code}/adrs          # ADRs under any entity
GET    /api/{entity_type}/{short_code}/history       # Content version history
```

**Transition endpoint:**

```
POST   /api/strategies/{short_code}/transition       # Move to a new column
       Body: {"to_column_id": "uuid"}
```

**Relationship management:**

```
GET    /api/{entity_type}/{short_code}/relationships  # All relationships for an item
POST   /api/relationships                             # Create a relationship
DELETE /api/relationships/{id}                         # Remove a relationship
```

**Search:**

```
GET    /api/search?q=authentication                   # Full-text search across all types
GET    /api/search?q=authentication&type=task          # Filtered by entity type
```

**Metadata:**

```
GET    /api/{entity_type}/{short_code}/metadata       # Get item's metadata
PATCH  /api/{entity_type}/{short_code}/metadata       # Update metadata values
GET    /api/metadata-definitions                       # List available definitions
POST   /api/metadata-definitions                       # Create new definition
```

**Templates:**

```
GET    /api/templates                                  # List available templates
POST   /api/templates                                  # Create custom template
GET    /api/templates/{id}                             # Template detail
```

**Organizational:**

```
GET    /api/teams                                      # List teams
POST   /api/teams                                      # Create team
GET    /api/delivery-streams                           # List delivery streams
POST   /api/delivery-streams                           # Create delivery stream
```

### 2. Tenant Context: Subdomain

Tenant is determined by the subdomain of the request:

```
acme.kairos.io/api/strategies          # Tenant: acme
widgets.kairos.io/api/boards           # Tenant: widgets
```

The server extracts the tenant slug from the `Host` header, resolves to the organization record in `public.organizations`, and sets the PostgreSQL `search_path` to `org_{slug}` for the duration of the request.

For local development / self-hosted: `localhost` with an `X-Tenant` header as fallback, or configurable single-tenant mode that skips tenant resolution entirely.

### 3. Phase Transitions: Explicit Endpoint

Moving an item between board columns is a deliberate action, not a field update:

```
POST /api/strategies/{short_code}/transition
{
  "to_column_id": "uuid-of-target-column"
}
```

The server validates:
- The item exists and belongs to a board
- A `board_transitions` row exists for `(board_id, current_column_id, to_column_id)`
- The user has permission (ABAC) to transition items on this board

Returns the updated item with its new column. Rejects with 422 if the transition is not allowed.

This is separate from `PATCH` which updates content/title (and is subject to optimistic concurrency per KAIROS-A-0004). Transitions don't increment the content version.

### 4. Cross-Level Queries: Structured Graph Query

Common traversals get dedicated nested endpoints (see above). For arbitrary graph queries, a structured JSON query endpoint:

```
POST /api/graph/query
{
  "from": {"short_code": "S-0001"},
  "traverse": ["parent"],
  "direction": "outbound",
  "depth": 3,
  "filter": {"entity_type": "task"}
}
```

**Fields:**

- `from`: Starting node, identified by short_code or id
- `traverse`: Relationship types to follow (parent, supports, informs, supersedes, blocks)
- `direction`: `outbound` (from -> child), `inbound` (child -> from), `both`
- `depth`: Maximum traversal depth (required, capped at a system limit to prevent runaway queries)
- `filter`: Optional filter on results - `entity_type`, metadata values, board column, etc.

The server translates this to a recursive CTE on `item_relationships`, then joins to the relevant entity tables to hydrate results.

**Examples:**

```json
// All tasks descended from strategy S-0001
{"from": {"short_code": "S-0001"}, "traverse": ["parent"], "direction": "outbound", "depth": 5, "filter": {"entity_type": "task"}}

// Everything blocking initiative I-0003
{"from": {"short_code": "I-0003"}, "traverse": ["blocks"], "direction": "inbound", "depth": 1}

// All ADRs supporting anything under strategy S-0001
{"from": {"short_code": "S-0001"}, "traverse": ["parent", "supports"], "direction": "outbound", "depth": 5, "filter": {"entity_type": "adr"}}
```

> **Superseded (2026-07-08):** the standalone `POST /api/graph/query` endpoint and the `GET /api/search` query-param endpoint above are superseded by the unified `POST /api/search` in KAIROS-A-0007, whose `traverse` capability carries these semantics forward unchanged.

### 5. Event Push: WebSocket Channel (added at ratification, 2026-07-08)

*Per Dylan: the UI must not poll. This supersedes the vision's "no real-time push in v1" constraint.*

```
GET /ws/events            # WebSocket upgrade; bearer token auth on connect
```

- **Thin events, not payloads.** The server pushes change notifications — `{event, entity_type, short_code, board_id, column_id?, actor, occurred_at}` for `item_created | item_updated | item_transitioned | item_deleted | relationship_changed | metadata_changed`. Clients re-fetch details through the REST API; the socket never carries content, so consistency and authorization stay in one place.
- **Tenant-scoped, read-model only.** The connection is bound to the resolved tenant; since reads are open tenant-wide (A-0006), every org member may subscribe. Optional client-side subscription filter by `board_id`.
- **Fan-out via PostgreSQL LISTEN/NOTIFY.** Mutating services emit a `NOTIFY kairos_events` (tenant-tagged) after commit; each server node LISTENs and forwards to its connected sockets. This keeps the server stateless-per-request, works unchanged if the deployment ever scales past one node, and adds no broker to the compose stack.
- **Delivery is best-effort.** Events are hints for UI freshness, not a durable stream: no replay, no ordering guarantee beyond per-connection FIFO. Clients reconcile by re-fetching on reconnect. (A durable event log is a future enhancement, not v1.)

### 6. OpenAPI Specification (added at ratification, 2026-07-08)

Every REST endpoint is specified via OpenAPI, generated from code with `utoipa` (axum-native derive on handlers and the shared DTO types from KAIROS-A-0015):

- Served at `GET /api/openapi.json`; Swagger UI mounted in dev builds
- The spec is a CI artifact per release, enabling client SDK generation (openapi-generator et al.) for languages beyond the shipped Rust `kairos-client`
- Drift is impossible by construction: the spec derives from the same handlers and DTOs that serve traffic; a handler without utoipa annotations fails review (A-0012 gate)
- The WebSocket channel is documented alongside (AsyncAPI-style section in the API docs) — OpenAPI covers the REST surface

## Alternatives Analysis

### API Structure

| Option | Pros | Cons | Risk Level | Cost |
|--------|------|------|------------|------|
| **Resource-oriented only** | Simple, standard REST | No board views, GUI must assemble boards client-side | Low | Low |
| **Board-oriented only** | Natural for GUI/kanban | Awkward for direct item access (CLI, MCP), items always require board context | Low | Low |
| **Hybrid** (chosen) | Both direct access and board views, serves all client types | More endpoints to maintain, two ways to access the same data | Low | Medium |

### Tenant Context

| Option | Pros | Cons | Risk Level | Cost |
|--------|------|------|------------|------|
| **Path prefix** (`/orgs/{slug}/...`) | Explicit, no DNS config, works with any HTTP client | Noisy URLs, tenant in every path, not how SaaS products typically work | Low | Low |
| **Subdomain** (chosen) | Clean URLs, standard SaaS pattern, customer expectation | Requires wildcard DNS or per-tenant DNS, slightly more complex routing | Low | Medium |
| **Header** (`X-Tenant`) | Cleanest URLs, simple routing | Hidden context, easy to forget, not self-documenting in logs/URLs | Medium | Low |

### Cross-Level Queries

| Option | Pros | Cons | Risk Level | Cost |
|--------|------|------|------------|------|
| **Nested resources only** | Simple, RESTful, pre-built | Can't express arbitrary traversals, new patterns require new endpoints | Low | Low |
| **Cypher DSL** | Maximum expressiveness | Parsing complexity, security surface (injection), overkill | Medium | High |
| **Structured JSON query** (chosen) | Typed, validatable, safe, expressive enough for real use cases | Less expressive than full Cypher, new query format to learn | Low | Medium |
| **Query parameters** | URL-friendly, cacheable | Can't express multi-hop traversals, limited filtering | Low | Low |

## Rationale

1. **Resource endpoints as primary serve all clients.** MCP agents and CLI need direct access by short code. GUI needs board views. Both are first-class.

2. **Subdomain tenancy is the expected SaaS pattern.** Customers understand `acme.kairos.io`. It's clean, self-documenting in URLs and logs, and maps naturally to schema isolation.

3. **Explicit transition endpoints prevent accidental state changes.** Moving an item between columns is a workflow action with validation rules. It should not be a side effect of a generic PATCH.

4. **Structured graph queries balance power and safety.** Common traversals get convenient nested endpoints. Complex queries use a typed JSON structure that's easy to validate and translate to CTEs. No query language parsing, no injection risk.

## Consequences

### Positive
- All client types (CLI, GUI, MCP) are well-served by the API surface
- Tenant context is natural and self-documenting
- Graph queries enable powerful cross-level analysis without pre-building every traversal pattern
- Transitions are deliberate actions with clear validation, not hidden in PATCH operations
- Search, metadata, history, relationships all have dedicated endpoints

### Negative
- Subdomain routing requires wildcard DNS configuration for deployment
- Graph query endpoint adds implementation complexity (CTE generation, result hydration across entity tables)
- More endpoints to document and maintain than a minimal API
- Local development needs a fallback tenant resolution mechanism

### Neutral
- Short codes are the primary identifier in URLs, not UUIDs. This is user-friendly but means short code uniqueness must be enforced within a tenant.
- Content updates (PATCH) and transitions (POST /transition) are distinct operations with different semantics