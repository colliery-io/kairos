---
id: server-subgraph-contract-traverse
level: task
title: "Server subgraph contract: traverse_edges, core types, graph endpoint, client"
short_code: "KAIROS-T-0088"
created_at: 2026-08-29T14:24:59.275235+00:00
updated_at: 2026-08-29T14:52:31.838214+00:00
parent: KAIROS-I-0008
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0008
---

# Server subgraph contract: traverse_edges, core types, graph endpoint, client

## Parent Initiative

[[KAIROS-I-0008]] — Focal Flight-Level Graph View. First task; everything else consumes this contract.

## Objective

The wire contract a real graph needs: stop discarding what the traverse CTE already computes. `traverse_edges` in kairos-db, subgraph types in kairos-core, `GET /api/{family}/{code}/graph?depth=N` on the server, DTOs + method in kairos-client — integration-tested.

## Implementation Notes

- **kairos-db** (`src/search.rs`): `traverse_edges` beside `traverse_ids` — same recursive CTE, but SELECT the edge rows the walk visits: `(source_id, target_id, relationship, depth)` with min-depth per edge (an edge reachable at depths 2 and 3 reports 2). Same cycle safety, same relationship/direction/depth params, same `MAX_TRAVERSE_DEPTH=10` cap (kairos-core/src/search.rs:47). Include edges BETWEEN two visited nodes even when the edge itself wasn't the discovery path (the "cross-link" edges are exactly what the panels view couldn't show) — one follow-up query over `item_relationships` filtered to the visited id-set is acceptable if the CTE shape fights this; record the choice.
- **kairos-core**: subgraph types — `SubgraphNode { id, short_code, entity_type, title, status, degree }` — `degree` = the node's TOTAL live-edge count in `item_relationships` (one grouped query over the visited set), so the canvas can show `+N` where `N = degree − edges shown` without a second round-trip (status = board column name for workflow items; lifecycle for documents — reuse the T-0078 vocabulary split), `SubgraphEdge { source_id, target_id, relationship, depth }`. Reuse the existing Relationship enum; no new rule logic.
- **kairos-server**: `GET /api/{family}/{code}/graph?depth=N` — root via `entity_directory` (404 on dead refs, matching graph.rs `resolve_entity`), depth default 2, clamp to `MAX_TRAVERSE_DEPTH`, hydrate nodes through the existing search hydration path. All five relationship types in the response (the client decides what to draw). Response: `{ focus: <short_code>, nodes: [...], edges: [...] }`. openapi registered; open tenant-wide read like the other graph reads.
- **kairos-client**: `types_graph.rs` DTOs + `get_item_graph(family, code, depth)` method.
- **Watch**: serde_urlencoded cannot flatten — explicit query struct for `depth` (T-0021 lesson). `touch crates/kairos-db/src/lib.rs` is NOT needed (no migration).

## Acceptance Criteria

## Acceptance Criteria

- [x] `traverse_edges` returns typed directed edges with min-depth; cycle-safe on the seeded blocks web; unit/integration-tested in kairos-db (depth bounding, direction, relationship filtering, cross-links between visited nodes).
- [x] `GET /api/{family}/{code}/graph?depth=N` returns focus + hydrated nodes (short_code, entity_type, title, status) + edges; 404 on dead refs via entity_directory; depth defaults 2 and clamps at MAX_TRAVERSE_DEPTH; openapi registered.
- [x] Workflow node status is the board column name; document nodes carry lifecycle (two-vocabulary split per A-0018).
- [x] kairos-client DTOs + method; client_roundtrip/openapi gates stay green.
- [x] Server integration test covers the contract on seeded-shape data: depth-2 default, blocks edges present, supports/informs edges present, parent edges present, depth respected.

## Status Updates

- 2026-08-29: Created from the KAIROS-I-0008 decomposition (design PO-approved: full detail tab, side-panel docs/ADRs, WS live, badges in-initiative, red deferred).
- 2026-08-29: COMPLETE. Deviation recorded: the subgraph service lives in `kairos-db/src/graph.rs` (as `item_subgraph`), NOT search.rs, and the data types live there too rather than kairos-core — graph.rs already owns `GraphError`, `parse_entity_type`, and the `Neighbor` precedent (plain data carriers at the db boundary; core stays rules-only). Shape: (1) recursive walk both-directions/all-relationships → `(id, MIN(depth))`; (2) hydration via `entity_directory` (live-only) joined to a per-family status union (column name for workflow, lifecycle for docs, `off-board` for boardless ADRs) + correlated live-neighbor `degree` count; (3) ALL live edges among the visible set (cross-links included) with `depth = max(endpoint depths)` — the min view depth at which both ends are visible. Server: `GET /api/{entity_type}/{short_code}/graph` in meta/relationships.rs (resolve_family_item 404s dead/mismatched refs), depth default 2 clamped to MAX_TRAVERSE_DEPTH, explicit `GraphQuery` struct (serde_urlencoded lesson), openapi registered. Client: `types_graph.rs` (GraphResponse/GraphNode/GraphEdge) + `get_item_graph(kind, code, depth)`.
- 2026-08-29: Verified: kairos-db `focal_subgraph_contract` test (min-depth per node, cross-link I2→T3 present, soft-deleted node AND its edges excluded, live-only degree, A-0018 status split, short-code ordering, depth-1 bound) + meta.rs endpoint section (default depth 2, clamp 200→10, focus/edges/degree, family-mismatch 404). `angreal test unit` + `angreal test integration` fully green.