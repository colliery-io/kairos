---
id: focal-flight-level-graph-view
level: initiative
title: "Focal Flight-Level Graph View - Layered Canvas, Live Refresh, and Blocks Badges"
short_code: "KAIROS-I-0008"
created_at: 2026-08-29T14:24:02.985417+00:00
updated_at: 2026-08-29T15:48:26.230500+00:00
parent: KAIROS-V-0001
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/completed"


exit_criteria_met: false
estimated_complexity: XL
initiative_id: focal-flight-level-graph-view
---

# Focal Flight-Level Graph View - Layered Canvas, Live Refresh, and Blocks Badges Initiative

## Context **[REQUIRED]**

Promoted from backlog ticket KAIROS-T-0081 (UAT verdict: the graph experience feels "light… scattered… grab bag vs intentional"). Full investigation, external survey, and options analysis live in the archived ticket; summary:

- **There is no graph rendering at all.** The relationships explorer (`pages/search/relationships.rs`) renders one item's 1-hop neighborhood as five stacked Panels, all rendered even when empty; navigation is one-hop-at-a-time with a full remount per step.
- **Multi-hop traverse destroys structure by construction**: the recursive CTE computes `(id, depth)` but finishes with `SELECT DISTINCT id` (`kairos-db/src/search.rs`) — depth dropped, no edges on the wire — so a depth-5 traverse renders identically to a flat filter.
- **Server contract to build on**: cycle-safe traverse CTE (relationship/direction/depth params, `MAX_TRAVERSE_DEPTH=10`), `entity_directory` root resolution, and the strict type matrix (`kairos-core/src/graph.rs`): `parent` Strategy→Initiative→Task only; `supports` workflow→Doc/ADR; `informs` Doc/ADR→workflow; `supersedes` ADR→ADR; `blocks` workflow→workflow; parent and blocks acyclic.
- **Survey lessons** (van Ham & Perer "Search, Show Context, Expand on Demand"; Obsidian local graph; Jira/Azure/Linear restraint): local focal graphs are tools, global graphs are marketing; layered/Sugiyama beats force-directed for directed work graphs (deterministic positions build spatial memory); hierarchy as containment, not arrows; tiny drawn-edge vocabulary; red reserved for violated/at-risk.

## Goals & Non-Goals **[REQUIRED]**

**Goals:**
- One intentional focal graph that answers: *"what does this item depend on, what does it feed into, and where does it sit in the hierarchy?"*
- Server subgraph contract: nodes AND typed directed edges with depth (stop discarding what the CTE computes).
- Deterministic layered layout — identical positions across reloads; no force-directed layout anywhere.
- Reachable where people are: replaces `/search/relationships/:code` AND a full Graph tab on `/items/:code` (PO decision 2026-08-29).
- Live over WS like boards, using the T-0074 fine-grained pattern; deterministic layout keeps positions stable across refetches (PO decision 2026-08-29).
- Jira-style blocked-by/blocks count badges on board cards, in this initiative (PO decision 2026-08-29 — accepted XL).

**Non-Goals:**
- NO global everything-graph (explicitly declined — unanchored graphs answer no operative question).
- Docs/ADRs never drawn as canvas nodes in v1 — side panel only (PO decision 2026-08-29).
- No red/at-risk edge encoding in v1 — red stays reserved by convention until violated semantics are defined.
- No WYSIWYG graph editing; org-admin link/unlink affordances are retained as-is.

## Detailed Design **[REQUIRED]**

- **DB**: `traverse_edges` beside `traverse_ids` (`kairos-db/src/search.rs`) returning `(source_id, target_id, relationship, min-depth)` — keep the columns the walk already computes.
- **Core**: subgraph types (node: id/short_code/entity_type/title/status; edge: source/target/relationship/depth), reusing the Relationship enum.
- **Server**: `GET /api/{family}/{code}/graph?depth=N` — `entity_directory` root resolution (404 on dead refs), existing hydration, depth default 2, capped by `MAX_TRAVERSE_DEPTH`. Blocks-count rollup for board cards (blocked_by/blocks counts per item) rides the board items payload or a sibling endpoint (task decides; record choice).
- **Web canvas**: Leptos SVG — kairos-web's first layout code. Strategy | Initiative | Task as fixed layered columns (ItemType statically determines the layer; only barycenter ordering within columns). `parent` = containment/lane grouping; `blocks` = the ONLY drawn arrows; `supports`/`informs`/`supersedes` in a slim side panel, suppressed when empty. Default depth 2; +N badges expand in place; refocus keeps a visible history trail; browser back returns to prior focus. Hover = tooltip + incident-edge highlight; short code → `/items/:code`. Entity colors reuse the search pill palette; Aurora Dark tokens only.
- **WS live**: subscribe like boards (T-0074 fine-grained rendering); on ItemUpdated refetch the subgraph; deterministic layout means unchanged subgraphs produce unchanged positions.
- **Board badges**: neutral-accent count badges (no red) on cards showing blocked-by/blocks counts; click-through to the graph view.
- **Traverse demoted honestly**: keep the API and the search-page switch, relabel as result scoping ("Limit results to items reachable from…").
- **The view's question in the UI** as the page subtitle: "What does this depend on, what does it feed into, where does it sit?"

## Alternatives Considered **[REQUIRED]**

- **B. No canvas (Linear/Azure stance)** — tree + badges + click-to-reveal. Rejected as the whole answer: blocks edges are exactly the cross-cutting links a tree cannot show. Its strongest piece — board-card badges — is pulled INTO this initiative per PO decision.
- **C. Global everything-graph done right** — rejected: answers no operative question, nondeterministic layouts destroy spatial memory, highest cost for least daily value.

## Implementation Plan **[REQUIRED]**

Decomposed 2026-08-29 (PO-approved design decisions folded in):

1. **KAIROS-T-0088** — Server subgraph contract: `traverse_edges`, core types, `GET /api/{family}/{code}/graph`, client DTOs/method, integration tests.
2. **KAIROS-T-0089** — Web graph canvas core: SVG layered layout, containment lanes, blocks arrows, expand/refocus/hover, side panel, route swap.
3. **KAIROS-T-0090** — Item-detail Graph tab + WS live refresh + traverse relabel.
4. **KAIROS-T-0091** — Blocked-by/blocks board-card badges: rollup + web + click-through.
5. **KAIROS-T-0092** — e2e + fixture wave: graph spec, badge coverage, fallout sweep, full ladder.