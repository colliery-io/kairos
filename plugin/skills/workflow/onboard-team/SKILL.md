---
name: onboard-team
description: Take a new or half-set-up delivery team to a working state with the person — its team, members, board, repositories, team page and charter, dispatch, and local wiring. Reads first, lists the gaps, asks before each write; safe to run again.
argument-hint: "[team slug | new team name]"
disable-model-invocation: true
---

# Onboard a team

Take one delivery team to a working state, step by step, with the person. Do this when a team is new, or when a team works but has gaps: no repository linked, an empty charter, no route for requests.

The rules:

- **Read first.** Read the state of the team before you write. Then show the person a checklist of what is there and what is missing.
- **Ask before each write.** Say what you will change, and wait for a yes. A "yes" to one step is not a yes to the next.
- **Only fix what is missing.** You can run this skill again on a team that is set up. Do not change a thing that is already correct.
- **Use the MCP tools.** When a step has no MCP tool, use the CLI command that this skill names, or tell the person which step they must do in the web UI.

## 0. Check the admin tools

Most tools of this skill are in the admin profile of the Kairos MCP server (KAIROS-T-0454): `create_team`, `update_team`, `add_team_member`, `grant_capability`, the column and transition tools, the dispatch tools, `set_agent_triage` and the tools that write the team page. The default server (`/mcp`) does not list them, and Claude Code calls only the tools that a server lists.

Look for the `kairos-admin` server: its tools have names such as `mcp__kairos-admin__create_team_page`. (`team_pages`, which reads the team page, and `list_dispatch_rules` are on the default server only: read with the default server, write with `kairos-admin`.) If it is not there, tell the person to add it with this command, then to start a new session:

```sh
claude mcp add --scope user kairos-admin -- kairos mcp-proxy --profile admin
```

The command adds a second `kairos mcp-proxy` with the admin profile. It uses the same deployment and credential as the `kairos` server of the plugin: `kairos login`, or an agent key in `KAIROS_KEY` or `KAIROS_MCP_KEY`. Do not add the server yourself, and do not add a different MCP server: the person decides.

Use the `kairos-admin` tools for the admin steps, and the default server for the rest.

**The CLI fallback.** When the person does not add the server, use the CLI for the admin steps. It uses the same login. For example:

- A team and its delivery board: `kairos teams create --name <NAME> --slug <SLUG> --code-prefix <PREFIX> [--kind <KIND>]`.
- Members: `kairos teams members add|remove`.
- Boards, columns and transitions: `kairos boards --help`.
- The team page: `kairos teams pages list|show|create|edit|move|delete`.

Run `kairos <command> --help` for the arguments. Show the person each command before you run it.

## 1. Read the state

Find the team with `whoami` (your teams) and `my_boards` (boards, columns, the team of each ADR board). For a team that does not exist yet, go to step 2.

Collect:

| Part | Read with |
|---|---|
| The team: name, slug, type | `whoami`, `my_boards` |
| Members | `kairos teams members list <TEAM>` (slug or UUID; no MCP tool reads members) |
| Delivery board: prefix, columns, entry column, WIP limits | `my_boards`; `kairos boards show <BOARD>` |
| Transitions | `kairos boards transitions list <BOARD>` (no MCP tool reads them) |
| ADR board | `my_boards` (`ADR board of the team <slug>`) |
| Repositories, each with one owner team | `list_repositories {team}`, `get_repository` |
| Team page: the charter and the tree | `team_pages {team}`, then `team_pages {team, page: "charter"}` |
| Dispatch rules of the board | `list_dispatch_rules {board}` |

Then show the checklist. Mark each line **ok**, **missing** or **to check**. Example:

```text
Team platform — checklist
  ok       team, stream_aligned, 4 members
  ok       delivery board platform-delivery (PLAT), entry column Backlog
  to check ADR board: none (does the team record its own decisions?)
  missing  repository payments-api has no owner team here
  missing  charter: still the empty skeleton
  to check dispatch: no rules (does the team use a board session?)
```

Ask the person which lines to work on, in which order.

## 2. The team and its members

- **No team:** `create_team {name, slug, code_prefix}`. It makes the team and its delivery board (`{slug}-delivery`); an ADR board is a question of step 3. The prefix does not change later, so agree it with the person first.
- **A different name or type:** `update_team`.
- **Members:** `add_team_member {team, user}` for each person. Membership gives `manage_tasks`, `manage_documents` and `transition_items` on the boards of the team, and the right to edit the team page. `remove_team_member` takes a person off.
- **An agent of the team** (a service account or the agent key of a person) must also be a member to edit the team page. A board capability does not give that right.
- **Extra rights:** `grant_capability` or `set_board_capabilities`, for example `configure_boards` for the person who looks after the board.

These calls need the organization admin role or `manage_structure` on the organization. If they are refused, tell the person who can do them.

## 3. The board

The delivery board gets the default columns and transitions. Change them only when the team works in a different way.

- **Columns:** `add_column`, `update_column` (name, position, WIP limit, done column), `remove_column`.
- **Transitions:** `set_transition`. A board allows one-column hops by default.
- **No ADR board, and the team wants one:** `create_board {board_level: "adr", team, ...}` with the prefix of the team.

## 4. Repositories

Each repository has exactly one owner team. For each repository of the team:

- Not in the directory: `add_repository` with the team as owner.
- In the directory with a different owner: ask the person. A move to this team is `update_repository` with `team`, or `kairos repos update --team`. You need the right on the old team and on the new team.
- The description says how to work in the repository (build, test, review). Fill it in with `update_repository` when it is empty.

## 5. The team page

The team page has the charter and the folders and pages that the team grows. A folder holds folders and pages. A page holds nothing. You need membership of the team (or the organization admin role) to write.

**The charter** is the landing page and the social contract of the team. Write it with the person, not for them. Ask one question at a time and put the answers in these sections:

- **Mission:** why the team exists, in one or two sentences.
- **Scope:** what the team owns (products, repositories, services) and what it does not.
- **How to send us work:** a request to this team is a task on its delivery board. It lands in the entry column, in the support lane. Name who triages, and how often.
- **Ways of working:** the cadence, reviews, on-call, where decisions are recorded.
- **Success measures:** how the team knows it is doing well.

Read the charter first (`team_pages {team, page: "charter"}`), and keep what the person already wrote. Save it with `edit_team_page {team, page: "charter", content, version}`. Use the version that you read. A `CONFLICT` means that somebody saved first: read again, merge, and save.

**Folders and pages:** there is no fixed set. Ask the person what the team writes down (runbooks, how-to guides, design documents), and make only those: `create_team_page {team, kind: "folder", title}`, then pages in them with `parent`. `move_team_page` moves, reorders or renames. `delete_team_page` archives a page, or a folder with everything in it; a person restores it in the Archived list of the Pages view of the team.

The same steps are in the CLI: `kairos teams pages list|show|create|edit|move|delete`.

## 6. Dispatch and agent triage (when the team uses a board session)

Ask the person whether an agent session works the board of the team. If no, skip this step. If yes:

- Check `list_dispatch_rules {board}`. It also shows the agent triage setting. A missing rule: `create_dispatch_rule` (it can make the webhook target from `url`).
- Ask who decides the triage of intake (a request in the support lane). By default a person decides, and an agent only writes its proposal with `append_note`. When the team wants the agent to decide, call `set_agent_triage {board, agents_decide: true}`. It needs `configure_boards` on the board.

## 7. Local wiring

In each checkout of a repository of the team, run `/kairos:bootstrap`. It writes `.claude/kairos.local.md` with the team board and the repository. Then check that the repository has a code index: `rebuild_code_index` builds one when it is missing.

## 8. Close

Read the state again (step 1) and show the checklist. Each line must be **ok**, or the person must agree that it stays open. For each line that stays open, give the reason and who can close it.
