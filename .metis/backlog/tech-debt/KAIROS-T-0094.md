---
id: team-pages-offer-root-level-page
level: task
title: "Team pages: offer root-level page/folder creation (deferred from v1)"
short_code: "KAIROS-T-0094"
created_at: 2026-08-29T15:51:45.318008+00:00
updated_at: 2026-09-25T02:26:36.566704+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#tech-debt"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: NULL
---

# Team pages: offer root-level page/folder creation (deferred from v1)

## Objective

Give team members a way to create ROOT-level pages and folders on a team's documentation tree — the one create surface KAIROS-T-0086 deliberately left out of v1 (creation exists only inside folder indexes).

## Backlog Item Details

### Type
Tech Debt

### Priority
P3 — the scaffold's root sections (Charter, Support Processes, Documentation) cover the common shapes; this only bites when a team wants a NEW top-level section.

### Technical Debt Impact
- **Current Problems**: The API supports root creation (`POST /api/teams/{id}/pages` with no `parent_id` — integration-tested in kairos-server/tests/team_pages.rs), but no UI offers it: folder indexes carry the only create form, so users can't add a top-level sibling of Documentation without an API call. Recorded as a v1 decision on KAIROS-T-0086.
- **Benefits of Fixing**: Teams can shape their own tree top level; removes the "the API can, the UI can't" asymmetry.
- **Risk Assessment**: None operationally; mild UX confusion when someone looks for the affordance.

## Implementation Notes

- Smallest honest fix: reuse `CreateForm` (pages/teams/doc.rs) with `parent_id: None` — mount it under the Documentation panel on `/teams/:slug` (member/org-admin gated via the same whoami check) or on a small root index view. The form already handles kind/slug/title and surfaces the typed 422s (SLUG_CONFLICT etc.).
- Remember NULL-parent sibling-slug uniqueness is enforced by the COALESCE index (T-0082) — a root "charter" duplicate 422s correctly; nothing new server-side.
- e2e: one step in teampages.spec (create a root folder as bob, see it in the tree).

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] A member/org-admin can create a root-level page or folder from the team UI; non-members see no affordance.
- [x] Root slug conflicts surface the server's 422 in the standard Alert pattern.
- [x] teampages.spec covers the root-create path; ladder green for touched tiers.

## Status Updates

- 2026-08-29: Ticketed — recorded v1 deferral from KAIROS-T-0086 ("folder indexes are the create surface; creating new ROOT nodes is deliberately not offered in v1").
### 2026-09-25 — the smallest honest fix, which is the one the notes proposed

`CreateForm` now takes `parent_id: Option<String>` and is mounted a second time
under the Documentation panel on `/teams/:slug` with `parent_id=None`. The server
needed nothing: `POST /api/teams/{id}/pages` has always accepted a null parent,
and the COALESCE sibling-slug index ([[KAIROS-T-0082]], tightened to live rows in
[[KAIROS-T-0184]]) already enforced root uniqueness. This was only ever a missing
affordance.

Gated on org-admin-or-member-of-this-team, read from the shared `whoami` resource
— the same rule and the same source as the announcement composer below it. A
non-member sees nothing rather than a button that 403s.

One detail worth the line of code: the caption. The same component now appears in
two places that differ only in where the thing lands, so it reads "created inside
this folder" or "created at the top level of the tree" depending on its parent. The
e2e step asserts the caption rather than trusting position, since position is the
one thing that would silently be wrong if it were mounted in the wrong panel.

### The e2e step found an accessibility bug

The first version filled the form with `getByLabel('Slug')`. It timed out — while
the panel, its caption and the sibling `<select>` all resolved, so the form was
rendered and visible and the locator was the only thing wrong.

Aurora's `TextInput` renders `<label class="cl-field__label">` with no `for` and
an `<input>` with no `id`. Nothing associates them, so **the accessible name of
every text input in the GUI is empty** — a screen reader announces an unlabelled
field. That is not a test problem; the test was the first thing to notice.

Filed as [[KAIROS-T-0198]] rather than fixed here: `TextInput` is upstream in
`colliery-io-aurora`, the fix arrives via a version bump, and the other form
components want auditing in the same pass. The step now locates fields through the
`.cl-field` wrapper with a comment saying why, and naming the ticket — a test can
navigate the DOM structurally and a screen reader cannot, so the workaround must
not read as the resolution.

### Coverage

`teampages.spec` creates a root folder as bob (a member, not an org admin), asserts
it lands as a sibling of Documentation rather than nested inside it, and asserts a
duplicate root slug surfaces the server's typed failure instead of a silent no-op.
The existing non-member step gains the negative: on `/teams/web`, where bob is not
a member, the panel is absent.

### Gates

lint clean, `angreal web lint` clean, `angreal web build --release` green, **397
unit tests**, integration **47/47**, e2e **16**, uat **22 journeys + drift gate**.