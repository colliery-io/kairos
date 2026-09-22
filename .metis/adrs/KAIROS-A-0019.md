---
id: 001-repositories-as-first-class
level: adr
title: "Repositories as First-Class Execution Scope - Tickets Issued and Executed per Repo, Planned per Board"
number: 1
short_code: "KAIROS-A-0019"
created_at: 2026-09-22T01:52:41.543281+00:00
updated_at: 2026-09-22T01:58:50.938537+00:00
decision_date: 
decision_maker: Dylan Storey
parent: 
archived: false

tags:
  - "#adr"
  - "#phase/decided"


exit_criteria_met: false
initiative_id: NULL
---

# ADR-1: Repositories as First-Class Execution Scope - Tickets Issued and Executed per Repo, Planned per Board

## Context **[REQUIRED]**

Kairos's stated purpose is that **agents know how to work within and complete
work inside a codebase**, while holding enough knowledge of other teams and
codebases to open PRs against them and coordinate cross-team work. The unit an
agent actually executes in is a git repository: it is checked out in one, its
tools and gates are that repo's, and its PRs land there. A team, however,
routinely owns several repositories, and one delivery board plans across all of
them.

Today the repository is nearly invisible to the ticket model (verified
2026-09-21):

- `tasks` carries `board_id` and `team_id` only
  (`crates/kairos-db/migrations/tenant/2026-07-09-000000_create_tenant_schema/up.sql`).
  A team with three repos has one board and no way to say which ticket belongs
  to which codebase.
- The only repo-shaped table is `forge_connections` (KAIROS-T-0097,
  `2026-09-01-000000_forge_links`): `(forge, repo_full_name, repo_url,
  team_id?)`, existing solely so webhooks can mirror PRs and branches into
  `item_links`. Nothing else reads it.
- The skills plugin binds a repo to **boards**, not to a repo record:
  `/kairos:bootstrap` writes `team_board`, `delivery_stream`, `initiative_board`
  into `.claude/kairos.local.md`, and `plugin/hooks/session_start.py` points the
  agent at the whole team board. An agent in repo A therefore sees repo B's
  tickets as its own queue, and the server never learns which repo the agent
  is in.
- There is no repo directory. An agent cannot ask "who owns repo X, what board
  do its tickets go on, how do I work there" — the only way to coordinate
  across teams is to already know the other team's board name.
- The vision (KAIROS-V-0001) says the opposite of what the product now needs:
  "Delivery streams over repositories … Repositories are metadata on tasks, not
  organizational boundaries" — and even that task-level metadata was never
  built.

The MCP surface today (`crates/kairos-server/src/mcp/tools.rs`): `whoami`,
`my_boards`, `board_items`, `get_item`, `get_history`, `search`, `create_item`,
`update_item`, `edit_item`, `transition_item`, `link_items`, `unlink_items`,
`set_metadata`, `delete_item`. None takes or returns a repository.

ABAC (A-0006, amended by KAIROS-T-0072): capabilities are board-scoped grants
plus implicit team-membership grants on the team's boards; org admins bypass.
There is no notion of "anyone may file into this board's Backlog".

## Decision **[REQUIRED]**

**Repositories become a first-class, team-owned entity in the tenant schema.
Boards and delivery streams remain the planning unit; the repository is the
unit tickets are issued against and executed in. Every task may bind to exactly
one repository; every repository is owned by exactly one team; and any
authenticated principal may file a task into any team's Backlog against that
team's repository.**

### 1. `repositories` table (tenant schema)

- Columns: `id`, `slug` (tenant-unique, URL-safe), `forge` (`github` |
  `gitlab` | `other`), `repo_full_name`, `repo_url`, `default_branch`,
  `team_id NOT NULL REFERENCES teams(id)`, `description` (short "how to work
  here" blurb agents read before starting), `created_by`, `deleted_at`,
  `created_at`, `updated_at`. Unique on `(forge, repo_full_name)` where not
  deleted; unique on `slug` where not deleted.
- **One owning team per repository.** Routing stays unambiguous: repo → owning
  team → that team's delivery board. Shared codebases are modeled by making the
  co-owning team a member of the owner's board via ordinary ABAC grants, not
  by multiple owners.
- `forge_connections` becomes the webhook attribute **of** a repository:
  gains `repository_id NOT NULL REFERENCES repositories(id)`; its
  `(forge, repo_full_name, repo_url, team_id)` columns are backfilled into a
  new `repositories` row per live connection and then dropped. `item_links`,
  the secret derivation (`forge/auth.rs::derive_secret` over the connection
  id), and ingestion (KAIROS-T-0099) are unchanged. A repository may exist
  with no connection (no webhooks yet); a connection cannot exist without a
  repository.

### 2. `tasks.repository_id` (nullable)

- Only **tasks** bind to a repo. Strategies, initiatives, documents and ADRs
  stay repo-less: that is where cross-repo intent lives.
- **At most one repository per task.** Work that touches several repos is
  decomposed into one task per repo, joined by the existing parent and
  `blocks` relationship edges (A-0001). No join table.
- Setting `repository_id` on create routes the task: `board_id` defaults to
  the owning team's delivery board and `team_id` to the owning team, unless
  the caller explicitly overrides them and holds capability on the override.
  A task's `repository_id` must belong to a team whose delivery board is the
  task's board (enforced in the service layer, not a DB constraint, so
  re-homing a repo between teams is one update).

### 3. Repo-scoped agent loop

- `/kairos:bootstrap` detects the repository from `git remote get-url origin`,
  resolves it via the new API (creating it, if the user may), and records
  `repository: <slug>` in `.claude/kairos.local.md` alongside the existing
  keys. `team_board` etc. become derived defaults, still written for backwards
  compatibility.
- The SessionStart hook and the `implement`, `triage`, `decompose` and
  `code-review` skills use the repo as the primary scope: `board_items` /
  `search` accept `repository`, and the injected context is "this repo's
  open work", with the team board as the wider lens.
- New MCP tools: `list_repositories` (slug, forge, owning team, delivery
  board, open counts) and `get_repository` (plus `description`, default
  branch, in-flight links). `create_item` accepts `repository`.
- Because the forge webhook already links a PR carrying a short code to its
  item (T-0099), an agent that opens a PR in another team's repo for a ticket
  it filed there needs no new plumbing to have it show up on that ticket.

### 4. Cross-team filing (amends A-0006)

- Any authenticated tenant member may `create_item` a **task** into **any**
  delivery board, but only into that board's **Backlog** column (the column
  with `position = 0`), with `repository_id` set to one of that board's
  team's repos. This is a new implicit capability, `file_backlog`, granted
  tenant-wide by the ABAC check the way team membership is today
  (KAIROS-T-0072): computed, not stored.
- Everything past Backlog — transitions, edits by non-members, deletion —
  stays behind the existing board capabilities. The owning team's triage gate
  is the control point.
- The filed task records `created_by` as usual; the filing skill also adds a
  `blocks` edge from the new task to the originating item so the requesting
  team's board shows the dependency.

### 5. Vision amendment

KAIROS-V-0001 is amended in the same change: "Delivery streams over
repositories" becomes "Streams and boards plan the work; repositories are where
tickets are issued and executed." The "repos are metadata, not boundaries" lines
are replaced accordingly. Streams still span repos; that part of the vision is
kept.

## Alternatives Analysis **[CONDITIONAL: Complex Decision]**

| Option | Pros | Cons | Risk Level | Implementation Cost |
|--------|------|------|------------|-------------------|
| **Team-owned `repositories` + nullable `tasks.repository_id` + open Backlog filing (chosen)** | Unambiguous routing; agent scope matches execution reality; cross-team coordination works with zero new grants; reuses forge links, ABAC, relationships wholesale | Reverses a vision principle; migration of `forge_connections`; one more scoping axis in board views | Low | Medium |
| Many-to-many repos ↔ teams | Models genuinely shared codebases directly | Routing needs a primary owner anyway; every "which board?" question becomes a choice; more admin surface | Medium | Medium-High |
| Repos attached to boards, not teams | Loosest coupling; a repo can show on several boards | Teams inferred through boards; contradicts "team owns delivery board"; ownership becomes unclear for agents asking "who do I talk to" | Medium | Medium |
| Many repos per task (join table) | One ticket can span codebases | Agent queue per repo becomes a join; "done" becomes ambiguous per repo; PR-to-item linking loses precision | Medium | Medium |
| Cross-team filing only by explicit grant | Tighter control | Every pair of collaborating teams needs admin setup before an agent can coordinate; kills the day-1 cross-team story | Low | Low |
| No cross-team creation (propose only) | No write into other boards | Agents can only leave breadcrumbs; the other team must transcribe; coordination latency is human-bounded | Low | Low |
| Keep repos as free-text task metadata (vision as written) | No schema beyond a metadata definition | No ownership, no routing, no directory, no validation; agents cannot discover other teams' repos | High (does not meet the goal) | Low |

## Rationale **[REQUIRED]**

1. **The agent's frame is the repo.** Everything an agent can actually do —
   run tests, open a PR, read conventions — is bounded by the checkout it is
   in. A ticket model that cannot name the repo cannot tell the agent what is
   its work and what is a neighbour's.
2. **One owner, one repo per task keeps every routing question closed-form.**
   Given a repo you know the team, the board, and the Backlog column; given a
   task you know the one codebase it changes. Multi-repo work decomposes into
   per-repo tasks, which is what an agent has to do anyway to open separate
   PRs.
3. **Cross-team coordination has to be default-on for the product to work.**
   The value proposition is an agent in repo A opening the right ticket (and
   PR) against repo B without a human first arranging permissions. Backlog-only
   filing, behind the owning team's existing triage gate, is the minimum write
   that achieves this and the maximum that A-0006's whitelist stance can
   tolerate.
4. **Reuse over invention.** `forge_connections` already is a repo record
   missing a name; `item_links` already attaches PRs to items by short code;
   `blocks` edges already express cross-team dependency; implicit
   team-membership grants already show how to add a computed capability.
   Nothing here is a new mechanism, only a promoted one.
5. **The vision was written before the agent loop existed.** "Delivery streams
   over repositories" was a reaction to Metis's one-repo limit. That limit is
   still gone — streams and boards still span repos — but a planning unit and
   an execution unit are different things, and the vision conflated them.

## Consequences **[REQUIRED]**

### Positive
- Agents get a per-repo queue and a per-repo "how to work here" blurb; the
  session hook stops showing a whole team board as "your work".
- A repo directory: any agent or human can find who owns a codebase and where
  to file against it.
- Cross-team tickets flow with zero admin setup, and PRs opened against them
  link back automatically through the existing forge ingestion.
- Board views gain a repo swimlane/filter, which also serves humans on teams
  with several codebases.

### Negative
- A migration that reshapes `forge_connections` and touches every existing
  tenant (the upgrade-path test noted in KAIROS-T-0093 gets one more
  migration to pin).
- One more scoping axis in `board_items`, `search`, the GUI board, and the
  team page; plus new MCP tools and client/CLI commands.
- Open Backlog filing is a deliberate widening of A-0006; a noisy tenant can
  fill another team's Backlog. Triage is the remedy; rate limits are not in
  scope.
- The vision reverses a principle. The ADR is the record of why.

### Neutral
- Delivery streams are unchanged and still the cross-repo planning
  construct.
- Tasks without a repository remain valid (non-code work, or tenants not
  using repos at all).
- `forge_connections` keeps its id-derived webhook secret; existing webhook
  URLs and secrets keep working after the migration.

## Review Schedule **[CONDITIONAL: Temporary Decision]**

### Review Triggers
- A tenant with genuinely co-owned repositories where "grant the second team
  on the owner's board" proves inadequate → revisit many-to-many ownership.
- Cross-team Backlog filing producing spam or abuse in practice → revisit an
  explicit `file_backlog` grant or per-team opt-out.
- Monorepo tenants wanting sub-repo scope (path prefixes) → revisit whether
  `repositories` needs a `path` dimension.