---
id: reference-the-cli-command-tree-and
level: task
title: "Reference: the CLI command tree and every configuration value"
short_code: "KAIROS-T-0168"
created_at: 2026-09-23T22:11:09.503905+00:00
updated_at: 2026-09-23T22:11:09.503905+00:00
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

`reference/cli.md` and `reference/configuration.md` — the two pages a
practitioner looks things up in most, and the two most at risk of being
wrong rather than merely thin.

## Implementation Notes

**Blocked by [[KAIROS-T-0167]].** Reference mode: information-oriented, no
instruction. S-0008's R-rules govern, R4 especially — complete for its scope,
nothing "documented elsewhere only".

### `reference/cli.md`

Source of truth is the binary, not the old README. `kairos --help` and
`kairos <noun> --help` for all 16 nouns; the family verbs come from the
`entity_family_cli!` macro in `crates/kairos-cli/src/commands/entities.rs`
(note `restore` was added by KAIROS-T-0160, and the five family `list` verbs
gained `--include-deleted` in KAIROS-T-0159).

Migrate from `README.md` lines 23–92, but **verify every flag against the
binary** rather than copying — that section predates several changes.

Reference does not instruct, so a flag's entry says what it does, its default
and its constraints. "To do X, run Y" belongs in a how-to and should be
linked, not restated.

### `reference/configuration.md`

Every knob, in one place, which is what R4 requires:

- server environment variables — grep `std::env::var` across
  `crates/kairos-server/`, and `crates/kairos-core/src/retention.rs:37-48`
  has the retention set (`KAIROS_HISTORY_HOT_DAYS`,
  `KAIROS_HISTORY_KEEP_LATEST`, `KAIROS_ACTIVITY_RETENTION_DAYS`,
  `KAIROS_ARCHIVE_TARGET`, `KAIROS_RETENTION_MODE`) with defaults at `:162-172`;
- Helm values — `deploy/helm/kairos/values.yaml`, which is already
  commented per-value;
- CLI config — `KAIROS_CONFIG_DIR` and the `credentials.json` shape;
- `deploy/.env.example`.

**One honesty requirement.** The retention sweeper is **not wired into the
server** — `spawn_retention_loop` has zero references in
`crates/kairos-server/` and its doc comment calls server wiring an M2 task.
Its variables are therefore inert today. Document them as such. A reference
page that lists a knob which does nothing is worse than one that omits it.

## Acceptance Criteria

- [ ] `reference/cli.md` covers all 16 nouns and their verbs, every flag
      verified against the binary rather than the README.
- [ ] `reference/configuration.md` lists every server env var, Helm value and
      CLI setting, with defaults and constraints.
- [ ] The retention variables are marked inert, with the reason.
- [ ] Neither page instructs; task-shaped content links to how-to instead.
- [ ] `diataxis-review` passes; R-rule IDs cited in the Status Update.
- [ ] `angreal docs build` clean, `SUMMARY.md` updated.

## Status Updates

*To be added during implementation*
