---
id: web-team-page-view-edit-route-with
level: task
title: "Web: team page view/edit route with generalized editor"
short_code: "KAIROS-T-0086"
created_at: 2026-08-29T02:59:07.460235+00:00
updated_at: 2026-08-29T13:53:26.103440+00:00
parent: KAIROS-I-0007
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0007
---

# Web: team page view/edit route with generalized editor

## Parent Initiative

[[KAIROS-I-0007]] — Team Landing Pages. Depends on KAIROS-T-0083; pairs with KAIROS-T-0085.

## Objective

`/teams/:slug/pages/{path...}`: render a team page's markdown, edit it with the proven editor pattern (Edit/Preview + markdown toolbar + version-checked saves + 409 merge dialog), manage pages/folders (create, rename, move, soft-delete) within the permission model.

## Implementation Notes

- Route resolves path segments against the page tree (slug path); folders render an index of children.
- **Generalize the editor**: extract ContentEditor's save/merge machinery behind a save callback (family/short-code coupling noted at editor.rs:30–38) OR a parallel TeamPageEditor sharing the merge dialog; markdown toolbar = buttons inserting syntax at the cursor (heading/bold/italic/list/link/code). Markdown stays the source of truth (v1 decision — no live WYSIWYG).
- Saves send expected version; 409 opens the keep-mine/take-theirs/merge dialog with details.current.
- Create/rename/move/delete affordances for team members/org admins; charter protection surfaced (no rename/move/delete controls on it).
- Breadcrumb back to the team page and up the folder path.

## Acceptance Criteria

## Acceptance Criteria

- [x] Pages render at stable path URLs; folders index their children; breadcrumbs navigate up.
- [x] Edit/Preview + toolbar; version-checked saves with the 409 merge dialog; history rows written per save.
- [x] Create page/folder, rename, move, soft-delete from the UI, permission-gated; charter shows no destructive controls.
- [x] Unknown paths render a clean not-found; all markdown through the safe pipeline.
- [x] Unit + lint + build green.

## Status Updates

- 2026-08-29: Created from the KAIROS-I-0007 decomposition (PO-approved; wave 2).
- 2026-08-29: COMPLETE. Editor GENERALIZED (not forked): new `pages/editor.rs` `MarkdownEditor` owns edit/preview, the A-0004 versioned save, the 409 keep-mine/take-theirs/merge dialog, and the NEW markdown toolbar (H/B/I/list/link/code — cursor insertion via NodeRef + HtmlTextAreaElement, UTF-16-safe splicing since JS selection indexes are code units; element value set before the signal so the cursor survives the re-render). Saves go through a `Saver` callback (`Rc<dyn Fn(title, content, version) -> SaveFuture>`, held in `StoredValue::new_local` — !Send by design on wasm). Item's `ContentEditor` is now a thin wrapper binding `api::update_content`; the 409-parsing PATCH was extracted as `item::api::patch_versioned` and reused by `teams/api::save_page_content`. New route `/teams/:slug/pages/*path` (`pages/teams/doc.rs`): slug-path resolution against the flat tree, folder indexes with a create form (kind/slug/title), breadcrumbs (team → ancestors → current), PageView with version pill + Edit gate, StructurePanel (rename, move-select offering root + every folder except self/descendants, two-click delete navigating up on success), charter shows content-edit only + an explanatory note. Unknown team and unknown path each render clean not-founds.
- 2026-08-29: Found + fixed a real gap while building the move UI: `rename_move_page` only refused SELF-parenting — moving a folder under its own descendant created a cycle. kairos-db now walks the new parent's ancestor chain and refuses with BadParent; kairos-db/tests/team_pages.rs asserts both the self and descendant cases.
- 2026-08-29: Verified: `angreal test unit` green, `angreal web lint` clean, `angreal web build` succeeds, `angreal test integration` 32/32 targets ok (history-per-save was already integration-asserted in T-0083's server test). Playwright walk of the edit + 409 merge arrives with T-0087.