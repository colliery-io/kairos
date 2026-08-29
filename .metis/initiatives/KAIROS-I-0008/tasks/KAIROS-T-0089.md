---
id: web-graph-canvas-core-layered-svg
level: task
title: "Web graph canvas core: layered SVG, containment lanes, blocks arrows, expand/refocus"
short_code: "KAIROS-T-0089"
created_at: 2026-08-29T14:25:02.816740+00:00
updated_at: 2026-08-29T15:08:18.078775+00:00
parent: KAIROS-I-0008
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0008
---

# Web graph canvas core: layered SVG, containment lanes, blocks arrows, expand/refocus

## Parent Initiative

[[KAIROS-I-0008]] — Focal Flight-Level Graph View. Depends on KAIROS-T-0088 (the subgraph contract).

## Objective

kairos-web's first graph rendering: replace the body of `/search/relationships/:code` with a deterministic layered SVG canvas — Strategy | Initiative | Task columns, parent as containment, blocks as the only drawn arrows, depth-2 default with +N expand-in-place, refocus with a history trail, and the supports/informs/supersedes side panel.

## Implementation Notes

- **Layout as pure code**: new `pages/search/graph_layout.rs` with NO leptos imports — takes `(nodes, edges, focus)` and returns positioned geometry. Native-unit-testable (determinism is an AC; prove it with a test, not a screenshot). ItemType statically assigns the column (Strategy | Initiative | Task); within a column, order by barycenter of already-placed neighbors, ties broken by short code (total determinism). Parent containment: initiatives render as bands; their tasks group under them in the task column (lane grouping, NOT drawn edges).
- **Canvas component** (`pages/search/graph.rs`, mounted from `relationships.rs`'s route): plain SVG elements in Leptos — `<svg>`, `<g>`, `<rect>`, `<path>` with `marker-end` arrowheads for blocks edges only. Node = rounded rect with short code (mono), title (truncated), status pill dot; entity colors from the search pill palette (search.rs:40–49); Aurora Dark tokens only (lint enforces). No pan/zoom in v1 — the depth bound keeps subgraphs small; overflow scrolls in the panel (page must not scroll horizontally: wrap in `overflow-x: auto`).
- **Interactions**: hover = tooltip (title + status) + incident-edge highlight via CSS class toggle; click node body = refocus; click short code = navigate `/items/:code`. Refocus keeps the trail: focus is the route param, the visited trail rides a `?trail=` query so browser back returns to the prior focus (in-app navigation only — memory token, A-0015). `+N` badge (from `degree` minus shown edges) fetches that node's `?depth=1` subgraph and MERGES into the current node/edge stores — no remount, no lost expansion (fine-grained rendering discipline from T-0074: pre-created signals, no Effect writing its own source).
- **Side panel**: supports/informs/supersedes listed with direction labels + lifecycle chips (reuse T-0078 colors), suppressed entirely when empty — no always-rendered empty panels anywhere in the new view.
- **Keep**: org-admin link/unlink affordances (port from relationships.rs); the parent breadcrumb becomes redundant (the canvas shows lineage) — drop the 10-GET upward walk (search/data.rs:330–353) once the canvas renders lineage.
- **Page subtitle** states the question: "What does this depend on, what does it feed into, where does it sit?"
- **Watch**: Leptos 0.8 lessons — signals created at render time (never in event handlers), views built owned/'static, `StoredValue::new_local` for !Send values.

## Acceptance Criteria

## Acceptance Criteria

- [x] `/search/relationships/:code` renders the SVG graph: three fixed columns, parent as containment lanes (never arrows), blocks as the only drawn arrows with direction markers; no force-directed layout anywhere.
- [x] Layout is deterministic — a native unit test on `graph_layout` proves identical positions for identical input, including barycenter tie-breaks.
- [x] Default depth-2; nodes with undisplayed neighbors show `+N` (from the contract's `degree`); expanding merges in place without remount or lost prior expansions.
- [x] Refocus updates the route param and appends to a visible trail; browser back returns to the prior focus.
- [x] Hover shows title/status and highlights incident edges; short code navigates to `/items/:code`; org-admin link/unlink retained.
- [x] supports/informs/supersedes appear only in the side panel, suppressed when empty; entity colors match the search pill palette; red used nowhere.
- [x] The old five-panel explorer and its sequential breadcrumb walk are gone; unit + lint + build green.

## Status Updates

- 2026-08-29: Created from the KAIROS-I-0008 decomposition (design PO-approved).
- 2026-08-29: COMPLETE. `graph_layout.rs` (pure, no leptos): fixed columns by type; barycenter ordering (strategies by code → initiatives by mean strategy row → tasks grouped contiguously under their first visible initiative parent, orphan group last, all ties by short code); containment lanes computed for BOTH strategy→initiative and initiative→task bands (uniform algorithm); blocks arrows as ready-made bezier path strings (cross-column and same-column bow-out cases); `hidden_neighbors = degree − shown edges` computed per node. Native tests prove determinism (including input-order shuffling) plus columns/lanes/arrows/+N. `graph.rs`: GraphView (standalone — T-0090 mounts it on the detail tab) with base LocalResource + `expansions: RwSignal<Vec<GraphResponse>>` merged in a Memo (expansion = depth-1 fetch of that node, merged in place — no remount, prior expansions kept); hover → incident-edge highlight + native SVG `<title>` tooltip; node body click refocuses (route param + `?trail=` query, history entry so back works); code text → /items; +N badge expands. Side panel from supports/informs/supersedes edges with direction labels, suppressed when empty. ManagePanel (org admin) ports create-link and rebuilds unlink as an edge list from the relationships read (the graph contract carries no edge ids — recorded). relationships.rs is now a thin route shell with the question as subtitle; the old five panels and the 10-GET parent-chain walk (`data::parent_chain`, `item_summary`) are deleted. Mirrors: `GraphResponse`/`GraphNode`/`GraphEdge` in search/data.rs + decode test. `.kairos-graph__*` styles in app.css (tokens only; red nowhere).
- 2026-08-29: Verified: layout unit tests green, decode test green, `angreal test unit` clean workspace-wide, `angreal web lint` clean, `angreal web build` succeeds. Browser walk of expand/refocus/hover rides T-0092's graph.spec.