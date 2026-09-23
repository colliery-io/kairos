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
  - "#phase/todo"


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

- [ ] All six guides exist, under the two audience groups.
- [ ] Each includes the refusal or constraint a practitioner actually meets
      (both-boards capability, the two team-delete guards, the archived
      refusals, one-time key display).
- [ ] No guide teaches; conceptual detours link to `explanation/`.
- [ ] `diataxis-review` passes on each; H-rule IDs cited per page.
- [ ] `angreal docs build` clean, `SUMMARY.md` updated.

## Status Updates

*To be added during implementation*
