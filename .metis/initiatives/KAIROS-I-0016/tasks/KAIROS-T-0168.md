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
  - "#phase/completed"


exit_criteria_met: true
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

- [x] `reference/cli.md` covers all 16 nouns and their verbs, every flag
      verified against the binary rather than the README.
- [x] `reference/configuration.md` lists every server env var, Helm value and
      CLI setting, with defaults and constraints.
- [x] The retention variables are marked inert, with the reason.
- [x] Neither page instructs; task-shaped content links to how-to instead.
- [x] `diataxis-review` passes; R-rule IDs cited in the Status Update.
- [x] `angreal docs build` clean, `SUMMARY.md` updated.

## Status Updates

**2026-09-23 — done.** `docs/src/reference/cli.md` (~620 lines) and
`docs/src/reference/configuration.md` (~330 lines); both `SUMMARY.md` lines
uncommented; `angreal docs build` clean.

### Sources

`cli.md` is written from the binary, not the README: every command in the tree
was walked with `--help` (18 top-level commands, 56 distinct long flags, 9
positional arguments) and the page's flag set was diffed against that
extraction. The README's lines 23–92 were not copied — and it turns out they
predate `restore`, `--include-deleted`, `repos`, `service-accounts`, `keys` and
`tasks move`. **The README is now the stale copy**, which [[KAIROS-T-0175]] may
want to reduce to a pointer.

`configuration.md` is written from `config.rs`, `retention.rs`,
`credentials.rs`, `values.yaml`, `configmap.yaml`, `secret.yaml`,
`_helpers.tpl`, `docker-compose.yaml` and `.env.example`.

### Three honesty findings, not one

The brief named the retention sweeper. Two more turned up and are documented
the same way:

1. **Retention is inert.** `spawn_retention_loop` has no caller in the server;
   `RetentionConfig::from_env` has no caller anywhere. The five variables are
   documented with the statement leading the section, not buried in it.
2. **`KAIROS_OTEL_ENDPOINT` is chart-only.** `configmap.yaml` emits it; no
   crate reads it and the binary has no OpenTelemetry dependency. Documented as
   having no effect.
3. **Compose does not forward two of its own `.env` values.**
   `KAIROS_API_BEARER` and `KAIROS_WEB_CLIENT_SECRET` are in
   `deploy/.env.example` with explanatory comments, and
   `deploy/docker-compose.yaml`'s `environment:` block omits both. An operator
   following the example file sets them and they never reach the container.
   **This is a live bug in the reference deployment, not a docs problem** —
   worth a ticket outside this initiative.

### `diataxis-review` findings and what changed

Review run over both pages, criteria cited per page. Both classified
**Reference** by location, title and opening, with actual distribution
cli.md 96%/3%/1% and configuration.md 90%/8%/2% — neither misaligned. **R1
passed on both**, which is the rule §2.3 says reference pages most often fail:
`cli.md` mirrors the `kairos --help` tree, `configuration.md` mirrors the four
real config surfaces. Inventory was checked exhaustively and came back exact —
no invented flag, none missing, every Helm value and `.env` entry accounted for
with the right default.

Fixed:

- **R6, blocking, `cli.md`** — the page claimed `whoami` prints board
  capabilities. `print_identity` prints three lines (user, org+role, teams);
  capabilities, implicit capabilities and repositories are `--json` only. This
  was the one finding that would have sent a reader to a command that does not
  show what was promised. Corrected in both places.
- **R6 + R4, `configuration.md`** — "read once at startup / fails startup /
  empty is unset" was stated for the whole section but holds for the sixteen
  `AppConfig` variables only. The retention five are read by nothing, and their
  parser does *not* treat empty as unset — an empty
  `KAIROS_HISTORY_HOT_DAYS` is a parse error, not a default. Both scoped.
- **R6 ×3, `configuration.md`** — "exactly one of `database.url` /
  `database.existingSecret` or rendering fails" (only *neither* fails; both set
  means `existingSecret` wins); "both packaged deployments enforce" the tenancy
  exclusion (Helm does, Compose forwards both and has only a comment — the
  clause with real operational teeth); "every value under `config` becomes a
  ConfigMap entry" (three `webClientSecret*` never do, six more are
  conditional).
- **§4.4 + R2, `configuration.md`** — the two paragraphs justifying why inert
  variables are listed at all were rationale inside reference. Reduced to the
  applicability statement R6 actually wants, plus a link to
  `explanation/archiving.md`. Same for "deliberately not inferred from the
  `Host` header", "so a developer can override an embedded build", "so the file
  stays diffable".
- **R3, both** — `cli.md` had three table schemas across sibling command
  families, and the two lesser ones dropped the Type and Default columns
  entirely, so `repos create`'s seven flags lived in one prose cell. Now one
  schema (`Argument / Option | Type | Default | Description`) and a synopsis
  block for every command. `configuration.md`'s Helm section had four schemas
  and ~15 empty Description cells; now one schema, cells filled, probe rows
  carry `port: http`.
- **R2, both** — three imperative clauses across ~740 lines ("Set at most one",
  "Set to a published release", "must be cleared with"). Restated
  declaratively. Worth recording that this was the whole R2 exposure: no second
  person anywhere on either page.
- **S4 + §4.4, both** — five explanation pages exist and were linked from
  neither. Added a "Related reading" list to each, and replaced inlined
  "because" clauses (e.g. why `documents` has no `transition` verb) with links.
- **R1/R3, both** — the credential-cache facts had two homes. `cli.md`'s
  "Files" section now carries the path and the two modes and defers to
  `configuration.md#cli-configuration`; `configuration.md` links back.

**One finding deliberately not fixed. S4, degrading:** the ten "Related guides"
links point at `how-to/*.md` pages that do not exist yet, so they 404 on the
published book today. That is the initiative's chosen sequencing — reference
links out to how-to rather than restating procedures, the how-to task is
concurrent, and [[KAIROS-T-0175]] verifies links at close-out. Recorded so it
is not re-discovered as a defect. Same shape as [[KAIROS-T-0167]]'s decision to
leave `introduction.md`'s section links as plain text.

### For the how-to author

Five facts these pages now own, which a how-to should link to rather than
restate: the exit-code contract and the four structured rejections; the
`--url`/`--tenant`/`--json` common options; the full `repos create` flag set;
the credential-cache paths and modes; and the complete server variable list
with defaults. `move work between boards` in particular can assume the
`manage_tasks`-on-both-boards and repository-binding constraints are stated in
`cli.md#tasks-move`.
