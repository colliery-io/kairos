---
id: 001-unified-search-endpoint-with
level: adr
title: "Unified Search Endpoint with Composable Query"
number: 1
short_code: "KAIROS-A-0007"
created_at: 2026-03-04T13:21:30.388208+00:00
updated_at: 2026-07-08T15:00:25.999411+00:00
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

# ADR-7: Unified Search Endpoint with Composable Query

## Context

The Kairos API needs to support multiple query patterns: full-text search, structured filtering (by board, column, team, metadata), and graph traversal (walk relationships to find descendants, blockers, etc.). Earlier designs (KAIROS-A-0005) spread these across separate endpoints:

- `GET /api/search?q=...` for full-text
- `GET /api/{type}?board_id=...&column_id=...` for filtered lists
- `POST /api/graph/query` for graph traversal

This created three problems:

1. **Inconsistent filter patterns** — ad-hoc query params on GET endpoints, structured JSON on graph POST. Different clients implement filtering differently.
2. **Composition gap** — no way to combine capabilities (e.g., "full-text search within descendants of strategy S-0001 filtered to tasks").
3. **Hydration complexity** — graph queries return mixed-type UUIDs that need resolution across 5 entity tables. A unified query system can handle hydration as part of the response pipeline rather than as a separate problem.

The core tension: GET with complex filters is awkward (base64 JSON, endless query params), but POST for reads feels wrong. The insight is that **search is not CRUD** — it's a distinct operation that warrants its own endpoint.

## Decision

### One search endpoint, three composable capabilities

```
POST /api/search
```

All GET endpoints become simple point accessors (get by short code, list all with pagination). All complex querying goes through the search endpoint.

### Request Structure

```json
{
  "q": "authentication",
  "filter": {
    "entity_type": ["task", "initiative"],
    "board_id": "uuid",
    "column_id": "uuid",
    "team_id": "uuid",
    "task_type": ["bug", "tech_debt"],
    "is_bucket": false,
    "metadata": {
      "priority": "critical",
      "component": "auth*"
    },
    "created_after": "2026-01-01T00:00:00Z",
    "created_before": "2026-03-01T00:00:00Z",
    "include_deleted": false
  },
  "traverse": {
    "from": {"short_code": "S-0001"},
    "relationships": ["parent"],
    "direction": "outbound",
    "depth": 3
  },
  "sort": {"field": "created_at", "order": "desc"},
  "limit": 25,
  "offset": 0
}
```

All top-level fields are optional. At least one of `q`, `filter`, or `traverse` must be present.

`sort.field` is one of `created_at`, `updated_at`, `title` or `relevance`. The
default is `relevance` when `q` is present and `created_at desc` otherwise
(KAIROS-T-0186, under KAIROS-A-0021). `relevance` requires `q` — asking for it
without one is a 400, not a silent fallback — and is `ts_rank_cd` over the
matched rows. Every sort is tie-broken by `short_code` ascending, so the order is
total and pagination cannot tear on ties, which `ts_rank_cd` produces often.

### Three Capabilities

**Full-text search (`q`)**: Queries the `searchable_items` view using `ts_query` against the tsvector. Returns items where title or content matches.

**Structured filter (`filter`)**: Narrows results by entity attributes and metadata. All filter fields are optional. Multiple values in arrays are OR within a field. Fields are AND with each other. Metadata string values support glob patterns (`*`).

**Graph traversal (`traverse`)**: Walks `item_relationships` via recursive CTE from a starting node. Fields:
- `from`: Starting entity, identified by `short_code` or `id`
- `relationships`: Which relationship types to follow (`parent`, `supports`, `informs`, `supersedes`, `blocks`)
- `direction`: `outbound` (source→target), `inbound` (target→source), `both`
- `depth`: Maximum traversal depth (required, server-capped to prevent runaway queries)

### Composition

The three capabilities compose naturally:

- `q` alone → full-text search across all entities
- `filter` alone → structured query ("all critical bugs on board X")
- `traverse` alone → graph walk ("everything under strategy S-0001")
- `traverse` + `filter` → graph walk filtered ("tasks under strategy S-0001")
- `q` + `filter` → scoped text search ("search 'auth' in tasks")
- `q` + `filter` + `traverse` → "find text 'auth' in tasks descended from strategy S-0001"

Execution order: traverse first (if present) → filter applied to results → full-text search applied to results → sort → paginate.

### Response Structure

Results grouped by entity type, each containing fully-typed entities:

```json
{
  "results": {
    "strategies": [
      {"id": "...", "short_code": "S-0001", "title": "...", "content": "...", "board_id": "...", "column_id": "...", "hypothesis": "...", ...}
    ],
    "initiatives": [...],
    "tasks": [...],
    "documents": [...],
    "adrs": [...]
  },
  "total": 42,
  "limit": 25,
  "offset": 0
}
```

Empty type groups are omitted. Each entity is returned with all its typed fields — no lossy common-denominator projection.

### Hydration Strategy

When the query involves graph traversal or cross-type results:

1. **Traverse** (if present): Recursive CTE on `item_relationships` returns a set of UUIDs.
2. **Type resolution**: Join against `entity_directory` view to get `(id, entity_type)` for each UUID.
3. **Group by type**: Partition UUIDs into per-type sets.
4. **Hydrate**: Run one `SELECT * FROM {table} WHERE id IN (...)` per entity type (at most 5 queries, usually fewer).
5. **Filter**: Apply structured filter and full-text conditions.
6. **Sort and paginate**: Apply sort order and limit/offset to the combined results.

This is bounded at 5 hydration queries regardless of result size, and each returns properly typed data.

### Simplified GET Endpoints

With search handling all complex queries, GET endpoints become purely point access:

```
GET /api/strategies                     List all (+ ?limit=&offset= for pagination)
GET /api/strategies/{short_code}        Get one
GET /api/initiatives                    List all
GET /api/initiatives/{short_code}       Get one
GET /api/tasks                          List all
GET /api/tasks/{short_code}             Get one
GET /api/documents                      List all
GET /api/documents/{short_code}         Get one
GET /api/adrs                           List all
GET /api/adrs/{short_code}              Get one
GET /api/boards                         List all boards
GET /api/boards/{id}                    Board detail (columns, transitions)
GET /api/boards/{id}/items              All items on board, grouped by column
```

No filter params, no query complexity. Board items view is kept as a GET because it's scoped to a single board (not a cross-cutting query).

## Alternatives Analysis

| Option | Pros | Cons | Risk Level | Cost |
|--------|------|------|------------|------|
| **Per-endpoint GET filters** (query params) | Standard REST, cacheable | Inconsistent across endpoints, metadata filters awkward as params, no composition, complex URLs | Low | Medium |
| **POST /query per entity type** | Typed responses, filter in body | N endpoints instead of 1, no cross-type queries, graph traversal still separate | Low | Medium |
| **GET with base64-encoded JSON filter** | Single param, structured | Ugly, not human-readable, debugging pain | Low | Low |
| **Unified POST /api/search** (chosen) | One endpoint, composable capabilities, clean separation from CRUD GETs, solves hydration, extensible | Not cacheable (POST), one more concept to learn | Low | Medium |

## Rationale

1. **Search is not CRUD.** Querying across entity types, traversing graphs, and combining full-text with structured filters is a fundamentally different operation from "get strategy by short code." It deserves its own endpoint rather than being shoehorned into GET query params.

2. **One filter system, not five.** A single filter structure works everywhere — full-text, structured, graph. Clients learn one query language. The server implements one filter pipeline.

3. **Composition is the killer feature.** "All critical tasks blocking initiative I-0003" is a single query combining traverse + filter. With separate endpoints this requires client-side orchestration.

4. **Solves hydration cleanly.** The search endpoint owns the full pipeline: traverse → resolve types → hydrate per type → filter → sort → paginate. No N+1, no awkward multi-query client patterns.

5. **GET endpoints stay simple.** By moving all query complexity to search, GET endpoints become trivial point accessors. The API has two clear modes: "get a thing" (GET) and "find things" (POST /api/search).

## Consequences

### Positive
- Single query endpoint for all complex querying needs
- Three capabilities (text, filter, graph) compose freely
- Hydration is bounded at 5 queries regardless of result size
- GET endpoints are trivially simple — no filter parsing, no query param debates
- Filter structure is extensible — new filter fields don't require new endpoints
- Response groups by type — clients get properly typed data without common-denominator projections

### Negative
- POST /api/search is not cacheable at the HTTP level (mitigated by application-level caching if needed)
- One endpoint handling three query modes adds implementation complexity
- Pagination across mixed entity types is non-trivial (offset applies to the combined result set, not per-type)
- Clients must learn the search query structure

### Neutral
- The graph query endpoint from KAIROS-A-0005 is superseded by the `traverse` capability in search
- Separate `GET /api/search` endpoint from KAIROS-A-0005 is superseded
- Board items view (`GET /api/boards/{id}/items`) remains a separate GET because it's board-scoped, not a cross-cutting query