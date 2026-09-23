---
id: explanation-flight-levels-teams
level: task
title: "Explanation: flight levels, teams, access, archiving, repositories"
short_code: "KAIROS-T-0171"
created_at: 2026-09-23T22:11:20.896963+00:00
updated_at: 2026-09-23T22:51:46.198257+00:00
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

The five explanation pages. This is the mode Kairos has never had at all, and
the one where its accumulated architectural decisions currently live only in
ADRs and commit messages.

## Implementation Notes

**Blocked by [[KAIROS-T-0167]].** Explanation mode: understanding-oriented,
discursive, allowed to discuss alternatives and history — and it does **not**
instruct. Any "do this" belongs in a how-to.

Each page has an ADR or specification behind it. Read the source; do not
paraphrase a paraphrase.

### `explanation/flight-levels.md`

Why strategy / initiative / delivery, what each level is for, and why work
moves down while feedback moves up. Source: KAIROS-A-0002, and A-0001 for the
entity graph. The audience is someone who has seen a kanban board and wants
to know why this one has three.

### `explanation/teams-and-boards.md`

Team Topologies types, board ownership, a team's delivery board, and the
lifecycle — including why a team cannot be deleted until its board is clear
(KAIROS-I-0012). Migrate the *why* half of `README.md` lines 259–299; the
*how* half goes to [[KAIROS-T-0173]].

### `explanation/capabilities-and-access.md`

The A-0006 board-scoped capability model, and the thing people get wrong:
**it is not roles**. Team membership implies capabilities (KAIROS-T-0072),
org admin bypasses, and a document inherits its parent's board. Worth
explaining why `manage_*` is per-board rather than global, since that is the
design decision a reader will otherwise assume is an oversight.

### `explanation/archiving.md`

**The page most worth writing.** [[KAIROS-A-0020]]: archiving means this is
old, so it is hidden by default, and nothing more. The decision is fresh,
load bearing across every surface, and currently recorded only in an ADR and
a set of commit messages.

Cover: hidden by default but retrievable; not a permission boundary;
read-only plus restore; archived work is not live work, so guards that count
live rows still do; and the vocabulary collision with the document lifecycle.
KAIROS-T-0158's formulation is the clearest statement of the boundary and
should appear: **containment is a fact about the record; progress is a fact
about live work.** Explain why restore does not un-cascade, and why it
refuses rather than re-homing — both are decisions a reader will otherwise
read as bugs.

### `explanation/repositories-as-execution-scope.md`

KAIROS-A-0019: repositories as first-class execution scope, one owning team,
at most one repository per task, and how that shapes the agent loop.
Migrate the *why* half of `README.md` lines 300–392.

## Acceptance Criteria

- [x] All five pages exist, each serving explanation only — no instructions.
- [x] Each cites the ADR or spec behind it and is written from that source.
- [x] `archiving.md` covers all five ADR-20 rules, the vocabulary collision,
      and why restore neither un-cascades nor re-homes.
- [x] `capabilities-and-access.md` states plainly that capabilities are not
      roles.
- [x] `diataxis-review` passes on each; E-rule IDs cited per page.
- [x] `angreal docs build` clean, `SUMMARY.md` updated.

## Status Updates

- 2026-09-23: **All five pages written and reviewed.** `docs/src/explanation/`
  now holds `flight-levels.md`, `teams-and-boards.md`,
  `capabilities-and-access.md`, `archiving.md` and
  `repositories-as-execution-scope.md`; the five `SUMMARY.md` lines are
  uncommented and `angreal docs build` is clean.

  Written from the sources, not from the README: A-0002 + A-0001 for flight
  levels, I-0012 + A-0001 for teams and boards, A-0006 (including the T-0072
  amendment) for capabilities, A-0020 + I-0015 for archiving, A-0019 for
  repositories. The *why* halves of README 259–299 and 300–392 are migrated;
  every command, table and surface matrix in those lines was left behind for
  T-0173 and the reference tasks.

  **`diataxis-review`, per page.** Declared mode Explanation on all five
  (location, title pattern, opening). Actual distribution: flight levels 92/8,
  repositories 93/7, teams and boards 90/10, capabilities 85/15, archiving
  82/18 — explanation dominant everywhere, nothing misaligned under §3.

  | Page | Verdict after fixes |
  |---|---|
  | `flight-levels.md` | E1–E5 pass. **E6/S4** finding (default column sequences delegated to a reference that does not list them) fixed by citing KAIROS-A-0002 inline; added the missing link to `teams-and-boards.md`. |
  | `teams-and-boards.md` | E1, E3–E6 pass. **E2** finding (the decisive turn attributed to "the rule Dylan asked for" rather than given as rationale) rewritten to argue it; **E1 caution** finding ("Before KAIROS-I-0012" as the only time anchor) replaced with an identifier-free anchor. |
  | `capabilities-and-access.md` | E1–E5 pass. **E6** ×3 (the team-implied set specified exhaustively, glob matching semantics, and a deferral to a capability list reference does not carry) fixed by arguing the *shape* of the implied set and citing KAIROS-A-0006 for the enumeration. |
  | `archiving.md` | E1–E5 pass. **E6** ×2 fixed: the six rules now resolve to their numbering in KAIROS-A-0020 (which is where `reference/rest/*`'s "rule 2"/"rule 3" citations land), and the `RESTORE_BLOCKED` literal — whose only home in the book was this page — was removed in favour of a link to the restore endpoint reference. **S4** (link sink: three pages linked in, none out) fixed with reciprocal links. |
  | `repositories-as-execution-scope.md` | E1–E6 pass; strongest E5 in the set (it takes a position against the project's own vision and explains the reversal). **S4** deferral fixed with a link to `reference/rest/execution-scope.md`. |

  Cross-page: **S4/E6** finding that the Backlog-filing rule was argued in full
  on three pages, each deferring to another for the argument it was itself
  making. Fixed by making `repositories-as-execution-scope.md` the single home
  and reducing the other two to an orienting sentence plus the link.

  **S2 pass** (four distinct, correctly named top-level sections; no mode
  nested in another). **S3** one cosmetic finding declined: the sidebar entries
  are bare noun phrases while the H1s carry the explanation signal. The spine's
  wording is the initiative's D4 design and `SUMMARY.md` is shared with four
  concurrent tasks, so it is left for T-0175's tree pass. **E4 passes cleanly
  on all five** — no code fences, no numbered steps, no `kairos` command lines,
  no "you should"; the only operations named are described, never instructed.

  **Left to reference under E6, and needed by T-0169/T-0170/T-0175:**

  - **No reference home for the capability surface.** The vocabulary
    (`manage_*`, `transition_items`, `configure_*`, `manage_members`,
    `file_backlog`), glob matching, the org-admin bypass, the team-implied set
    and the Backlog-filing bounds appear only incidentally per endpoint;
    `reference/cli.md` says grants are not part of the CLI surface. Explanation
    currently cites KAIROS-A-0006 as a stopgap.
  - **`RESTORE_BLOCKED` is in no reference page.**
    `reference/rest/across-any-work-item.md` documents the 422 and
    `details.missing` without the code string.
  - **The six ADR-20 rules are cited by number from
    `reference/rest/work-items.md` (×10) and `boards-and-teams.md`, and no page
    in the book numbers them.** Explanation points at the ADR instead.
  - **No reference lists the per-level default column sequences**;
    `reference/rest/boards-and-teams.md` says a board is seeded with its
    level's defaults without naming them.
  - `archived_at` and `include_deleted` are properly specified in
    `reference/rest/schemas.md`, so explanation names and links them — that one
    is already the shape the other four should be.
  - The glossary and MCP tool reference exist on disk but are still commented
    out of `SUMMARY.md`, so they are named in prose without links (the
    `introduction.md` precedent); T-0175 links them.