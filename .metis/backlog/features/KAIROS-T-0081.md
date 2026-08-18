---
id: graph-view-focal-layered-flight
level: task
title: "Graph view: focal, layered flight-level graph — depth-bounded, expand-on-demand"
short_code: "KAIROS-T-0081"
created_at: 2026-08-16T14:59:20.498818+00:00
updated_at: 2026-08-16T14:59:20.498818+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/backlog"
  - "#feature"


exit_criteria_met: false
initiative_id: NULL
---

# Graph view: focal, layered flight-level graph — depth-bounded, expand-on-demand

## Objective

UAT verdict: the graph experience feels "light… scattered… grab bag vs intentional." Replace the relationships explorer with **one intentional graph view** anchored to a focal item, answering a stated question — *"what does this item depend on, what does it feed into, and where does it sit in the hierarchy?"* — using a deterministic layered layout, depth-bounded with expand-on-demand.

## Backlog Item Details

### Type
Feature

### Priority
P2 — real UAT feedback, but the boards/team surfaces (T-0075–T-0080) are the daily-use fixes.

### Business Justification
- **User Value**: A graph that answers a question instead of displaying membership; dependency webs (blocks) become visible across initiatives, which no tree view can show.
- **Effort Estimate**: L — one new wire contract (DB + core types + endpoint) plus kairos-web's first SVG layout/interaction code.

## Current State (why it feels scattered — root causes, not seeding density)

- **There is no graph rendering at all** — no SVG canvas, no layout code in kairos-web. The "relationships explorer" (pages/search/relationships.rs) renders one item's 1-hop neighborhood as five stacked Panels (Lineage :251–267, Children :268–275, Blocked by :276–283, Blocks :284–291, Supporting material :292–302). All five render even when empty (:122–124). Edge direction is prose captions and text arrows (:240–243, :268).
- **Navigation is one-hop-at-a-time**: "explore" re-mounts the whole view on the clicked neighbor (:29–41); the only persistent context is the parent breadcrumb, built by up to 10 sequential upward GETs (search/data.rs:330–353). Children-of-children, siblings, and cross-links between shown neighbors are invisible.
- **Multi-hop traverse destroys structure by construction**: the search page's traverse mode (search.rs:352–377) hits the recursive CTE which computes (id, depth) but finishes with `SELECT DISTINCT id FROM walk` (kairos-db/src/search.rs:299–348 — depth dropped, root excluded); hydration regroups by entity type; results render as five type-grouped tables sorted `created_at desc`. A depth-5 traverse renders identically to a flat filter query.
- **Server contract to build on**: cycle-safe traverse CTE with relationship-type/direction/depth params, capped at MAX_TRAVERSE_DEPTH=10 (kairos-core/src/search.rs:47); entity_directory root resolution with 404 on dead refs; strict type matrix (kairos-core/src/graph.rs:139–167): `parent` Strategy→Initiative→Task only; `supports` workflow→Doc/ADR; `informs` Doc/ADR→workflow; `supersedes` ADR→ADR; `blocks` workflow→workflow; parent and blocks acyclic.
- Key constraint: today's wire formats carry **membership only** — no edges, no depth — so the server must return edge lists before any true graph can be drawn.

## What Other Tools Do (survey summary)

**Global graphs are marketing; local graphs are tools.** Obsidian's global vault graph is for orphan-finding; its **local graph** (focal note, depth slider, depth 2 sweet spot) is the one people use. Roam's full-vault hairball is the cautionary tale. The canonical model is van Ham & Perer's *"Search, Show Context, Expand on Demand"* (InfoVis 2009). PM tools converge on restraint: **Jira Advanced Roadmaps** shows numbered blocked-by/blocks badges (red only on real conflict) plus a separate purpose-built dependencies report; **Azure DevOps** draws dependency lines only on click; **Linear ships no graph at all** (relations in the sidebar) — a deliberate scope judgment, with third-party visualizers proving the demand anyway. Layout: **layered/Sugiyama beats force-directed for directed work graphs** — deterministic positions build spatial memory; force layouts reshuffle every render. Hierarchy is best encoded as **containment**, not as another edge type; keep drawn-edge vocabulary tiny; reserve red exclusively for violated/at-risk.

## Options Considered

**A. Focal flight-level graph (RECOMMENDED)** — server subgraph endpoint returning nodes AND edges (the CTE already computes depth — stop discarding it); Leptos SVG with **Strategy | Initiative | Task as fixed layered columns** (ItemType statically determines the layer — no layer-assignment algorithm needed, only barycenter ordering within columns); parent edges as containment/lane grouping; **blocks edges as the only drawn arrows**; supports/informs/supersedes in a side panel; default depth 2; +N badges expand in place; refocus keeps a history trail. *Pros*: directly answers the UAT critique; Kairos's strict type matrix makes layered layout unusually cheap and fully deterministic; reuses the existing CTE/hydration; all-Rust (no JS layout lib). *Cons*: first SVG/layout competency in kairos-web; a new wire contract; layout quality needs iteration on dense blocks webs.

**B. No canvas (Linear/Azure stance)** — hierarchy tree + board badges + click-to-reveal. *Rejected as the whole answer*: blocks edges are exactly the cross-cutting links a tree cannot show, and UAT asked for the graph to be intentional, not absent. **Its strongest piece — Jira-style blocked-by/blocks badges on board cards — should be a follow-on ticket.**

**C. Global everything-graph done right** — org-wide clustered map. *Rejected*: unanchored global graphs answer no operative question (Roam); nondeterministic layouts destroy spatial memory; highest cost for least daily value. We explicitly **decline to build a global graph**.

## Recommendation (v1 sketch)

- **DB**: `traverse_edges` beside `traverse_ids` (kairos-db/src/search.rs) returning (source_id, target_id, relationship, min-depth) — keep the columns the walk already computes.
- **Core**: subgraph types (node: id/short_code/entity_type/title/status; edge: source/target/relationship), reusing the Relationship enum.
- **Server**: `GET /api/{family}/{code}/graph?depth=N` — entity_directory root resolution (404 on dead refs), existing hydration, depth default 2, capped by MAX_TRAVERSE_DEPTH.
- **Web**: replace the body of `/search/relationships/:code` (same route) with a Leptos SVG canvas: three fixed columns (Strategy | Initiative | Task); parent = containment lanes; blocks = solid directional arrows; supports/informs/supersedes in a slim side panel (empty state suppressed); barycenter ordering for deterministic layout; hover = tooltip + highlight incident edges; click = refocus with history trail (no remount); short code → `/items/:code`; entity colors reused from search's pill palette (search.rs:40–49); org-admin link/unlink retained; Aurora Dark; **red reserved for at-risk/violated only** (unused in v1, reserved by convention).
- **Demote traverse honestly**: keep the API and the search-page switch, relabel as result **scoping** ("Limit results to items reachable from…") — stop presenting it as the graph story.
- **Put the view's question in the UI** as the page subtitle: "What does this depend on, what does it feed into, where does it sit?"

## Acceptance Criteria

- [ ] `GET /api/{family}/{code}/graph?depth=N` returns the focal item + neighborhood as hydrated nodes (short_code, entity_type, title, status) and typed directed edges (source, target, relationship, depth); root via entity_directory with 404 on dead refs; depth defaults 2, capped at MAX_TRAVERSE_DEPTH.
- [ ] `/search/relationships/:code` renders an SVG graph: Strategy/Initiative/Task fixed layered columns; parent shown as containment/lane grouping (not arrows); blocks as solid directional arrows; **no force-directed layout anywhere**.
- [ ] Layout is deterministic: identical node positions across reloads for the same subgraph.
- [ ] Default depth-2 view; nodes with undisplayed neighbors show a +N badge; clicking expands in place without remounting or losing the view.
- [ ] Refocusing preserves a visible history trail (parent-chain breadcrumb + visited-focus trail); browser back returns to prior focus.
- [ ] Hover shows title/status and highlights incident edges; short code navigates to `/items/:code`; org-admin link/unlink retained.
- [ ] supports/informs/supersedes never drawn as canvas edges; side panel suppressed when empty (no always-rendered empty panels anywhere).
- [ ] Search traverse switch relabeled as result scoping with no behavior change; POST /api/search semantics untouched.
- [ ] Entity colors match the search pill palette; Aurora Dark tokens respected; red not used for any healthy state.
- [ ] Server tests cover the edge-list contract (depth bounding, cycle safety, direction, relationship filtering); web smoke test covers render + expand + refocus on seeded data.

## Open Questions

- Also reachable as a tab on `/items/:code` in v1, or is the route swap sufficient?
- Documents/ADRs as canvas nodes behind a toggle, or side panel permanently?
- Live-refresh over WS like boards (T-0074 pattern), or manual refresh for v1?
- Board-card blocked-by/blocks badges (Option B's strongest piece): separate follow-on ticket, or pulled in at the cost of XL?
- When a blocks edge crosses from a done/blocked item: introduce the red at-risk encoding in v1 or defer until "violated" semantics are defined?

## Status Updates

- 2026-08-16: Created from UAT feedback; design via investigation + external survey (design workflow, session ffc0d1f9).
