---
id: repository-scoped-work-repo
level: initiative
title: "Repository-Scoped Work - Repo Directory, Task Repo Binding, Repo-Aware Agent Loop, Cross-Team Filing"
short_code: "KAIROS-I-0010"
created_at: 2026-09-22T01:52:43.407132+00:00
updated_at: 2026-09-25T00:03:27.809604+00:00
parent: KAIROS-V-0001
blocked_by: []
archived: false

tags:
  - "#initiative"
  - "#phase/completed"


exit_criteria_met: false
estimated_complexity: L
initiative_id: repository-scoped-work-repo
---

# Repository-Scoped Work - Repo Directory, Task Repo Binding, Repo-Aware Agent Loop, Cross-Team Filing Initiative

## Context **[REQUIRED]**

Decision record: **KAIROS-A-0019** (decided 2026-09-21). Read it first; this
initiative implements it and does not restate the argument.

The product goal: agents know how to work within and complete work inside a
codebase, while knowing enough about other teams and codebases to open PRs and
coordinate across teams. Today the repo is invisible to the ticket model —
`tasks` has `board_id` + `team_id` only, `forge_connections` (KAIROS-I-0009) is
the only repo-shaped table and exists purely for webhook mirroring, and the
skills plugin binds a checkout to a *board* (`.claude/kairos.local.md`
`team_board`), so an agent in repo A is shown repo B's tickets as its own queue.

Decisions taken 2026-09-21 (Dylan, via AskUserQuestion):

| Question | Decision |
|---|---|
| Repo ownership | Exactly one owning team per repository |
| Task ↔ repo cardinality | At most one repository per task; multi-repo work decomposes |
| Cross-team filing | Any authenticated principal may file a task into any team's Backlog, Backlog only (amends A-0006) |
| Vision | Amend V-0001 and record the reversal in A-0019 |

## Goals & Non-Goals **[REQUIRED]**

**Goals:**
- A tenant-scoped `repositories` directory, team-owned, with `forge_connections`
  re-parented under it and existing connections backfilled.
- `tasks.repository_id` with create-time routing (repo → owning team → delivery
  board) and repo filtering on `board_items`, `search`, and the GUI board.
- The agent loop scoped to the repo it is in: bootstrap detects the repo from
  the git remote, the SessionStart hook and workflow skills pull that repo's
  queue, and new MCP tools (`list_repositories`, `get_repository`) let an agent
  discover other teams' repos, owners, boards and conventions.
- Cross-team filing: `create_item` into another team's Backlog with a
  `blocks` edge back to the originating item; PRs opened there link back via
  the existing forge ingestion.
- GUI: repo filter/swimlane on the team board; Repositories panel on the team
  landing page; org-admin repo CRUD folded into the existing forge setup
  surface.
- V-0001 amended; A-0019 decided.

**Non-Goals:**
- Many-to-many repo ownership or multi-repo tasks (review triggers in A-0019).
- Monorepo sub-path scoping.
- Rate limiting or quotas on cross-team Backlog filing.
- Repos on strategies, initiatives, documents or ADRs.
- Any change to how forge webhooks are authenticated or ingested (T-0099
  stays as is).

## Detailed Design **[REQUIRED]**

Grounded against the code on 2026-09-21. File paths are the seams a task
will edit; anything not named here follows the neighbouring convention.

### D1. Schema (kairos-db, tenant migration `2026-09-2x-000000_repositories`)

```sql
CREATE TABLE IF NOT EXISTS repositories (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug            TEXT NOT NULL,                     -- tenant-unique, [a-z0-9-]
    forge           TEXT NOT NULL CHECK (forge IN ('github', 'gitlab', 'other')),
    repo_full_name  TEXT NOT NULL,                     -- owner/repo | group/sub/project
    repo_url        TEXT NOT NULL,
    default_branch  TEXT NOT NULL DEFAULT 'main',
    team_id         UUID NOT NULL REFERENCES teams(id), -- exactly one owner (A-0019)
    description     TEXT NOT NULL DEFAULT '',          -- "how to work here" for agents
    created_by      UUID NOT NULL,
    updated_by      UUID NOT NULL,
    deleted_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_repositories_slug
    ON repositories (slug) WHERE deleted_at IS NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_repositories_forge_name
    ON repositories (forge, repo_full_name) WHERE deleted_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_repositories_team
    ON repositories (team_id) WHERE deleted_at IS NULL;

-- forge_connections becomes the webhook attribute OF a repository.
ALTER TABLE forge_connections ADD COLUMN IF NOT EXISTS repository_id UUID REFERENCES repositories(id);
-- Backfill: one repository per live connection. A connection with no team_id
-- cannot be backfilled (A-0019 requires an owner) — the migration fails loudly
-- listing them; the operator sets teams via the existing
-- PATCH /api/forge-connections/{id} first. (Dev/demo tenants all carry teams.)
INSERT INTO repositories (slug, forge, repo_full_name, repo_url, team_id, created_by, updated_by)
SELECT lower(regexp_replace(repo_full_name, '[^A-Za-z0-9]+', '-', 'g')),
       forge, repo_full_name, repo_url, team_id, created_by, created_by
FROM forge_connections WHERE deleted_at IS NULL AND repository_id IS NULL;
UPDATE forge_connections fc SET repository_id = r.id
FROM repositories r WHERE fc.forge = r.forge AND fc.repo_full_name = r.repo_full_name
  AND fc.deleted_at IS NULL;
ALTER TABLE forge_connections ALTER COLUMN repository_id SET NOT NULL;
ALTER TABLE forge_connections DROP COLUMN repo_full_name, DROP COLUMN repo_url, DROP COLUMN team_id;
DROP INDEX IF EXISTS idx_forge_connections_repo;
DROP INDEX IF EXISTS idx_forge_connections_team;
CREATE UNIQUE INDEX IF NOT EXISTS idx_forge_connections_repository
    ON forge_connections (repository_id) WHERE deleted_at IS NULL;   -- one live webhook per repo

ALTER TABLE tasks ADD COLUMN IF NOT EXISTS repository_id UUID REFERENCES repositories(id);
CREATE INDEX IF NOT EXISTS idx_tasks_repository ON tasks (repository_id) WHERE deleted_at IS NULL;
```

- Soft-deleted `forge_connections` rows are backfilled too (join on
  name, not `deleted_at`) so the NOT NULL holds; if a soft-deleted
  connection has no live repo twin, insert a soft-deleted repo for it.
- `item_history`: `repository_id` is a routing field like `team_id`, not
  content — recorded in `activity_log` as `task_repository_set`, no
  `item_history` version bump (matches `set_work_class`).
- `models/forge.rs::ForgeConnection` loses `repo_full_name/repo_url/team_id`,
  gains `repository_id`. `forge.rs::find_connection_by_repo` becomes a join
  through `repositories`; `links_for_connection_team` and `team_link_rollup`
  (T-0101) attribute via `repositories.team_id`. `LinkWithRepo`/`TeamLinkRow`
  keep their shape (repo name and forge now come from the join) so the
  Development panel and team rollup need no changes.
- New `models/repositories.rs` (`Repository`, `NewRepository`,
  `RepositoryUpdate`) and `repositories.rs` query module: `list`, `load`,
  `load_by_slug`, `find_by_forge_name`, `create`, `update`, `soft_delete`
  (refused while live tasks or a live connection reference it), and
  `delivery_board_for_team(team_id)` — the routing helper: the team's board
  with `level = delivery`; error if none or several.
- `tenant.rs` provisioning: nothing to seed. `seed.rs` demo: two repos for
  the platform team, one for the second team, connections re-pointed, a
  handful of tasks with `repository_id`, one cross-team-filed task.
- T-0093 upgrade-path test: pin this migration (still per-migration; the
  generalization stays its own backlog item).

### D2. Routing and the board-consistency rule (kairos-db `items.rs`, server `api/tasks.rs`)

`items::CreateTask` gains `repository_id: Option<Uuid>`. Resolution order in
the handler, before the capability check:

1. `repository_id` given, `board_id` absent → `board_id =
   delivery_board_for_team(repo.team_id)`, `team_id = repo.team_id`.
2. `repository_id` given, `board_id` given → the board must be that team's
   delivery board, else `422 validation: repository <slug> belongs to team
   <t>, whose delivery board is <b>`. `team_id` defaults to `repo.team_id`;
   an explicit different `team_id` is rejected the same way.
3. `repository_id` absent → today's behaviour exactly; `board_id` required.

`dto::CreateTaskRequest.board_id` therefore becomes `Option<String>` (breaking
only in the "neither given" case, which is 422). `UpdateTask` does not touch
the repo; a dedicated `PUT /api/tasks/{code}/repository { repository_id |
null }` (mirrors `set_work_class`) re-checks the rule and requires
`manage_tasks` on the task's board.

`dto::Task` gains `repository: Option<{ id, slug, repo_full_name, forge,
team_id }>` — embedded, not just an id, because every card and every agent
read wants the slug.

### D3. ABAC: computed `file_backlog` (kairos-core `abac.rs`, kairos-db `abac.rs`)

New vocabulary constant `FILE_BACKLOG = "file_backlog"`. It is never stored
and never granted; it is a third arm in `check_capability`:

```sql
OR ( $5 AND EXISTS (            -- $5 = required == FILE_BACKLOG
      SELECT 1 FROM boards b
      JOIN organization_members om ON om.user_id = $2  -- any tenant member
      WHERE b.id = $1 AND b.level = 'delivery' ) )
```

(`organization_members` is in `public`; the tenant is already pinned by
`search_path`, and `is_org_admin` shows the join pattern.) The handler asks
for `FILE_BACKLOG` instead of `MANAGE_TASKS` **only when all of**: item is a
task, `repository_id` is set, target column resolves to the board's position-0
column, and the caller lacks `MANAGE_TASKS` on that board. Every other create
path is unchanged. Negative tests (`tenant_isolation`-style, in
`kairos-server/tests`): non-member cannot file into position-1+; cannot file
without a repo; cannot file a repo whose team owns a different board; cannot
transition or edit what they filed; a service account (A-0017) can file.
`whoami` reports `file_backlog` under a new `implicit` list so agents can see
it.

### D4. Repository API (kairos-server `api/repositories.rs`, `api/org/forge_connections.rs`)

| Route | Cap | Notes |
|---|---|---|
| `GET /api/repositories?team=` | any member | list, joined owner team + delivery board + open task count |
| `GET /api/repositories/{slug}` | any member | plus `description`, default branch, live connection (id only), in-flight `item_links` rollup via T-0101 query |
| `POST /api/repositories` | org admin **or** `manage_tasks` on the owning team's delivery board | so a team lead (or their agent on bootstrap) can register their own repo |
| `PATCH /api/repositories/{slug}` | same as POST, evaluated on the *current* owner; changing `team_id` is allowed and does not touch existing tasks (rule D2 is checked on the next task write) |
| `DELETE /api/repositories/{slug}` | org admin | 409 while live tasks/connection reference it |
| `POST /api/forge-connections` | org admin (unchanged) | body becomes `{ repository: <slug or id> }`; the repo carries forge/name/url |
| `PATCH /api/forge-connections/{id}` | org admin | `team_id` field removed (ownership lives on the repo) |

`board_items` (`GET /api/boards/{id}/items`) and search's
`SearchFilterParams` gain `repository` (slug or UUID). `dto::ForgeConnection`
gains `repository: { slug, … }` and drops the three moved fields; the
`createForgeConnection` e2e helper and `forge.spec` follow.

### D5. MCP, client, CLI

- `list_repositories { team?: String }` → `[ { slug, forge, repo_full_name,
  repo_url, default_branch, team: { slug, name }, delivery_board: { slug,
  name }, open_tasks, has_webhook } ]`.
- `get_repository { repository: String }` → the above plus `description` and
  `in_flight: [ { kind, external_id, title, url, state, item_short_code } ]`.
- `create_item` gains `repository: Option<String>` (tasks only; `board`
  becomes optional when `repository` is given, per D2). Tool description
  states the cross-team rule in one sentence: "Any member may create a task
  against another team's repository; it lands in that board's Backlog."
- `BoardItemsParams` and `SearchFilterParams` gain `repository`.
- `whoami` adds `repositories: [ … ]` for the caller's teams.
- kairos-client: `list_repositories`, `get_repository`, `create_repository`,
  `update_repository`, `delete_repository`, `set_task_repository`; types
  updated. CLI: `kairos repos list|get|create|update|delete|bind|unbind`,
  `--repo` on `kairos tasks create` and `kairos search` (slug or UUID).
- *As built (T-0115):* the create/search wire field is `repository`
  (slug|UUID, resolved in the handler); `repository_id` stays a serde alias
  for one release. `set-repo` shipped as `kairos repos bind|unbind`.

### D6. Plugin (`plugin/`)

- `.claude/kairos.local.md` frontmatter keys become exactly:
  `deployment_url`, `tenant`, `repository`, `delivery_stream`, `team_board`,
  `initiative_board`. `repository` is the slug; the three board keys stay
  and are now derived defaults from the repo's owning team (still written so
  older hooks/skills keep working).
- `bootstrap`: step 3 gains "detect repo": `git remote get-url origin` →
  normalize to `(forge, full_name)` (ssh and https forms) →
  `get_repository`/`list_repositories` match → if none, offer to create it
  (`create_repository` with the user's team, if they have exactly one; else
  ask) → write `repository:`. No remote, or a remote the deployment does not
  know and the user declines to register: leave `repository:` empty and say
  what unblocks it.
- `hooks/session_start.py`: add `repository` to `FRONTMATTER_KEYS`; when
  set, the live-state hint becomes "call `get_repository` then `board_items`
  with `repository=<slug>`"; when unset, today's text.
- Skills: `implement` and `triage` scope their queue/search to the repo and
  refuse (with a pointer) to implement a task bound to a different repo than
  the checkout; `decompose` sets `repository` on every task it creates
  (asking per task when the initiative spans repos); `code-review` verifies the
  PR's repo matches the item's repo. `meta/kairos` gains a short "filing work
  against another team's repo" recipe: `list_repositories` → `get_repository`
  (read the description) → `create_item` with `repository` + `parent`/`blocks`
  → tell the user the short code and that it sits in the other team's Backlog.
- *As built (T-0115):* the recipe lives at
  `skills/workflow/implement/CROSS-TEAM-FILING.md` (the router only points at
  it); skills read the item's repo from `get_item` and bind via
  `kairos repos bind`; bootstrap matches the remote with `list_repositories`
  or `GET /api/repositories?forge=&name=` and registers via `kairos repos
  create`. `plugin/references/` holds methodology references (diataxis,
  architecture-review), not rendered tool docs, so there was nothing to
  regenerate; the MCP tool descriptions are the reference.

### D7. GUI (kairos-web)

- Board view: a **Repository** filter chip (multi-select) in the existing team
  lens bar; when the team owns >1 repo, an optional "group by repo" swimlane
  toggle reusing the Planned/Support lane machinery (T-0077). Task cards get a
  repo chip (slug) next to the type badge; the detail header the same.
- Team landing page: a **Repositories** panel (slug, forge link, open tasks,
  in-flight PR count) between the charter and the in-flight rollup (T-0101).
- Org admin: the forge-connections page becomes **Repositories** — list,
  create/edit (owner team, description, default branch), and "connect
  webhook" on a row, which is the old connection creation flow.
- Item detail: a repository picker on tasks (calls D2's PUT), rule violations
  shown inline.

### D8. E2E + docs

- `e2e/`: `repositories.spec` — admin creates a repo, team board filter,
  swimlane, card chip; cross-team filing as a member of team B into team A's
  Backlog and the resulting `blocks` edge; forge.spec adjusted for the new
  connection body; PR webhook against a cross-team ticket links back.
- Seeded fixtures per D1.
- Docs: operator guide "Repositories and forge connections" replaces the
  forge setup page; plugin README `bootstrap` section; MCP tool reference.
- *As built (T-0115):* `repositories.spec` uses per-run slugs, bob (platform
  member, non-admin) proves the team gate, carol's `blocks` edge is asserted
  (and a `supports` edge refused). Docs live in `README.md` ("Repositories" —
  why / how-to / reference / upgrade notes) rather than a separate operator
  guide; `plugin/README.md` points at the recipe file.

### Open points for Dylan's second pass

1. **Who may register a repo (D4 POST):** I chose org admin *or* a team-board
   `manage_tasks` holder so bootstrap can self-serve. Tighten to org-admin-only
   if you'd rather ownership be a deliberate admin act.
2. **`board_id` optional on `POST /api/tasks`:** required for D2 routing; the
   only behavioural change for existing clients is "neither given → 422".
3. **Backfill failure mode (D1):** fail the migration on team-less
   connections vs. auto-assign to a placeholder. I chose fail-loud; in practice
   only dev tenants have connections today.

## Alternatives Considered **[REQUIRED]**

Recorded in KAIROS-A-0019 (many-to-many ownership, board-attached repos,
multi-repo tasks, grant-gated or no cross-team filing, repos as free-text
metadata). Not repeated here.

## Implementation Plan **[REQUIRED]**

Phase gates:

- **discovery → design**: Dylan signs off A-0019 (transition it to decided),
  V-0001 amendment merged.
- **design → ready**: Detailed Design above filled in with concrete
  migration SQL shape, routing rules, ABAC query change, MCP tool schemas,
  and the `.claude/kairos.local.md` key set. Dylan reviews.
- **ready → decompose**: tasks cut roughly along the seven seams above, in
  dependency order (schema → service/API → ABAC → MCP/CLI → plugin → GUI →
  e2e/docs), each a vertical slice with its own tests.
- **decompose → active**: `/metis-ralph-initiative KAIROS-I-0010`.

## Progress Log

- 2026-09-21: Initiative created in discovery alongside A-0019 (draft).
- 2026-09-21: Dylan approved A-0019 → decided; initiative → design. Detailed
  Design D1–D8 written against the code; three open points listed for
  Dylan's second pass before → ready.
- 2026-09-21: Dylan: "go" on the defaults for the three open points
  (self-serve repo registration; `board_id` optional on task create;
  fail-loud backfill). Decomposed into T-0103 … T-0110 along D1–D8:
  T-0103 schema → {T-0104 routing → T-0105 ABAC, T-0106 repo API} →
  T-0107 MCP/CLI → T-0108 plugin; T-0109 GUI after T-0104+T-0106;
  T-0110 e2e/docs last. → active; Ralph loop started.- 2026-09-22: T-0103 … T-0110 completed in order (`f4f3330` schema,
  `cd206fb` routing, `204eebb` file_backlog, `4231b84` repo API, `85eefdb`
  MCP/CLI, `4a9f39c` plugin, `795e3e6` GUI, `7192431`+`b8eb956` e2e/docs).
- 2026-09-22: Deep-dive review (four-agent pass over the delivered code)
  produced six fix tickets T-0111 … T-0116; all completed: `21f8a05`
  collaborative edges + parent gate + repo in MCP reads (T-0111); `5e10c18`
  ownership invariants on every write path (T-0112); `e6ac54e`
  collision-safe backfill + populated/down migration tests (T-0113);
  `231aa25` web lens/lanes/picker/clippy backlog + `angreal test lint`
  (T-0114); `a657da4` e2e/plugin/docs coherence + `repository` wire name
  (T-0115); `42d7cd4` forge/ABAC hygiene (T-0116). D5/D6/D8 above carry
  "as built" notes where the delivery diverged from the design. Gates at
  HEAD: fmt, workspace clippy `-D warnings`, unit, integration 38/38, e2e
  11/11 (twice). Initiative left **active** for Dylan's review.