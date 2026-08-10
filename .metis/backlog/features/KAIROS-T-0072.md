---
id: team-membership-as-implicit-board
level: task
title: "Team membership as implicit board capabilities (A-0006 amendment)"
short_code: "KAIROS-T-0072"
created_at: 2026-08-10T03:30:42.731384+00:00
updated_at: 2026-08-10T03:41:33.705997+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#feature"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: NULL
---

# Team membership as implicit board capabilities (A-0006 amendment)

## Objective **[REQUIRED]**

Make membership of a board's owning team an IMPLICIT capability source (KAIROS-A-0006 amendment, option (b) approved by Dylan 2026-08-09): the ABAC check passes when the user holds an explicit grant OR is a member of the team that owns the board, for a fixed implied set — `manage_tasks`, `manage_documents`, `transition_items`. Nothing stored, nothing to sync: leaving the team is the revocation.

UAT finding: bob (member of platform) could not transition cards on platform-delivery — the seed grants no capabilities, and team membership implied none. The GUI also showed affordances the server would 403.

## Backlog Item Details **[CONDITIONAL: Backlog Item]**

### Type
- [x] Feature - New functionality or enhancement

### Priority
- [x] P1 - High (important for user experience)

### Business Justification **[CONDITIONAL: Feature]**
- **User Value**: "Join the team ⇒ work the team's board" is the universal expectation; requiring per-member per-board admin grants makes every onboarding a two-step chore and made the demo tenant unusable as a non-admin.
- **Effort Estimate**: M
- **Rejected alternatives** (recorded from the UAT discussion): (a) auto-grant rows on team join — sync/revocation ambiguity when admins customize grants; (c) pure whitelist + seeded grants — keeps the onboarding chore, fixes only the demo.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria **[REQUIRED]**

- [x] `kairos-core::abac`: `TEAM_IMPLIED_CAPABILITIES` (`manage_tasks`, `manage_documents`, `transition_items`) + pure `team_implies(required)` helper, unit-tested; `configure_*`, `manage_members`, and the other `manage_*` families are NOT implied
- [x] `kairos-db::abac::check_capability`: one query still — an OR-EXISTS arm over `boards.team_id` → `team_members` fires only when `team_implies(required)` (bound as a parameter); explicit-grant semantics unchanged
- [x] bob (no explicit grants) can create/transition tasks and documents on `platform-delivery`; cannot on `web-delivery` (not his team) or org-level boards; cannot configure boards or manage members anywhere
- [x] GUI affordances follow capability: drag/move-menu and the create "+" (and "New document") hidden on boards where the user has neither explicit grants, implied team capability, nor admin role — whoami already carries role + explicit grants; the teams arm compares `whoami.teams[].id` to `board.team_id`
- [x] Integration coverage: db-level check for the implied arm (member passes, non-member fails, non-implied capability fails even for members)
- [x] E2E: team-lens spec extended — bob creates AND drags a card on platform-delivery, and web-delivery shows him no move/create affordances
- [x] KAIROS-A-0006 carries a dated amendment; `angreal test all` + e2e green

**Completed 2026-08-09**: full gate chain green — web lint, unit, integration (incl. `team_membership_implies_delivery_capabilities`), and e2e with all three Playwright specs passing clean (drag 1.9s, smoke 1.1s, team-lens 1.0s incl. the new bob create+drag and no-affordances steps).

## Implementation Notes **[CONDITIONAL: Technical Task]**

### Technical Approach
- Implied set fixed in core (vocabulary owner); db binds `team_implies(required)` as a bool param and adds `OR ($4 AND EXISTS (SELECT 1 FROM boards b JOIN team_members tm ON tm.team_id = b.team_id WHERE b.id = $1 AND tm.user_id = $2))` — tenant search_path, unqualified names, same convention as the existing query.
- `authorize` (admin bypass → check_capability) is unchanged; the arm lives inside check_capability so every call site (items, transitions, documents-inherit-parent-board) gets it.
- GUI: `WhoamiTeam` mirror gains `id` (server already sends it); Shell provides the shared whoami via context; the board view computes `can_transition` / `can_create(kind)` / `can_create_documents` from role + explicit grants (glob-aware) + team implication, and gates draggable, the Move menu, column "+", and "New document".
- whoami's `capabilities` list intentionally still reports only EXPLICIT grants (it's the grant-management view); the implied source is derivable from `teams` + `board.team_id`.

### Risk Considerations
Implied set is deliberately narrow (no `configure_*`, no `manage_members`, no strategy/initiative/ADR `manage_*`). A team-owned board at a non-delivery level (possible via `POST /api/boards` with `team_id`) grants its team the same narrow set — acceptable and documented in the ADR amendment.

## Status Updates **[REQUIRED]**

- 2026-08-09: Created; option (b) chosen by Dylan over auto-grant rows and pure-whitelist alternatives.
- 2026-08-09: Implemented across all four layers. **core**: `TEAM_IMPLIED_CAPABILITIES` + `team_implies` + vocabulary test (implied set exact; globs never implied). **db**: `check_capability` gains the `$4 AND EXISTS(boards→team_members)` arm, implication decided in Rust via core (one query still); new integration test `team_membership_implies_delivery_capabilities` in tests/abac.rs (own scratch DB `kairos_abac_team_test` — the lifecycle test drops/recreates its own and cargo tests run parallel): implied set passes for a member on the team board, withheld set fails, outsider fails, teamless board fails, explicit grants unaffected. **web**: `WhoamiTeam` mirror gains `id`; Shell `provide_context`s the shared whoami; boards.rs gets the pure `BoardPowers`/`board_powers` client mirror (admin bypass, board-scoped grants with trailing-glob matching, team implication) with host tests ×2; BoardPage derives `Signal<BoardPowers>` (reactive on whoami landing); gated: card draggable + dragstart, the Move menu, per-column "+", and "New document". **e2e**: team-lens 5b (bob CREATES a task then DRAGS it Backlog→Todo on platform-delivery — created fresh because smoke's pickMovableTask makes seeded placements nondeterministic by then) and 5c (web-delivery: draggable="false", no Move, no "+"). **ADR**: A-0006 "Team-Implied Capabilities" amendment section (dated, motivation + rejected alternatives + non-delivery-board caveat). Gates: core/db/web checks clean; `angreal web lint` + `test all` + e2e running.