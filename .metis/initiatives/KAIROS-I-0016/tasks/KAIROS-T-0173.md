---
id: how-to-for-people-doing-the-work
level: task
title: "How-to: for people doing the work, and for agent authors"
short_code: "KAIROS-T-0173"
created_at: 2026-09-23T22:11:33.613672+00:00
updated_at: 2026-09-23T22:11:33.613672+00:00
parent: KAIROS-I-0016
blocked_by: [KAIROS-T-0167]
archived: false

tags:
  - "#task"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: KAIROS-I-0016
---

## Parent Initiative

[[KAIROS-I-0016]]

## Objective

Six how-to guides for the two remaining audiences: people doing the work, and
people wiring agents up.

## Implementation Notes

**Blocked by [[KAIROS-T-0167]].** Two audience groups in `SUMMARY.md`:
`## For people doing the work` and `## For agent authors`.

### For people doing the work

- **`how-to/set-up-a-board.md`** — columns, transitions, lanes. The real edge
  worth including: a column holding only archived cards *can* now be removed
  (KAIROS-T-0161), but one holding a live card returns `COLUMN_NOT_EMPTY`.
- **`how-to/move-work-between-boards.md`** — `POST /api/tasks/{code}/move`
  (KAIROS-I-0012), which needs `manage_tasks` on **both** boards or org
  admin. That two-sided requirement is the thing a practitioner will hit.
- **`how-to/wind-down-a-team.md`** — the *how* half of `README.md` lines
  259–299 (the *why* half is [[KAIROS-T-0171]]). A team deletes once it owns
  no repositories (409) and its board holds no live cards (422, naming them).
  Archiving is how you clear the board.
- **`how-to/find-archived-work.md`** — the audit path, and the reason
  [[KAIROS-A-0020]] exists. Search with the put-away toggle
  (`[data-testid="include-put-away"]`), `kairos search --include-deleted`,
  reading an item and its history, and restoring it. Note the two refusals a
  reader will meet: writes to archived work, and `RESTORE_BLOCKED` when its
  board, column, team or repository is gone.

### For agent authors

- **`how-to/give-an-agent-machine-access.md`** — service accounts, API keys,
  rotation and revocation. Migrate `README.md` lines 192–258. The edges:
  the raw key is shown exactly once, two keys can be live at once so rotation
  needs no downtime, and deleting the account kills its keys.
- **`how-to/connect-over-mcp.md`** — the streamable-HTTP endpoint, auth,
  tenant resolution from the host, and what a client should expect.
  `crates/kairos-server/tests/mcp.rs` documents the real handshake;
  `uat/surfaces/mcp.ts` is a working minimal client.

## Acceptance Criteria

- [x] All six guides exist, under the two audience groups.
- [x] Each includes the refusal or constraint a practitioner actually meets
      (both-boards capability, the two team-delete guards, the archived
      refusals, one-time key display).
- [x] No guide teaches; conceptual detours link to `explanation/`.
- [x] `diataxis-review` passes on each; H-rule IDs cited per page.
- [x] `angreal docs build` clean, `SUMMARY.md` updated.

## Status Updates

### 2026-09-23 — six guides written, reviewed and revised

All six pages are in `docs/src/how-to/` and uncommented under
`## For people doing the work` and `## For agent authors`. `angreal docs build`
is clean. The how-to mode is now complete: 11 of 11.

Every required edge is in: `manage_tasks` on **both** boards with the three ways
out when you hold only one side; the two team-delete guards with their real
refusal text quoted; `COLUMN_NOT_EMPTY` against a column whose only remaining
cards are archived; the put-away toggle, `--include-deleted`,
`include_deleted=true` and `RESTORE_BLOCKED`; the one-time key display with
two-keys-live rotation. The procedures follow the UAT journeys
(`housekeeping`, `revival`, `machine-access`, `board-setup`, `reorg`) and the
real MCP handshake in `crates/kairos-server/tests/mcp.rs`, since those are the
closest thing to verified procedure the repo has.

#### `diataxis-review` — per page, with rule IDs

Reviewed against `plugin/references/diataxis.md` §2.2 (H1–H6), §4 and §5. All
six declared How-to by location, title, nav group and opening; all six actually
How-to dominant, none over the ~20% other-mode bar, and the reviewer found no
misalignment. Every link and anchor resolves; every CLI flag used exists in
`reference/cli.md`; all sixteen error codes cited exist in `reference/errors.md`.

- **`set-up-a-board.md`** — H1 ✓ H3 ✓ H4 ✓ H6 ✓. **Fixed a blocking H2/H3
  defect:** the preconditions claimed `configure_boards` carried every step, and
  step 4 does not work with it — metadata-definition writes are org-admin only
  (`crates/kairos-server/src/api/meta/definitions.rs:431,510,579`). A reader
  holding exactly what the page asked for hit 403 on the whole of step 4,
  including the retirement warning. Step 4 now states org admin in its heading
  and its precondition. **Fixed H5/§4.2:** a `## Lanes` section opened by saying
  there was nothing to configure and then toured three surfaces anyway — cut to
  one line and a link. **Fixed S4:** the trailing-glob sentence restated
  `cli.md`. **Fixed S3:** two sibling pages linked here as "Shape a board"
  against a nav entry and H1 of "Set up a board"; all three now agree.
- **`move-work-between-boards.md`** — H1 ✓ H2 ✓ H3 ✓ H4 ✓✓ H5 ✓ H6 ✓. The
  reviewer's strongest page: both tables are action-columned rather than
  descriptive, and the repository-binding fork with its three ways out was
  called textbook H4. **Fixed cosmetic H2/H5:** the "and it is deliberate…"
  justification in the preconditions and the `file_backlog` gloss both moved to
  links.
- **`wind-down-a-team.md`** — H1 ✓ H2 ✓ H3 ✓✓ H5 ✓ H6 ✓; clean on the per-page
  pass. Best H3 of the set — "exactly two guards, they fire in this order" is a
  plan, not a preamble. **Fixed H4:** step 3 offered `tasks delete` to clear a
  card without saying the delete **cascades to children**, which matters
  precisely because restore does not un-cascade. **Fixed S4:** no `Errors` link
  despite citing two codes.
- **`find-archived-work.md`** — H2 ✓ H3 ✓ H4 ✓ H6 ✓. **Fixed H5:** three
  motivating asides trimmed, and the "Kairos will not re-home it" rationale
  replaced by the `archiving.md#restore-refuses-rather-than-re-homing` anchor.
  H1 noted as cosmetic — the page covers reading and restoring as well as
  finding; the opening line declares that scope and the nav label is the one
  [[KAIROS-I-0016]] D4 fixed, so it was left alone deliberately.
- **`give-an-agent-machine-access.md`** — H1 ✓ H2 ✓ H3 ✓ H4 ✓ H6 ✓. The
  `client_credentials` warning was singled out as exemplary how-to voice: it
  corrects a competent reader's wrong assumption instead of explaining OAuth.
  **Fixed H5/§3:** a `## Security notes` block restated four facts the steps had
  already given; reduced to the one new instruction (scan for `kairos_sk_`).
- **`connect-over-mcp.md`** — H1 ✓ H2 ✓ H4 ✓ H6 ✓; its skip-ahead for clients
  that already speak streamable HTTP, and pointing at `uat/surfaces/mcp.ts`
  instead of a code walkthrough, were called the best H5/H6 moves in the set.
  **Fixed H3:** `X-Tenant` was introduced sixty lines after the `initialize`
  request that needs it on a header-tenancy deployment, so a hand-wired client
  failed at request one. **Fixed H4:** the tenancy fork omitted
  `KAIROS_SINGLE_TENANT`, where the header is ignored outright. **Fixed H5:**
  the agent-loop rationale cut to its operational consequence.

The reviewer proposed splitting an agent-loop page out of `connect-over-mcp.md`
and an upgrade page out of the Helm guide. Both declined: D4 sizes the book at
~11 how-tos deliberately, and in each case the material serves the same reader
at the same moment. Trimmed rather than relocated.

#### Two findings that belong to other tasks

1. **`reference/capabilities.md` documents a capability nothing checks.** It
   lists `configure_metadata` — "Create and edit metadata definitions" — as
   grantable, and `crates/kairos-web/src/pages/admin/capabilities.rs` offers the
   checkbox, but no handler consults it: every metadata-definition write is
   `require_org_admin`. So a grant can be made that does nothing, and the
   reference page and the generated `rest/tenant-configuration.md` contradict
   each other about who may do this (R4/R6). This is the discrepancy that made
   the blocking finding above possible. It needs either a product fix or a
   correction to [[KAIROS-T-0176]]'s page; it is not fixable from a how-to.
2. **S4, inbound links.** `reference/errors.md` and `reference/capabilities.md`
   have no "Related guides" lists, unlike `cli.md` and `mcp-tools.md`, so a
   reader landing on `RESTORE_BLOCKED` or `configure_boards` has no route to the
   procedure. Left for [[KAIROS-T-0175]]'s cross-link pass.

#### Tree-level

S2, S3, S4 (outbound), S5 all pass. S1 and S6 still fail on the empty tutorial
quadrant, which `introduction.md` actively routes new readers into — owned by
[[KAIROS-T-0174]]. Two explanation gaps were identified as the *cause* of two
findings here, worth noting rather than filling: nothing explains why metadata
definitions are admin-gated, and nothing explains why work class is a task
property rather than a board one.
