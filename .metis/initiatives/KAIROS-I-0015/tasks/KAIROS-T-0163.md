---
id: the-search-page-can-ask-for
level: task
title: "The search page can ask for archived work"
short_code: "KAIROS-T-0163"
created_at: 2026-09-23T11:30:04.957939+00:00
updated_at: 2026-09-23T13:11:30.214430+00:00
parent: KAIROS-I-0015
blocked_by: [KAIROS-T-0157]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: true
initiative_id: KAIROS-I-0015
---

## Parent Initiative

[[KAIROS-I-0015]]

## Objective

Let someone ask for archived work from the search page. Today the GUI cannot
express the question at all, which makes "visible for audit and search" false
for the surface most people actually use.

## Implementation Notes

**Blocked by [[KAIROS-T-0157]].**

`crates/kairos-web/src/pages/search/data.rs:67-69` — the GUI's `SearchFilter`
mirror is explicitly partial, with a comment saying `team_id` / `is_bucket` /
`include_deleted` "aren't in the T-0042 builder". The builder has to be able
to express it before anything else here is possible.

Then a **visible toggle** on the search page, not a URL-only parameter.
"Visible for audit" means discoverable by someone who did not read the docs.
Label it for what it does ("Include archived work"), and show archived hits
distinctly in the result list rather than mixed in unmarked.

### Vocabulary hazard

`kairos-web/src/pages/item.rs:269,315` already uses "archived" for the
**document editorial lifecycle** (`draft | review | published | archived`,
KAIROS-T-0078), which is unrelated — a published document can be editorially
archived while perfectly live. The GUI must not use one word for two things
on the same screen.

This initiative does not rename anything (see non-goals). Pick copy that
distinguishes them, and leave a comment recording the collision for whoever
does the eventual rename.

Aurora Dark tokens and the existing filter-chip patterns apply; follow the
entity-type chips already on the page.

## Acceptance Criteria

- [x] The GUI `SearchFilter` can express `include_deleted`.
- [x] A visible, labelled toggle on `/search`, off by default.
- [x] Archived hits are visually distinct from live ones.
- [x] The Relationships panel and the graph explorer mark archived
      neighbours (carried in from [[KAIROS-T-0158]]).
- [x] Copy does not collide with the document lifecycle's "archived"; a
      comment records the collision.
- [x] Default search results are unchanged with the toggle off.
- [x] `angreal test` green.

## Status Updates

**2026-09-23 — implemented.**

*The filter builder.* `SearchFilter` in `pages/search/data.rs` gains
`include_deleted: bool`, typed as the server types it but with
`skip_serializing_if` so an unasked search sends the **byte-identical
body** it sent before the field existed — that is what makes "default
results unchanged" a property of the wire rather than a promise. The
flag alone makes the filter non-empty (`is_empty()` is a whole-struct
comparison), which is the GUI half of what [[KAIROS-T-0157]] made
`is_constraining()` accept: "just show me what's been put away" is a
complete search, not a 400.

*The toggle.* A labelled `Switch` above the filters, wrapped in
`[data-testid="include-put-away"]`, off by default, with a sentence
under it saying what put-away work is and that hits are marked. Above
the entity chips deliberately — "visible for audit" is a claim about
someone who never read the docs.

*The marker.* `Hit` gained `archived_at` and a put-away hit renders a
solid-gold `.kairos-archived-badge` beside its title, sits on
`tr.kairos-search__hit--put-away` (inset surface + gold left edge), and
is counted in its group's caption ("7 on this page · 2 put away"), so
the shape of the answer is legible before the rows are read.

*The trap was real.* All four web mirrors were silently dropping the
new field: `Hit`, `search::data::RelatedItem`, `search::data::GraphNode`
and `item::api::RelatedItem`. Nothing failed to build. Each now carries
`archived_at` plus a decode test asserting both the marked and the bare
(live) shape — `related_item_mirror_carries_the_archived_marker` and
friends exist to hold that shut.

*T-0158's carried-in scope.* The Relationships panel badges every
put-away neighbour and, when it has one, prints the line that makes the
divergence from the rollup legible: *containment is a fact about the
record; progress is a fact about live work.* `children-progress` is
untouched and its doc comment now says live-only-on-purpose. On the
graph canvas an archived node gets `.kairos-graph__node--put-away` (inset
fill, dimmed entity stroke) and a gold "put away" label on its status
line; the `+N` arithmetic is untouched, as T-0158 asked. The
supporting-material side panel and the admin Manage-links rows badge
their archived ends too.

*Vocabulary.* "put away" everywhere, matching [[KAIROS-T-0164]]; never a
bare "archived". The collision is commented at the toggle, at the
`include_deleted` field, at the hit badge, at the graph node and on
`PanelRow.archived_at` — the last because the side panel is the one
place where a document's editorial `lifecycle: archived` renders as its
`status` **next to** the ADR-20 marker.

**Finding, fixed in passing.** The `.kairos-graph__*` block in
`app.css` referenced five custom properties aurora-dark does not define
(`--surface`, `--surface-raised`, `--text`, `--text-bright`,
`--text-dimmed`). An undefined var makes the declaration invalid at
computed-value time, and `fill` is inherited, so the graph's boxes and
labels were falling back to initial **black**. `angreal web lint` only
bans raw literals, so it never caught it. Renamed to the real tokens
(`--panel`, `--panel-2`, `--fg`, `--fg-bright`, `--muted`) — without
this the put-away marker would have been correct and invisible.

**E2E.** Two specs appended to `e2e/tests/archived.spec.ts` (now 5).
Fixture discipline held: no board is created, and everything either ends
archived or was archived to begin with.

## Notes carried in from [[KAIROS-T-0164]] — the GUI vocabulary is settled

**2026-09-23.** T-0164 resolved the "archived" collision on the item page,
and this task must match it rather than invent a second answer.

- **Call the ADR-20 state "put away"** in all user-facing copy. Never a bare
  "archived" anywhere a document's editorial `lifecycle:` badge can also
  appear — that is the collision (KAIROS-T-0078's
  `draft|review|published|archived`, which is unrelated to `deleted_at`).
- `.kairos-archived-badge` is the shared class: **solid** gold outline for
  the put-away state; the **dashed** badge is the editorial one.
- Wire names: entity DTOs carry `archived_at` (absent while live). Board
  columns carry `removed_at`, and only when asked for
  (`GET /api/boards/{id}?include_removed_columns=true`).
- Search result DTOs live in `crates/kairos-client/src/types_search.rs`.
  **Confirm a search hit actually carries `archived_at` before designing a
  marker for the result list** — [[KAIROS-T-0157]] is editing that file, and
  whether hits are marked is its acceptance criterion, not an assumption
  this task may make.
- Linking works now: any archived hit can link to `/items/:code` or
  `/activity/history/:code` and get a real page rather than an error.

## Additional GUI scope, carried in from [[KAIROS-T-0158]]

**2026-09-23.** T-0158 widened the relationship graph, and [[KAIROS-T-0164]]
(the item page) is already closed — so the remaining GUI consequences land
here. Treat these as part of this task.

- `RelatedItem.archived_at` and `GraphNode.archived_at` are both
  `Option<String>`, RFC 3339, **omitted when live** (so presence means
  archived). The **Relationships panel and the graph explorer must render
  them distinctly.** An unmarked archived neighbour is worse than a missing
  one, because the reader will act on it.
- The web mirrors (`kairos-web/src/pages/item/api.rs`,
  `pages/search/data.rs`) are partial structs, so they compile unchanged and
  **silently ignore the new field** until you add it. Nothing will fail to
  build; the marker will simply never appear.
- `GraphNode.degree` now counts archived neighbours, so the explorer's
  `+N = degree - edges shown` arithmetic still balances. Do not "fix" it.
- **The counterpart that must NOT change:** `children-progress` and the
  board cards' blocks counts stay live-only (ADR-20 rule 5). A panel showing
  "2 children" beside "1 of 1 done" is **correct**. Make the difference
  legible rather than reconciling it away. T-0158's formulation is worth
  putting in the UI copy: *containment is a fact about the record; progress
  is a fact about live work.*

## Also confirmed by [[KAIROS-T-0157]]

Search hits **do** carry `archived_at` — the API already marked them, and
T-0157 added markers to MCP (`render_search_results`) and the CLI search
table. So the open question this task was told to check is answered: design
the result-list marker.