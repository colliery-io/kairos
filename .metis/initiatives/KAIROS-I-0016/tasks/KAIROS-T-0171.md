---
id: explanation-flight-levels-teams
level: task
title: "Explanation: flight levels, teams, access, archiving, repositories"
short_code: "KAIROS-T-0171"
created_at: 2026-09-23T22:11:20.896963+00:00
updated_at: 2026-09-23T22:11:20.896963+00:00
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

- [ ] All five pages exist, each serving explanation only — no instructions.
- [ ] Each cites the ADR or spec behind it and is written from that source.
- [ ] `archiving.md` covers all five ADR-20 rules, the vocabulary collision,
      and why restore neither un-cascades nor re-homes.
- [ ] `capabilities-and-access.md` states plainly that capabilities are not
      roles.
- [ ] `diataxis-review` passes on each; E-rule IDs cited per page.
- [ ] `angreal docs build` clean, `SUMMARY.md` updated.

## Status Updates

*To be added during implementation*
