---
id: web-team-page-view-edit-route-with
level: task
title: "Web: team page view/edit route with generalized editor"
short_code: "KAIROS-T-0086"
created_at: 2026-08-29T02:59:07.460235+00:00
updated_at: 2026-08-29T02:59:07.460235+00:00
parent: KAIROS-I-0007
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/todo"


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

- [ ] Pages render at stable path URLs; folders index their children; breadcrumbs navigate up.
- [ ] Edit/Preview + toolbar; version-checked saves with the 409 merge dialog; history rows written per save.
- [ ] Create page/folder, rename, move, soft-delete from the UI, permission-gated; charter shows no destructive controls.
- [ ] Unknown paths render a clean not-found; all markdown through the safe pipeline.
- [ ] Unit + lint + build green.

## Status Updates

- 2026-08-29: Created from the KAIROS-I-0007 decomposition (PO-approved; wave 2).
