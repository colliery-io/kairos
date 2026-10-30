---
title: "MCP tools"
---

Kairos serves the Model Context Protocol at `/mcp`. The surface is exactly
eighty-five tools. A drift gate in the test suite
asserts that `tools/list` returns these eighty-five and no others. The same
gate asserts that this page has one section for each tool.

The promise is for one release: this page agrees with `tools/list`. The count
is not a promise for later releases. A later release can add a tool or an
argument.

This page describes Kairos 0.18.1. Argument names, types and defaults are those
of the JSON schema the server sends in `tools/list`.

## Profiles

`tools/list` gives one profile of the tools (KAIROS-T-0454). The tool text
goes into each agent session, and most sessions do not administer the
organization.

| URL | Profile | Tools |
|---|---|---|
| `/mcp` or `/mcp?profile=work` | work (the default) | The work on items, the repository directory and code index builds, the members, claims, notifications, notices and watches, the scales and templates, board flow, dispatch rules and team pages to read: 53 tools. |
| `/mcp?profile=admin` | admin | Teams, boards, columns and transitions, grants, metadata definitions and templates, the writes of dispatch rules and webhook targets, agent triage, the writes of team pages, org read tokens, the delete of repositories, purge, the count of the refusals: the other 32 tools. |
| `/mcp?profile=all` | all | Each tool. |

The profile changes the list, not the rights. Kairos runs a call to a tool
that the profile does not list, with the usual checks of the rights. A client
such as Claude Code calls only the tools that `tools/list` gives. Thus a
Claude Code session needs a server with the admin profile for an admin tool.
Kairos refuses an unknown profile, and the refusal names it.

The work tools:

- items: `whoami`, `my_boards`, `my_work`, `board_items`, `search`, `get_item`,
  `get_history`, `related_work`, `create_item`, `edit_item`, `update_item`,
  `transition_item`, `move_item`, `append_note`, `promote_note`,
  `set_lifecycle`, `set_state`, `set_review`, `set_metadata`;
- links and fields: `link_items`, `unlink_items`, `propose_edge`,
  `set_repository`, `set_parent`, `set_work_class`, `set_complexity`,
  `set_team`, `clear_team`;
- the end of an item: `cancel_item`, `reopen_item`, `delete_item`,
  `restore_item`, `archive_completed`;
- repositories: `list_repositories`, `get_repository`, `add_repository`,
  `update_repository`;
- claims and notifications: `claim_item`, `hand_off_item`, `release_item`,
  `get_events`, `mark_events_read`, `watch_item`, `unwatch_item`,
  `list_watches`;
  `get_events`, `mark_events_read`, `send_notice`;
- people: `list_members`;
- scales and templates: `list_metadata_definitions`, `list_templates`;
- team pages, to read: `team_pages`;
- the flow of a board and its dispatch rules, to read: `board_flow`,
  `list_dispatch_rules`;
- a code index build: `rebuild_code_index`.

To administer from Claude Code with the plugin, add a second server with the
admin profile. It uses the credential of the plugin server: `kairos login`, or an
agent key in `KAIROS_KEY` or `KAIROS_MCP_KEY`.

```sh
claude mcp add --scope user kairos-admin -- kairos mcp-proxy --profile admin
```

Without the `kairos` CLI, add the HTTP server with an agent key:

```sh
claude mcp add --scope user --transport http kairos-admin \
  "$KAIROS_URL/mcp?profile=admin" --header "Authorization: Bearer $KAIROS_MCP_KEY"
```

## Conventions

These hold for every tool.

| Aspect | Rule |
|---|---|
| Tenant | There is no tenant argument. The tenant is the one resolved from the connection's host or the `X-Tenant` header. |
| Identity | Every tool runs as the authenticated principal — a human user or a service account — under the same attribute-based access control as the REST handlers. Archiving is not a permission boundary: whoever could read an item before it was archived can read it after. |
| Item identity | Items are named by short code, e.g. `ACME-T-0012`, in every input and every output. Kairos changes a code in input to capital letters, so `acme-t-0012` finds `ACME-T-0012`. |
| Retired codes | When an item gets a new short code, its old code is retired. Kairos does not issue a retired code again. A read tool (`get_item`, `get_history`, `search`) finds the item from a retired code and shows the current code. A write tool refuses a retired code for the item that it changes. The refusal is `NOT_FOUND`, and it gives the current code. A reference in a write follows a retired code: the `source` and the `target` of `link_items` and `unlink_items`, and the `parent` of `create_item`. The answer then has a line such as `COLLIERY-T-2430 is now SKADI-T-0577.` |
| References | Each kind of reference has one set of forms in each tool and in each filter (KAIROS-T-0689). A team (`team`, `from_team`, `filter.team`), a board (`board`, `to_board`, `filter.board`) and a repository (`repository`, `filter.repository`): a slug or a UUID. A column (`column`, `to_column`, `from_column`, `filter.column`): a column name in any case, or a UUID. A person (`user`, the `to` of `hand_off_item`, `filter.claimed_by`, `filter.created_by`, `filter.owned_by`): `me`, a user id, an email, a user name, or the name of a service account. An item: a short code. Kairos removes the blanks at the two ends of a reference. It refuses each other form: for example, a team by its name or its code prefix. |
| Listing weight | Listings are compact: short code, title and key fields. Full markdown content arrives only from `get_item` and from `get_history` with a `version`. |
| Parts of a list | A list can show a part of its rows. Then the list says how many rows come after it, and the `offset` of the next part: `2 more; offset 50.` Send that `offset` to get the next part. `search`, `get_history`, and the two lists of open work of `get_repository` say it this way. |
| Errors | A refusal comes back as an MCP tool error whose text is `CODE: message`, the code being the same stable one as the REST API's error envelope. Where that code has structured `details`, a second line follows: `details: ` and the JSON object. See [Errors](../errors/) for what each code's `details` carries. The key `field` names the argument that the refusal is about, as it names the field of a REST body. |
| Audit | Writes are recorded in the activity log by the same code path as the REST API. |
| Notifications | While the caller has unread [notifications](#your-notifications), each answer (not a refusal) ends with a block: the 5 newest, then `More: call get_events` when there are more. The block stops when the caller marks them read. An agent key keeps its own read state, apart from the person. `get_events` itself has no block, because it lists them. A service account gets no notifications. |
| Request keys | A write can be complete when its answer does not get to the client (a timeout, an upgrade). These tools take an optional `request_key`: `create_item`, `append_note`, `link_items`, `propose_edge`, `promote_note`, `transition_item`, `move_item`, `edit_item`, `add_repository`, `create_team`, `create_board`, `add_column`, `create_team_page`, `create_dispatch_rule`. The key is 8 to 128 characters of letters, digits, `.`, `_`, `:` and `-`; a UUID is a good key. Send a new key with each new request, and the same key when you retry it. A second call with the same key, by the same person and agent key, in 24 hours, writes nothing. It gives the answer of the first call, then the line `Replayed: the call with this request_key at <time> applied the write. Nothing new was written.` The same key with a different tool or different arguments is refused with `REQUEST_KEY_REUSED`. A refused call keeps no key, so its retry runs again (KAIROS-A-0065). When the deployment is unavailable, `kairos mcp-proxy` sends a call with a `request_key` again two times, as it does a read (KAIROS-T-0687), and the error data says `"retry": "replay"`. It also sends an idempotent write again two times: the sets, the patches and the replaces (`set_state`, `set_metadata`, `set_parent`, `mark_events_read` and the others that `kairos_client::mcp_tools` marks idempotent). A second call gives the same state, and the error data says `"retry": "idempotent"`; a retry of a `set_state` with `expected_updated_at` that the first send applied is refused with `CONFLICT` and the current state (KAIROS-T-0699). It does not send any other call with no key again. |
| Claims | A task in Active has a [claim](../glossary/#claim): the person who works on it. A write to a task that a different person has the claim on succeeds and is recorded. The text of the tool then ends with a line: `Warning: Alice (agent) has the claim on ACME-T-0012 (since 2026-10-09T10:00:00Z). Your change is recorded.` A write by the person who has the claim, also with the agent key of the person, gets no warning. |

### Refusal codes

Codes an agent can receive, and what each means.

| Code | Meaning |
|---|---|
| `NOT_FOUND` | A short code, board or repository named as the subject of the call does not exist; an `unlink_items` edge does not exist; or a `search` `traverse.from` does not resolve. On a write tool it also means the item exists but is archived. Writes resolve live items only. The message says that the item is archived, and it names `restore_item`. A reference to an archived item, for example the `target` of `link_items`, is `VALIDATION` with the same text. |
| `VALIDATION` | An argument is malformed, an enum value is outside its vocabulary, an argument does not apply to the item type, or something named as a *filter or reference* — a team, a repository filter, a parent, a metadata definition, a column — does not exist. A reference that does not resolve is `VALIDATION`; the call's own subject not existing is `NOT_FOUND`. **One exception:** `search`'s `traverse.from` is a reference and still answers `NOT_FOUND`, because a traversal's root is the subject of that traversal. |
| `FORBIDDEN` | The rule for the write refuses the principal. For an edit, the [edit rule](../capabilities/#the-edit-rule): the principal did not create the item and lacks `manage_<type>` on its board. For an edge, the [link rule](../capabilities/#who-can-write-relationships): the principal can edit neither end. For a move or a create: the principal lacks the board capability. The message names the capability. For `add_repository` and `update_repository`: the principal is not a member of the owner team and is not an organization admin. The message names the team. For `create_team`, `create_board` and `add_team_member`: the principal is not an organization admin and has no `manage_structure` on the organization. The message names the capability and the scope. One case has a longer message: a move (`transition_item`, `move_item`) of a request that the caller created, while the request is in the entry column. That message says that the item is a request, that the team of the board moves it, that the caller can edit, link and archive it, and which capability the move needs. A `create_item` that sends `work_class: planned` to a board that the caller does not manage is also `FORBIDDEN`. |
| `CONFLICT` | An optimistic-concurrency version mismatch. The refusal carries the current version and content. From `add_repository`: the directory has the slug already, or it has the repository already. From `promote_note`: the note has the mark of a promote, and `details.promoted_to` names the item. |
| `INVALID_TRANSITION` | The target column is not reachable from the item's current column in the board's transition graph. The refusal enumerates the allowed target columns. |
| `ITEM_NOT_ON_BOARD` | The item has no board placement, so it cannot be transitioned or moved. |
| `SAME_BOARD` | A `move_item` whose target is the board the task is already on. A document is different: see [`move_item`](#move_item). |
| `NOT_DELIVERY_BOARD` | A `move_item` whose target board is not a delivery board, or an `archive_completed` of a board that is not a delivery board. |
| `NO_ENTRY_COLUMN` | The target delivery board has no entry column to land the task in. |
| `RESTORE_BLOCKED` | The archived item's board, column, owning team or repository no longer exists. The refusal names what is missing. For a dictionary: its board is not the board `strategy`, or the board `strategy` has a live dictionary. |
| `DICTIONARY_NOT_ON_STRATEGY` | A `create_item` of a document with the template `dictionary`, or a `move_item` of a dictionary, to a board that is not the board `strategy`. An organization has one dictionary, on the board `strategy` (KAIROS-A-0079). The refusal names the board `strategy`. |
| `DICTIONARY_EXISTS` | A `create_item` of a second dictionary on the board `strategy`. The refusal names the dictionary that is there. Edit it, or archive it first. |
| `RELATIONSHIP_RULE` | The relationship type is not allowed between those two item types. For `impacts`: the source is not a document and not an ADR. |
| `CYCLE_DETECTED` | The edge would create a cycle. |
| `ALREADY_LINKED` | That edge already exists. For `impacts`: the item impacts that repository already. |
| `RENAME_NOT_NEEDED` | A `move_item` with `rename` to a board whose prefix the code has already. |
| `NOT_CLAIMABLE` | A `claim_item`, a `hand_off_item` or a `release_item` of a task that is not in a column that holds or keeps claims (Active or Blocked). |
| `NO_CLAIM` | A `release_item` of a task that has no claim. |
| `CLAIMED` | A `claim_item` of a task that a different person holds. The text names the holder. |
| `NO_OWNER` | A `release_item` of a strategy or an initiative that has no owner. |
| `NOT_BOARD_MEMBER` | A `hand_off_item` of a strategy or an initiative to a person who is not a member of the board of the item. |
| `TASK_DONE` | A `cancel_item` or a `move_item` of a task that is in a done column. Reopen the task first: `reopen_item`. |
| `TASK_LIVE` | A `purge_task` of a task that is not archived. Archive the task first: `delete_item`. |
| `TASK_NOT_DONE` | A `reopen_item` of a task that is not in a done column. |
| `NO_OPEN_COLUMN` | A `reopen_item` of a task whose board has only done columns. |
| `NO_DONE_COLUMN` | A `cancel_item` of a task whose board has no done column. |
| `AGENT_TRIAGE_OFF` | An agent's triage of intake on a board that lets a person decide. Intake is a support-lane task in the entry column. Its triage is `set_work_class`, `transition_item`, `move_item` or `cancel_item`. Write the proposal with `append_note`. |

**Each tool refuses an argument that it does not know.** Each tool has the
rule of the routes of the REST API, the tools that read too. The tool does not
ignore the argument, and it writes nothing. The schema of each tool shows
`additionalProperties: false`. The refusal is `VALIDATION`. Its text names the
argument and gives the arguments of the tool:

```text
VALIDATION: The call has the argument "column". This tool does not accept that argument. The arguments of this tool are: item_type, title, board, …
details: {"allowed":["item_type","title","board", …],"field":"column"}
```

An argument can be in an object of the call, such as `filter` of `search`. Then
the list is the list of that object. `whoami` has no arguments, and its refusal
says `This tool has no arguments.`

A call without an argument that the tool must have has a refusal of the same
form: `VALIDATION: The call does not have the argument "short_code". This tool
must have that argument. …`. An argument with the wrong JSON type is
`VALIDATION` too. The text names the argument and the type that the tool
must have. The tool can refuse a value of the correct type. Then the text
gives the values or the form that the tool accepts, and the value of the call:

```text
VALIDATION: The value of within.mode must be one of these values: fast, slow. The call gives "quick".
details: {"expected":"one of these values: fast, slow","field":"within.mode"}
```

A refusal does not contain the text of a library (KAIROS-T-0690). See
[An input that a route does not accept](../errors/#an-input-that-a-route-does-not-accept)
for the rule of the REST API.

`DEFINITION_IN_USE` protects a metadata definition that carries values.
[`delete_metadata_definition`](#delete_metadata_definition) returns it, as
`DELETE /api/metadata-definitions/{id}` does. See
[Tenant configuration](../rest/tenant-configuration/).

## Orientation

### `whoami`

Identity, organization role, teams, those teams' repositories, the boards where
the caller holds write capabilities, and the capabilities every member holds
implicitly. Each board of a team of the caller shows the capabilities that the
team gives with no grant (`(team <slug>, no grant)`). On the ADR board of the
team, these include `manage_adrs`. The section `Structure (manage_structure)`
gives the grants of [`manage_structure`](../capabilities/#manage_structure) of
the caller: on the organization, or on one board.

When the request uses an agent key, the section `Agent key scope` gives the
capabilities and the boards of the scope of the key. A key with no scope says
so. See [The scope of an agent key](../capabilities/#the-scope-of-an-agent-key).

The line `user name` gives the name that `hand_off_item` and the `claimed_by`
filter of `search` take. `@<user name>` in the content of an item mentions the
person (KAIROS-T-0556).

No arguments.

Refuses: nothing beyond transport-level authentication.

### `my_boards`

Boards in the organization, grouped by level, with column names and per-column
item counts for the caller's delivery boards. Each of those boards also has a
`moves:` line (KAIROS-T-0554). The line shows each live column in its order.
The flags of a column are in brackets: `entry`, `done`, `claims`, `WIP <limit>`.
An arrow gives the columns a card can move to from it. Read the allowed moves
here, not from a refused `transition_item`.

```text
  moves: Backlog [entry] → Todo | Todo → Active, Blocked | Active [claims, WIP 3] → Completed, Blocked | Blocked → Todo, Active | Completed [done]
```

Each delivery board and each initiative board has a `policy:` line with its
[board policies](#set_board_policy), for example
`policy: parent_gate on (a task moves to a claims column only while its
initiative is active); blockers_gate off (on: a task with an open blocker cannot
move to a claims or done column; off: the move gets a warning)`.

Each ADR board names its team
(`ADR board of the team <slug>`), or the organization. A team ADR board holds
the delivery ADRs of the team, with the prefix of the team. Each board of a team
of the caller has a `team capabilities:` line. This line shows the
capabilities that the team gives with no grant. On the ADR board of the team,
these include `manage_adrs`.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `board_level` | string | no | all levels | One of `strategy`, `initiative`, `delivery`, `adr`. The tool also accepts the name `level`. |

Refuses: `VALIDATION` for a `board_level` outside that vocabulary.

### `my_work`

Your work, in two lanes. The web page at `/` and `kairos me` show the same
work. REST: `GET /api/me/work`.

- Initiatives: the strategies and the initiatives that you own.
- Tasks: the tasks that you or your agent keys claim. The tasks in Todo,
  Active or Blocked under an initiative that you own, claimed by anyone or by
  nobody. The tasks that one of these tasks blocks.

Each item has a state: Todo, Active or Blocked. An item with an open blocker
has the state Blocked. An item in a column named Blocked has it too. An open
blocker is an item with a `blocks` edge to the item that is not in a done
column. A strategy or an initiative in Active or Monitoring is Active.

Each line gives the board, the column, the claimant or the owner, and why the
item is in your work. The tool does not show an item in a done column.

```text
# Your work

## Initiatives (1)

### Active
- ACME-I-0003 — Checkout (initiatives / Active; owner: Robin) [owned]

## Tasks (2)

### Active
- WEB-T-0012 — Card form (web / Active; claimed by Robin (agent)) [claimed]

### Blocked
- WEB-T-0013 — Receipt (web / Todo; no claim) [under_my_initiative, blocked_by_my_work]
  blocked by WEB-T-0012 — Card form
```

No arguments.

Refuses: an unknown argument, and names it.

### `list_members`

The members of the organization, of a team or of a board (KAIROS-T-0556).
Each member can read the list. Use it to find the person for `hand_off_item`,
for the `claimed_by` filter of `search`, or for an `@mention`.

Each line gives the user name, the display name, the email and the kind. The
kind is `person`, `service account` or `system actor` (the forge actor of the
organization). The lines are in the order of the user names.

```text
# Members of the board web-delivery (Web Delivery)
An organization admin holds each capability on each board. `list_members` with no argument gives the role of each member.
- jamie — Jamie Lee <jamie@example.test> · person · capabilities: manage_tasks (grant)
- robin — Robin Park <robin@example.test> · person · capabilities: manage_tasks, manage_documents, transition_items (team web, no grant)
```

- No argument: the members of the organization. Each line gives the role
  (`admin` or `member`).
- `team`: the members of the team.
- `board`: the members of the board and their capabilities. A capability
  comes from a grant, or `(grant)`. On a strategy or an initiative board, the
  `member` grant also gives `(board member, no grant)`. The team of the board gives
  `(team <slug>, no grant)`. The list does not show the capabilities of the
  organization admin role.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `team` | string | no | — | The members of this team. Slug or UUID. |
| `board` | string | no | — | The members of this board, with their capabilities. Slug or UUID. |
| `limit` | integer | no | `50` | The number of members in the result. The maximum is 200. |
| `offset` | integer | no | `0` | The number of members to skip. |

A result can be a part of the list. Then a line after the heading gives the
count of the members and the `offset` of the next part:
`Members: 120. Shown: 50 (limit 50, offset 0). Call again with offset 50 for
the next part.`

Refuses:

- `VALIDATION` for `team` and `board` together. Send one of them.
- `VALIDATION` for an unknown `team`.
- `NOT_FOUND` for an unknown `board`.
- an unknown argument, and names it.

### `list_repositories`

The repository directory. Each line of the output has these parts:

- the slug, the forge and the full name
- `owner`: the single owning team
- `owner's board`: the slug of the delivery board of that team. Send it to
  `board_items` as `board`.
- `open tasks`: the open task count
- `webhooks connected`, when a webhook connection exists

The open task count includes the linked tasks on all boards. The owner of a
repository does not choose the board of a task.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `team` | string | no | all teams | Narrow to one team's repositories. Slug or UUID. |
| `limit` | integer | no | `50` | The number of repositories in the result. The maximum is 200. |
| `offset` | integer | no | `0` | The number of repositories to skip. |

A result can be a part of the directory. Then its first line gives the count
of the repositories and the `offset` of the next part:
`Repositories in the directory: 120. Shown: 50 (limit 50, offset 0). Call
again with offset 50 for the next part.`

`GET /api/repositories` gives the slug of the board in `delivery_board_slug`,
and its id in `delivery_board_id`.

Refuses: `VALIDATION` for an unknown `team` — it is a filter value, not a
path.

The refusal of an unknown team lists the slugs of the teams of the
organization, 50 at most (KAIROS-T-0685). Each tool with a `team` argument
gives the same refusal:

```text
VALIDATION: The team "Kairos team" is not in the organization. Send the id or the slug of a team of the organization. The slugs are: kairos, skadi.
```

### `get_repository`

One repository in full. The output has these parts:

- the slug, the forge, the full name, the URL and the default branch. After
  the branch, the line says where it comes from: `set by hand`, `set by hand;
  the forge gives <branch>`, `from the forge`, or `from the forge; no fetch
  yet`. The code index builder indexes this branch. See
  [the branch that the builder indexes](../configuration/#the-branch-that-the-builder-indexes).
- `owner team`: the single owning team
- `owner's delivery board`: the delivery board of that team
- `open tasks (all boards)`: the open task count
- `webhooks`: `connected` or `not connected`
- `read token`: `not set`, or who set the read token of the builder of the
  code index, when, and the result of the last check. No tool gives or sets
  the token.
- `write-back`: `off (no write token)`, or `on` with who set the write token,
  when, and the result of the last write. With the token, Kairos writes a
  comment and the commit status `kairos` on each GitHub pull request that
  links to an item. No tool gives or sets the token. See
  [Turn on forge write-back](../../how-to/turn-on-github-write-back/).
- `code index build`: `on` or `off`. When it is `off`, the builder of the
  server does not build the code index of the repository.
  `update_repository` sets it.
- `code index breaker`: `not tripped`, or when the breaker tripped and why.
  A build job that stops its server (for example, it runs out of memory) or
  stalls trips the breaker. Then the builder starts no build of the
  repository. `rebuild_code_index` resets it. See
  [Build the code index again](../../how-to/build-the-code-index-again/#the-breaker-of-a-repository).
- `forge filters`: `default (admit everything)`, or each field whose include
  or exclude pattern is not the default. See
  [Connect a git forge](../../how-to/connect-a-git-forge/).
- `issue import`: `off`, or `every` and the import interval, then the last
  complete run, the next run, the count of issues in the record, and the last
  error. `update_repository` sets the interval.
- the team's description of how to work in the repository
- the documents and the ADRs that impact the repository
- `## ADRs of the owner team`: the other ADRs of the owner team
- `## ADRs that the code cites`: the ADR short codes that the code cites.
  They come from the newest code index of the default branch: its
  comments, its doc strings and its test names. Each line has the ADR, its
  column, and the count of its places and of its files. For a code that no
  ADR of the organization has, the line says so. 100 codes at most. The tool `references` of the code tools gives each place. See
  [the places that cite an ADR](../configuration/#the-code-tools-the-places-that-cite-an-adr).
- the in-flight branches and pull requests, each with its work item
- `## Not linked to an item`: the in-flight branches and pull requests that
  the forge filter admits, with no item that links to them

The delivery board is the board of the owner. A task that links to the
repository can be on the board of any team. The output has no list of the
tasks that link to the repository.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `repository` | string | yes | — | Slug or UUID. |
| `include_deleted` | boolean | no | `false` | Add the archived documents and ADRs of the two sections, each with the mark `[archived]`. |
| `offset` | integer | no | `0` | The number of rows to skip in `## In flight` and in `## Not linked to an item`. |

The sections `## In flight` and `## Not linked to an item` show 50 rows each,
newest first. When a section has more rows, its last line gives the count and
the next `offset`. The same `offset` applies to the two sections. An `offset`
after the end of a section shows the count of its rows:

```text
## In flight
- pull_request 412 [open] Retry the webhook — ACME-T-0012 (Retry the webhook https://github.com/acme/payments-api/pull/412)
- 3 more; offset 50.

## Not linked to an item
(none at offset 100: the list has 52)
```

The section `## Documents and ADRs that impact this repository` has one line
for each item. The documents come first, in the order of the short code. A
document shows its document type, if it has one, and its lifecycle. An ADR
shows its column, if it is on a board. An empty section shows `(none)`.

```text
## Documents and ADRs that impact this repository
- ACME-D-0004 — The vision of fidius · document (vision) · lifecycle: published
- ACME-D-0009 — An old plan · document · lifecycle: draft [archived]
- ACME-A-0002 — Plugins are dynamic libraries · adr · column: Decided
```

The section `## ADRs of the owner team` lists the ADRs on the boards of the
owner team that have no `impacts` link. Most older ADRs have no link. An ADR
with a link is in the first section of the repository that it names. Thus
each ADR shows one time only. The section does not show a superseded ADR.

Each line shows the column of the ADR. The newest ADR comes first. The
section shows 50 ADRs at most. When there are more, the last line gives the
count and names `team_pages`. An empty section shows `(none)`.

```text
## ADRs of the owner team
The ADRs on the boards of the team platform that no `impacts` link names, newest first. A superseded ADR is not listed.
- ACME-A-0007 — Retries use a fixed backoff · adr · column: Decided
- ACME-A-0005 — One database per tenant · adr · column: Draft
- 3 more: `team_pages platform` lists each ADR of the team.
```

`GET /api/repositories/{slug}` gives the same rows in `owner_team_adrs`.
`owner_team_adrs_total` gives the count of all of them.

Read these items with `get_item` before you plan work in the repository. See
[impacts](../glossary/#impacts).

Refuses: `NOT_FOUND` for an unknown repository, and for a repository that is
not live.

## The repository directory

`list_repositories` and `get_repository` read the directory. These two tools
write it. They do not link a task to a repository: `set_repository` does
that.

`update_repository` also changes the slug and the owner team of a repository
(KAIROS-T-0443). `delete_repository` removes a repository from the directory.

### `add_repository`

Adds a repository to the directory. The rule, the defaults and the refusals
are those of `POST /api/repositories`.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `forge` | string | yes | — | `github`, `gitlab` or `other`. |
| `repo_full_name` | string | yes | — | The name on the forge: 2 parts for `github` (`owner/repo`), 2 or more for `gitlab`, 1 or more for `other`. |
| `repo_url` | string | yes | — | The URL of the repository for a browser: `http` or `https`, with no user name and no password. |
| `team` | string | yes | — | The one owner team. Slug or UUID. |
| `slug` | string | no | made from `repo_full_name` | The slug of the repository in Kairos. `acme/payments-api` gives `acme-payments-api`. It must match `^[a-z0-9][a-z0-9-]{1,62}$`. |
| `default_branch` | string | no | the default branch of the forge | The default branch: a branch name that git accepts. With a value, the branch is set by hand. With no value, the code index builder follows the default branch of the forge, and the value is `main` until the first fetch. |
| `description` | string | no | empty | How to work in the repository. `get_repository` shows it to each agent. |

The glossary gives the full rule of `repo_full_name`, `repo_url` and
`default_branch`: see
[the form of the fields](../glossary/#the-form-of-the-fields-of-a-repository).

Requires membership of the owner team, or the organization admin role. The
condition is `manage_tasks` on the delivery board of the owner team, and each
member of the team has it. When the team does not have one live delivery
board, only an organization admin can add the repository.

The result is one line:

```text
Added repository fidius: github colliery-io/fidius (owner: colliery-io, default branch main).
```

Refuses: `VALIDATION` for a `forge` outside that vocabulary, and for an
unknown `team`. `VALIDATION` for a `slug` that does not match the form.
`FORBIDDEN` when the caller is not a member of the owner team and is not an
organization admin. `CONFLICT` when a repository has the slug already.
`CONFLICT` when the directory has the pair of `forge` and `repo_full_name`
already. `VALIDATION` with `details.field` for a `repo_full_name`, a `repo_url`
or a `default_branch` that does not have the form of its field:

```text
VALIDATION: The repo_full_name "acme/fidius.git" ends with .git. Remove .git from the end. For the forge github, the name has 2 parts, for example acme/payments-api.
details: {"field":"repo_full_name"}
```

### `update_repository`

Changes the description, the default branch, the URL and the code index
builder of a repository. It also changes the source of the default branch. The rule is that of `PATCH /api/repositories/{slug}`,
for the current owner team.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `repository` | string | yes | — | Slug or UUID. |
| `description` | string | no | no change | The new text on how to work in the repository. It replaces the full text. An empty string removes the text. |
| `default_branch` | string | no | no change | The new default branch: a branch name that git accepts. The branch is then set by hand: the code index builder indexes it, whatever the forge says. |
| `default_branch_source` | string | no | no change | `forge` or `hand`. `forge` removes the branch set by hand: the builder follows the default branch of the forge again. `hand` keeps the current branch by hand. Do not send `forge` with `default_branch`. |
| `repo_url` | string | no | no change | The new URL for a browser: `http` or `https`, with no user name and no password. |
| `code_index_build` | string | no | no change | `on` or `off`: the builder of the code index of the server, for this repository. |
| `slug` | string | no | no change | The new slug. |
| `team` | string | no | no change | The new owner team. Slug or UUID. |
| `forge_filters` | object | no | no change | The forge filters (organization admin only). Keys: `branch_name`, `pr_head_branch`, `pr_title`, `pr_author`, `issue_title`, `issue_author`. Each value has `include` and `exclude`, Rust regular expressions of 512 bytes at most. Send only the fields and the patterns to change. |
| `issue_import_interval` | string | no | no change | How often the repository takes its open issues, with no webhook (organization admin only). `off`, or a number with a unit: `90s`, `15m`, `1h`, `1d`. The shortest interval is `1m` and the longest is `30d`. The repository needs a read token. The tool refuses another form and names the argument. See [Take issues on an interval](../../how-to/connect-a-git-forge/#6-take-issues-on-an-interval-with-no-webhook). |

The call must have one or more of `description`, `default_branch`,
`default_branch_source`, `repo_url`, `code_index_build`, `slug`, `team`,
`forge_filters` and `issue_import_interval`. The tool refuses an invalid or too long pattern with
`VALIDATION`. The error names it (`forge_filters.pr_author.exclude`).
Each checkout keeps the slug as `repository` of its `[[project]]` in
`config.toml`. A new slug breaks that reference: run `/kairos:bootstrap` in
the checkout again.

Requires membership of the owner team, or the organization admin role. A new
`team` also requires membership of that team.

The result names the arguments that changed the repository:

```text
Updated repository fidius: description, default_branch.
```

`code_index_build: "off"` stops the builds of the repository. Kairos cancels
the build jobs of the repository that wait for a server, and their runs end as
failed. The builder adds no new job. Kairos does not stop a build that runs
now, but the build gets no further try. The result gives the number of cancelled jobs:

```text
Updated repository fidius: code_index_build. The code index builder is off: 1 waiting build job(s) cancelled.
```

`code_index_build: "on"` starts the builds again.

The tool writes only when a value is different. A call can have only the
values that the repository has. That call is a success and it writes nothing.
`PATCH /api/repositories/{slug}` does the same.

```text
No change to repository fidius: it has these values already.
```

### `delete_repository`

Removes a repository from the directory, as `DELETE /api/repositories/{slug}`
does.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `repository` | string | yes | — | Slug or UUID. |

Requires the organization admin role or `manage_structure` on the
organization.

Refuses: `NOT_FOUND` for an unknown repository. `CONFLICT` while a live task
links to the repository or a forge connection uses it.

Refuses: `VALIDATION` with `details.field` for a new value that does not have
[the form of its field](../glossary/#the-form-of-the-fields-of-a-repository).
Its result is `No change to repository fidius: it has these values already.`

Refuses: `VALIDATION` when the call has none of the four arguments.
`VALIDATION` with `details.argument` for a `code_index_build` that is not `on`
or `off`.
`NOT_FOUND` for an unknown repository. `FORBIDDEN` when the caller is not a
member of the owner team and is not an organization admin.

### `rebuild_code_index`

Ask Kairos to build the code index of a repository again. The builder of
the server makes a full index of the head of the default branch. The new
index replaces the index of that commit.

| Argument | Type | Required | Meaning |
|---|---|---|---|
| `repository` | string | yes | The repository, by slug or UUID. |

The answer names the run. Read the runs of the repository on its page in
the GUI, with `kairos repos builds <slug>`, or with
`GET /api/repositories/{slug}/code-indexes/builds`.

The request resets the breaker of the builder for the repository (the line
`code index breaker` of `get_repository`).

Refusals:

- `CODE_INDEX_BUILD_RUNNING`: a run of the repository is active. Wait for
  its end.
- `CODE_INDEX_BUILD_OFF`: the repository has the builder off. Set
  `code_index_build` to `on` with `update_repository` or `kairos repos
  update`, then ask again.
- `CODE_INDEX_BUILDER_OFF`: the deployment has no builder
  (`KAIROS_CODE_INDEX_DIR` is not set).
- `FORBIDDEN`: you are not a member of the owner team, and not an
  organization admin.

## Reading

### `board_items`

The items on a board, grouped by column: short code, type and title.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `board` | string | yes | — | Slug or UUID. |
| `column` | string | no | all columns | Column name or UUID. |
| `repository` | string | no | all | Narrow the tasks to those that link to this repository. Slug or UUID. |
| `include_deleted` | boolean | no | `false` | Add archived cards back, each marked `[archived]`, in the column they were put away in. Columns that have since been removed appear only when this is true, and only carrying archived cards. |
| `limit` | integer | no | `200` | The number of items in the result. The maximum is 1000. Kairos changes a larger value to 1000. |
| `offset` | integer | no | `0` | The number of items to skip. |
| `team` | string | no | all | Narrow the strategies and the initiatives to those of this team, from tasks or set by hand. Slug or UUID. Tasks and ADRs do not change. Do not send with `no_team`. |
| `no_team` | boolean | no | `false` | Narrow the strategies and the initiatives to those with no team. Do not send with `team`. |
| `details` | boolean | no | `false` | Add to each card the date of its last change and its metadata values. See below. |

The result has 200 items at most by default. The filters (`column`,
`repository`, `team`, `no_team`, `include_deleted`) apply before `limit` and
`offset`. The order of
the items is: the position of the column, then the type, then the short code.
The order of the types is: strategy, initiative, task, ADR.

When the result is a part of the board, its first lines say so:

```
The board has 340 items. This result shows 200 (limit 200, offset 0). To read the next part, call the tool with offset 200.
```

Call the tool again with that `offset` until you have each part. In such a
result, the number after the name of a column is the count for that result.

A removed column can be named as `column` only while `include_deleted` is
true; otherwise it is not among the board's columns and is refused as unknown.

A strategy or an initiative that has teams carries `[teams: a, b]`, with the
slugs of the teams. See [the teams of an item](#the-teams-of-an-initiative-or-a-strategy).

A task with a claim carries `[claimed by Alice]`, or `[claimed by Alice
(agent)]` when the agent of the person made the claim.

A strategy or an initiative with an [owner](../glossary/#owner) carries
`[owner: Alice]`.

A [cancelled](../glossary/#cancel) task carries `[cancelled]`.

A task with a [state](#set_state) carries `[state: branch x; next: …]`: its
branch and its next step, the ones with a value.

A column with a WIP limit shows its live count against the limit after its
name: `## Active (2) [WIP 2/3]`. A column with more live items than its limit
shows `[WIP 4/3, over the limit]`. The count is of the whole column, not of the
result. Kairos does not refuse a move into a column over its limit. The answer
of `transition_item` or `move_item` ends with a warning line, for example
`Warning: the column Active has 4 items, over its WIP limit of 3. Your move of
ACME-T-0012 is recorded.`

The answers of `create_item`, `cancel_item` and `restore_item` end with the
same line.

With `details: true`, each card ends with the date (UTC) of its last change
and its metadata values, in slug order:

```text
- ACME-T-0012 [bug] Refunds lose the currency [claimed by Alice] {updated 2026-10-08; priority: high}
```

Use it to pick or triage work without a `get_item` for each card. Without
`details`, the list stays compact.

A card with dependencies that count carries `[blocked by N]`, `[blocks N]`, or
both. A `blocks` edge does not count when the item at either end is in a done
column. It does not count when the item at the other end has the `[archived]`
mark. A card in a done column carries neither tag.

Refuses: `NOT_FOUND` for an unknown or archived board; `VALIDATION` for a
`column` that is not on that board, and the refusal lists the board's columns,
for an unknown `repository`, for an unknown `team`, and for `team` with
`no_team`.

### `board_flow`

The flow metrics of a board for a range of days (UTC).

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `board` | string | yes | — | Slug or UUID. |
| `from` | string | no | 89 days before `to` | The first day of the range, `YYYY-MM-DD`. |
| `to` | string | no | today | The last day of the range, `YYYY-MM-DD`. |

The result has 4 parts:

- **Cycle time**: the days from the start to the done time of each done item
  of the range. The count, the mean, the median and the 85th percentile, then
  each item.
- **Throughput per week**: the done items and the cancelled tasks of each week.
  Weeks start on Monday.
- **Work in progress**: each item that is not done and has a start, with its
  age in days, oldest first.
- **Cumulative flow**: the items of each column at the end of each of the
  last 14 days. `GET /api/boards/{id}/flow` gives each day of the range.

The definitions:

- The start of an item is its first entry into a start column. A start column
  has the flag `claims` (the Active column of a delivery board). When the board
  has no such column, the column with the name Active is the start column.
- An item is done when it is in a done column now. Its done time is its last
  entry into a done column.
- A cancelled task counts apart. It is not in the done count, and it has no
  cycle time.
- The items of the board count. An archived item counts until its archive: an
  item archived from a done column keeps its done time and its cycle time. An
  archived item is not in progress, and the cumulative flow drops it on the day
  of its archive. The history names the columns by their name at the time of
  the move. A column with a new name shows its old
  name in the cumulative flow.

Refuses: `NOT_FOUND` for an unknown board; `VALIDATION` for a date that is not
`YYYY-MM-DD`, a `from` after `to`, or a range of more than 366 days.

### `get_item`

Full detail of one item: type, board and column, version, full markdown
content and its notes, metadata values, and relationships — parent chain, children, blockers,
supporting documents. It also names the creator.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The item's short code. |

The line `board` of a live item ends with `next:` (KAIROS-T-0554). It names
the columns a card can move to from its column, in board order. With no
transition from the column, it says `next: none`:

```text
- board: platform-delivery / column: Todo · next: Active, Blocked
```

The line `created by` gives the person who created the item and the time. It
gives the display name and the user name. The
[edit rule](../capabilities/#the-edit-rule) lets the creator edit the item:

```text
- created by: Robin (robin) on 2026-10-16T22:04:24Z
```

A document has no column. It shows its owner board in the line `owner board`,
for example `- owner board: platform-delivery`. Each document has an owner
board.

A document or an ADR that impacts a repository has the line `impacts` in the
section of the relationships. The mark `[archived]` shows a repository that is
not live.

```text
- impacts: repository fidius (github colliery-io/fidius); repository old-lib (github acme/old-lib) [archived]
```

An item with [external references](../glossary/#external-reference) has one
line `external` per reference, oldest first. The line has the label, then the
URI in angle brackets. A reference with no label shows the URI only.

```text
- external: RFC: zen as the policy engine <https://example.com/wiki/rfc-42>
- external: <https://example.com/design>
```

An ADR that another ADR [amends](../glossary/#relationship) has the line
`amended by`. It lists the amending ADRs in the order of the edges, oldest
first. Read the decision, then each change in turn. The amending ADR has the
line `amends` (KAIROS-A-0069):

```text
- amended by: ACME-A-0066 — A move of an initiative takes its owner only when the caller asks with claim: true
- amends: ACME-A-0042 — An initiative moves to Active only with an owner
```

A task in Active has the line `claim`. A task in Active with no claim has the
line too:

```text
- claim: Alice (agent) since 2026-10-09T10:00:00Z
- claim: none. The task is free: claim_item takes it, and a move to Active gives it.
```

A strategy and an initiative have the line `owner`, in each column and in the
archive:

```text
- owner: Alice since 2026-10-12T10:00:00Z
- owner: none. A member of the board can claim it (claim_item).
```

A [cancelled](../glossary/#cancel) task has the line `cancelled`, with who
cancelled it and the reason:

```text
- cancelled: 2026-10-09T10:00:00Z by Robin. Reason: The customer does not need it.
```

A task with a [state](#set_state) has the line `state`. It gives the fields
with a value, the next step, and who wrote it when:

```text
- state: branch acme-t-0012 at abc1234, pushed, PR https://github.com/acme/app/pull/12. Next: Wait for CI. (updated 5 min ago by Robin)
```

A task with a [review state](#set_review) has the line `review`
(KAIROS-T-0718):

```text
- review: needed (set 5 min ago by Robin)
```

An initiative or a strategy has the line `teams` in the section of the
relationships. Each team shows where it comes from. An item with no team has
the line too:

```text
- teams: skadi (from tasks); weir (set by hand); kairos (from tasks, set by hand)
- teams: none (no task on a team board, and no team set by hand)
```

Archived items are returned, marked with the instant they were put away. The
column reported for an archived card is the name of the column it was put away
in, which may since have been removed from the board: a removed column is
soft-deleted rather than dropped, and this lookup deliberately ignores that so
"which column was this in?" stays answerable.

The `blocked by` and `blocks` lines list each `blocks` edge of the item. The
mark `[done]` shows an item in a done column. The mark `[archived]` shows an
item that someone put away. An edge to an item with a mark does not block. The
mark `[cancelled]` shows a cancelled blocker, which still blocks
(KAIROS-T-0567). When
the item itself is in a done column, the two labels change to
`blocked by (resolved: this item is done)` and
`blocks (resolved: this item is done)`.

A retired short code finds the item. The first line has the current code, and
a notice follows it:

```text
# SKADI-T-0577 — Find the downloads

> **RETIRED CODE** The code COLLIERY-T-2430 is retired. The current code of this item is SKADI-T-0577. Use the current code.
```

An item that had other codes has the line `retired codes`:

```text
- retired codes: COLLIERY-T-2430
```

An item with [edge proposals](#propose_edge) has the section `## Proposals`
after the relationships (KAIROS-T-0558). The item can be the source or the
target. The section lists each pending proposal first, at most 20. Then it
lists the 5 newest decided proposals.

Each line gives the state, the edge, the id, the proposer and the reason. A
decided line also gives the decider and the date. When more proposals wait,
the last line gives their count.

```text
## Proposals
- pending: ACME-T-0012 -[blocks]-> ACME-T-0014 (5f0c…), proposed by Robin on 2026-10-17. Why: The first writes the table.
- confirmed: ACME-T-0012 -[parent]-> ACME-I-0003 (9a1d…), proposed by Robin on 2026-10-16, confirmed by Sam on 2026-10-18. Why: The same goal.
```

An item with notes has the section `## Notes` after the relationships. Each
line gives one note: its id, its date, its author and its first line. Cite a
note by its id: "note n-3 on ACME-T-0012".

```text
## Notes
- n-1 (2026-10-11, Robin): Found the cause.
- n-2 (2026-10-12, Sam): The fix is in PR #12.
```

The text of each note follows the content, under its heading. A note is not
part of the content: `get_history` and `update_item` do not show it.

```text
## Content
The content of the item.

## Note n-1 (2026-10-11, Robin)

Found the cause.

## Note n-2 (2026-10-12, Sam)

The fix is in PR #12.

Promoted to ACME-A-0029.
```

The section `Terms used` is at the end (KAIROS-T-0735). It gives the meaning of
each [dictionary](../glossary/#dictionary) term that the title, the content or
the notes use. The longest term is first. A term in code is not used. Each
item uses the dictionary of the organization, on the board `strategy`
(KAIROS-A-0079). A dictionary does not have this section.

```text
## Terms used
- production document (noun): The document that the team gives to its users.
- board (noun): A set of columns for the work of a team.
```

Refuses: `NOT_FOUND` only when the short code names nothing at all.

### `get_history`

The history of an item, newest first: its content versions, then its
activity. A version line has the version, the time and the editor.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The item's short code. |
| `limit` | integer | no | `20` | Maximum rows of each list: the versions and the activity. Clamped to 1–200. |
| `offset` | integer | no | `0` | The number of versions to skip, newest first. The activity starts at its newest row. |
| `version` | integer | no | — | Return that snapshot's full title and content instead of the list. |

The list can show a part of the versions. Then its last line gives the count
of the older versions and the next `offset`: `- 18 more; offset 20.`

Archived items' history is returned, marked archived.

A version that an agent made with the agent key of a person shows the editor
as "Alice (agent)". A rename that an agent made has the same mark.

The list of an item that a move renamed has the section **Renames**. Each
line has the old code, the new code, the time and who did the move:

```text
## Renames
- COLLIERY-T-0100 -> SKADI-T-0001 — 2026-10-03T18:20:00Z by Robin
```

The list ends with the section **Activity**. Each line tells what changed,
the time, and who did it. The newest line is first. The list has these rows:

- a move: `moved from Todo to Active`;
- a cancel, with the reason;
- a move to a different board, with the boards and the columns;
- a claim: `claim taken: Robin`;
- a hand-off: `claim handed off: Robin -> Sam`;
- a release: `claim released: Sam`. A claim that a move ends shows
  `claim ended: Sam (transition)`;
- a lane: `lane: planned -> support`;
- a metadata value: `metadata priority: high`, or
  `metadata priority: cleared`;
- a state write: `state changed: branch, commit`. The line names the
  fields that changed. `get_item` shows the values;
- a link: `link added: blocks ACME-T-0001 -> ACME-T-0002`, and
  `link removed`. A link between two items is in the activity of the two
  items;
- the repository, the lifecycle, the owner board, a rename, the create, an
  archive and a restore.

A row of a different kind shows its action and its details. The names come
from the directory of the organization, not from the ids. An agent's change
has the mark "(agent)". When the list does not show each row, the last line
tells how many older rows there are:

```text
## Activity
- state changed: branch, commit — 2026-10-17T10:05:00Z by Robin (agent)
- claim taken: Robin — 2026-10-17T10:00:00Z by Robin (agent)
- moved from Todo to Active — 2026-10-17T10:00:00Z by Robin (agent)
- 4 older rows are not shown. Send a larger `limit` to see them.
```

An item with no activity rows has the line
`No activity is recorded for this item.`

Refuses: `NOT_FOUND` for an unknown short code, or for a `version` with no
snapshot. `VALIDATION` for `version` with `limit` or with `offset`.

### `list_metadata_definitions`

The metadata definitions of the organization, by slug. For each one: the
name, the type, the values of an enum in their order (the scale, for example
the priorities from the first to the last), and the entity types that it
applies to.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `entity_type` | string | no | all | Only the definitions that apply to this type: `strategy`, `initiative`, `task`, `document`, `adr`. |

```text
- `priority` Priority (enum) — values, in order: low, medium, high, critical — applies to: all types
```

Use the slugs in `set_metadata`, and in `search` (`filter.metadata`,
`filter.metadata_missing`). REST: `GET /api/metadata-definitions`.

Refuses: `VALIDATION` for an unknown `entity_type`.

### `list_templates`

The templates of the organization, by slug. For each one: the name, and the
metadata that it puts on an item, with the default value and the required
flag.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `slug` | string | no | all | Show this one template, with its full content. |

REST: `GET /api/templates` and `GET /api/templates/{id}`.

Refuses: `NOT_FOUND` for a `slug` that names no template.

## Searching

### `search`

Full-text query, structured filter and graph traversal, composing freely. At
least one of `q`, a constraining `filter`, or `traverse` is required. Results
are compact and grouped by type.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `q` | string | no | — | Full-text query. Websearch syntax: quoted phrases, `OR`, `-negation`. Must not be blank. When `q` is a short code, the result also has the item with that code, first. When `q` is a retired code, the result has the item with its current code, and a notice gives the current code. |
| `filter` | object | no | — | See below. Fields AND together. |
| `traverse` | object | no | — | See below. |
| `sort` | object | no | `relevance` descending with `q`, else `created_at` descending | See below. |
| `limit` | integer | no | `25` | Page size, 1–100. |
| `offset` | integer | no | `0` | Offset into the combined result set. Must not be negative. |

`filter`:

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `entity_type` | array of string | no | all | `strategy`, `initiative`, `task`, `document`, `adr`. Must not be an empty array. |
| `board` | string | no | — | Items on this board, by slug or UUID. A document is not a card, so this filter gives no document. The old name `board_id` is an alias. |
| `column` | string | no | — | Items in this column, by name (with `board`) or UUID. A name with no `board`, or a name the board does not have, is refused and the refusal lists the columns. The old name `column_id` is an alias. |
| `team` | string | no | — | Tasks of this team, by slug or UUID. The old name `team_id` is an alias. |
| `repository` | string | no | — | The items of this repository: see below. Slug or UUID of a live repository. |
| `task_type` | array of string | no | all | `task`, `bug`, `tech_debt`, `support`. Must not be an empty array. |
| `work_class` | array of string | no | all | `planned`, `support`. Must not be an empty array. |
| `is_bucket` | boolean | no | both | Bucket or non-bucket initiatives. |
| `metadata` | object of string to string | no | — | Conditions keyed by metadata-definition slug. Values allow a trailing `*` glob. Keys must not be blank. |
| `created_after` | RFC 3339 string | no | — | Items created strictly after. |
| `created_before` | RFC 3339 string | no | — | Items created strictly before. Must be later than `created_after`. |
| `updated_after` | RFC 3339 string | no | — | Items with a last change strictly after. |
| `updated_before` | RFC 3339 string | no | — | Items with a last change strictly before. Must be later than `updated_after`. For the items with no change in 90 days, send the time 90 days ago. |
| `metadata_missing` | array of string | no | — | Items with no value for each of these metadata-definition slugs. Must not be empty, and each slug must name a definition. |
| `claimed_by` | string | no | — | Tasks that this person has the claim on. A claim that an agent key of the person made counts. Send `me`, a user id, an email or a user name. |
| `owned_by` | string | no | — | Strategies and initiatives that this person owns. Send `me`, a user id, an email or a user name. |
| `created_by` | string | no | — | Items that this person created. Send `me`, a user id, an email or a user name. |
| `include_deleted` | boolean | no | `false` | Include archived items, marked `[archived]`. Composes with everything, `q` and `traverse` included, and counts on its own as a constraining filter. |

The first line gives the count and the part:
`12 found, 10 shown (limit 10, offset 0). 2 more; offset 10.` The words
`more; offset` show when more rows come.

A task line gives the type, the board and the column. Then, as it applies, it
gives who has the claim, the count of open blockers and the repository. The
search reads these facts with the results. Thus you do not need `get_item`
for each task:

```text
- KAIROS-T-0511 — Retry the webhook [bug] kairos/Active, claimed by Robin (agent), blocked by 1 [repo:kairos]
- KAIROS-T-0512 — Write the guide [task] kairos/Todo
```

An open blocker is a live item that is not done, or a cancelled task. A task
in a done column has no open blockers. `board_items` counts them the same way.

`filter.repository` gives three types of item:

- the tasks that link to the repository, on all boards
- the documents that impact the repository
- the ADRs that impact the repository

The filter gives no strategy and no initiative. With `team`, `task_type` or
`work_class`, only tasks match. `entity_type` makes the result smaller, as for
each filter.

A triage pass reads the bugs with no priority and no change in 90 days with
one call:

```json
{"filter": {"entity_type": ["task"], "task_type": ["bug"], "board": "platform-delivery",
            "metadata_missing": ["priority"], "updated_before": "2026-07-13T00:00:00Z"}}
```

`traverse`:

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `from` | string | yes | — | The starting item's short code. |
| `relationships` | array of string | yes | — | `parent`, `supports`, `informs`, `supersedes`, `amends`, `blocks`. Must not be empty. |
| `direction` | string | yes | — | `outbound`, `inbound`, `both`. |
| `depth` | integer | no in the schema | — | 1–10. Optional in the schema, but a traversal without it is refused: the field is deliberately not defaulted so that a missing depth is a typed refusal rather than a silent choice. |

`traverse` follows the edges between two items. An `impacts` link goes to a
repository, so `impacts` is not a value of `relationships`. Use
`filter.repository` to find the items that impact a repository.

`sort`:

| Field | Type | Required | Description |
|---|---|---|---|
| `field` | string | yes | `created_at`, `updated_at`, `title`, `relevance`. `relevance` requires `q`. |
| `order` | string | yes | `asc`, `desc`. |

Refuses: `VALIDATION` for a blank `q`, an empty enum array, a blank metadata
key, an inverted date range, an empty `metadata_missing` or a slug in it that
names no definition, a field with its old name in the same filter (`board`
with `board_id`, `team` with `team_id`, `column` with `column_id`), an
unknown `board` or `team`, a person in `claimed_by`, `owned_by` or
`created_by` that is not a member of the organization, a
missing `traverse.depth`, a `depth` of 0 or above 10, an empty
`relationships`, a request with no query, no constraining filter and no
traversal, or an unknown `filter.repository`. `NOT_FOUND` for a `traverse.from`
that does not resolve — the one reference on this surface that answers
`NOT_FOUND` rather than `VALIDATION`.

Every request except one carrying `filter.repository`, `filter.board` or
`filter.team` is validated before a database connection is taken. These
filters need a connection to resolve the slug, so the server validates the
request after it resolves them.

## Writing content

### `create_item`

Creates a work item and returns its new short code.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `item_type` | string | yes | — | `strategy`, `initiative`, `task`, `document`, `adr`. |
| `title` | string | yes | — | The item's title. |
| `board` | string | no (yes for a document) | see below | Target board, slug or UUID. For a task, the board decides the team of the task. For a document, it is the owner board: it is required, and it has no default. |
| `parent` | string | no | — | Parent item's short code. Creates the `parent` edge. For a document or an ADR, creates the `supports` edge. |
| `content` | string | no | empty, or the template's | Initial markdown content. |
| `template` | string | no | — | Documents only. Template id, slug or name. |
| `task_type` | string | no | `task` | Tasks only. `task`, `bug`, `tech_debt`, `support`. |
| `repository` | string | no | — | Tasks only. Slug or UUID. An optional link that says where the code is. Any live repository, of any team. It does not choose the board. |
| `work_class` | string | no | see below | Tasks only. `planned`, `support`. |
| `from_team` | string | no | — | Tasks only. The team that sends the task. Slug or UUID. When it is not the team of the board, the task is a request: entry column, support lane, also for a manager of the board or an org admin. An agent in a board session sends its own team. Kairos does not keep it on the task: name the sending team in the body. |
| `hypothesis` | string | no | — | Strategies only. |
| `complexity` | string | no | — | Initiatives only. `xs`, `s`, `m`, `l`, `xl`. `set_complexity` changes it later. |
| `bucket_type` | string | no | — | Initiatives only. `tech_debt`, `bug`, `ad_hoc`. Makes the initiative a bucket rather than a dated one; `is_bucket` is derived from it. |
| `decision_maker` | string | no | — | ADRs only. |
| `decision_date` | string | no | — | ADRs only. `YYYY-MM-DD`. |

`board` may be omitted when the tenant has exactly one live board of the
matching level. A document is different: it must have `board`. For a task, that level is `delivery`. A `repository` does not
replace `board`. The tool has no `team` argument.

An ADR goes on the board that `board` names. When the organization has more
than one ADR board, send `board`. For a delivery
ADR, send the ADR board of your team.

An ADR can have a `parent`. The `parent` names a strategy, an initiative or a
task. The tool creates the `supports` edge from that item to the ADR. The
caller needs `manage_adrs` on the ADR board, and no capability on the board of
the parent. A member of the team has `manage_adrs` on the ADR board of the team
with no grant. The caller creates the ADR, so the link rule lets the caller link
it.

The same applies to each `parent`: the caller who creates an item can link it
to that parent.

An initiative goes to the entry column of its board (Discovery). It reaches
each other column by a move (KAIROS-T-0643).

A task goes to the entry column of its board. When that column holds claims,
the create is a move into a claims column. Then the policy `parent_gate` of
the board applies to the `parent`. An initiative that is not active gives
`PARENT_NOT_ACTIVE`, and Kairos creates nothing.

A document must have an owner board. The call must have `board`, with
`parent` or with no `parent`. The owner board can be a live board of each
level. The caller needs `manage_documents` on that board.

The code of the document gets the prefix of that board. With `parent`, the
document also supports that item: a strategy, an initiative or a task. The
parent gives no owner board.

A call with no `board` gets `VALIDATION`, and `details.argument` is `board`:

```text
VALIDATION: The call has no `board`. Each document must have an owner board. Send `board`: the slug or the id of the board that owns the document. The code of the document gets the prefix of that board.
```

The document is not a card of its owner board. It has no column, and
`board_items` does not show it. See [owner board](../glossary/#owner-board).

`repository` does not apply to a document. To say that a document impacts a
repository, create the document. Then call [`link_items`](#link_items) with
the relationship `impacts`.

The result for a document is one line. It has one of two forms:

```text
Created document PLATFORM-D-0004: The vision of fidius (version 1), owner board platform-delivery.
Created document PLATFORM-D-0006: Rollout plan (version 1), owner board platform-delivery, supports ACME-I-0002.
```

The default of `work_class` depends on the caller:

- The caller holds `manage_tasks` on the board: `support` for a task of type
  `support`, and `planned` for each other type.
- Each other caller: the task is a request, and its work class is `support`
  for each task type.

Any member can send a request to any team. The caller names the delivery board
of that team in `board`. The request goes to the entry column of that board,
through the computed `file_backlog` capability. The `repository` is optional
for a request.

**`create_item` has no column argument, and that is deliberate.** A new item
always lands in its board's first column, and `transition_item` is the only way
work moves. Accepting a column would let an agent place an item past states the
board's transition graph exists to enforce — something a person using the GUI
cannot do. The restriction is stated rather than left to be inferred, because an
agent reading the schema cannot ask whether a missing field is a rule or an
oversight.

`bucket_type` and `decision_date` used to be missing too, which was an oversight
rather than a rule: an initiative created over MCP could never be a bucket, and
both fields were already *readable* through `get_item`. They are accepted now.

Refuses: `VALIDATION` for an unknown `item_type`; for a `task_type`,
`work_class`, `complexity` or `bucket_type` outside its vocabulary; for a `decision_date` that is not `YYYY-MM-DD`; for a type-specific
argument passed with the wrong `item_type`, naming the type it belongs to, with
the argument in `details.field` (the arguments of the table above with "Tasks
only" and the like; a test keeps the table, the schema and the server equal,
KAIROS-T-0691); for a
document with no `board`, or whose parent is not a strategy, initiative or
task; for a document with `repository`; for a `parent` relationship that the type rules do not allow, with the
rule in the message; for a `parent` that does not name a live item; for an unknown template,
and for a template *name* that matches more than one template, which asks for
the id or slug instead; when no live board of the required level exists; when
several do, listing their slugs; and for an unknown `repository`.
`NOT_FOUND` for an unknown `board`. `FORBIDDEN` when the caller
lacks `manage_<type>` on the resolved board. A task is different: a caller without
`manage_tasks` sends a request. That caller gets `FORBIDDEN` for a board that
is not a delivery board, and for `work_class: planned`.

A document with a template that has typed sections: the content must pass
each typed section, else `SECTIONS_NOT_PASSED`. With no `content`, the tool
checks the content of the template. See
[Typed sections on an MCP write](#typed-sections-on-an-mcp-write).

A create that fails writes nothing. The tool does each check before the first
write. The tool writes the item and its edge in one transaction. No item,
history, activity or event stays behind. The tool does not use the short code
number of that create again.

### `update_item`

Replaces an item's full content, and optionally its title, under optimistic
concurrency.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The item's short code. |
| `content` | string | yes | — | The full replacement markdown content, not a patch. |
| `version` | integer | yes | — | The version this edit is based on, as read from `get_item`. |
| `title` | string | no | unchanged | New title. |

The [edit rule](../capabilities/#the-edit-rule) applies. The caller created the
item, or holds `manage_<type>` on its authorization board, or is an
organization admin.

Refuses: `NOT_FOUND` for an unknown short code or an archived item;
`FORBIDDEN` when the edit rule refuses the caller;
`CONFLICT` for a stale `version`, carrying the current version and content;
`SECTIONS_NOT_PASSED` for content that does not pass the typed sections of the
template of a document.

### `edit_item`

Targeted server-side search and replace against the item's current content.
Retries once on a concurrent-edit race.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The item's short code. |
| `search` | string | yes | — | Exact text to find in the current content. Must not be empty. |
| `replace` | string | yes | — | Replacement text. |
| `replace_all` | boolean | no | `false` | Replace every occurrence. At the default, the match must be unique. |

Refuses: `VALIDATION` for an empty `search`, for a `search` not found in the
current content, and for a `search` matching more than once while
`replace_all` is false — the refusal states the occurrence count. Otherwise as
`update_item`, and the same edit rule applies.

Sometimes the `search` text is not in the content, but a text that differs
only in whitespace is there. Then the refusal quotes the exact text of the
item (KAIROS-T-0684). Each run of spaces, tabs and line breaks counts as one space.
Kairos does not replace that text: send it as `search`.

```text
VALIDATION: The text of search is not in ACME-T-0012 (version 3). A text that differs only in whitespace is in the content one time. Send this exact text as search: "Line one.\n  Line two.".
```

### Typed sections on an MCP write

A template can name typed sections: a heading and a type (KAIROS-T-0729).
`create_item`, `update_item` and `edit_item` check the new content of a
document against the typed sections of its template (KAIROS-T-0730):

- Each typed section must be in the content, under a heading with the same
  text, at each level.
- Each typed section must have text.
- The text of a `plain` section must pass the plain-language check and the
  vocabulary.
- Each requirement of an `ears` section must fit one frame of EARS. See
  [The EARS check](#the-ears-check).
- The check of a `free` section stops at the first two rules.

The vocabulary is the word list and the terms of the dictionary of the
organization (KAIROS-T-0733). The owner board of the document does not
change the vocabulary: there are no team dictionaries (KAIROS-A-0079).

`create_item` checks each typed section. It checks the right to create first:
a caller with no right gets `FORBIDDEN`. `update_item` and `edit_item` check
only the typed sections that the write changes. They do not check a section
that has the same text before and after the write. They do not check a
section that is missing before and after the write. Thus you can edit one
section of an old document that does not pass a different section.

When the content does not pass, the tool refuses the write with
`SECTIONS_NOT_PASSED`, and it saves nothing. The refusal gives each section
that does not pass. For a `plain` or an `ears` section, it gives each
violation: the rule, the text and the fix. The message lists 40 violations at most. `details`
gives each one.

```text
SECTIONS_NOT_PASSED: The content of ACME-D-0003 does not pass the typed sections of its template. The server saved nothing. Correct each section, and then send the write again.
- Section "Summary" (free): the section has no text. Write text under the heading.
- Section "Goal" (plain), STE-V1: "server". "server" is not in the vocabulary. Use a word of the word list or a term of the dictionary.
details: {"field":"content","sections":[{"heading":"Summary","problem":"empty","type":"free","violations":[]},{"heading":"Goal","problem":"violations","type":"plain","violations":[{"fix":"\"server\" is not in the vocabulary. Use a word of the word list or a term of the dictionary.","rule":"STE-V1","text":"server"}]}]}
```

`problem` is `missing`, `empty` or `violations`. The REST routes and the web
do not do this check: a person can write free text.

### The EARS check

EARS (Easy Approach to Requirements Syntax) puts each requirement in a fixed
frame (KAIROS-T-0732). In an `ears` section, each list item is one
requirement, and each sentence of a paragraph is one requirement. Headings,
code blocks, tables and quotes are not requirements. The words in the frame
are free: the check reads the frame only, not the vocabulary. A word in
inline code is not a keyword.

| Frame | Template |
|---|---|
| Ubiquitous | The <system> shall <response>. |
| Event-driven | When <trigger>, the <system> shall <response>. |
| State-driven | While <state>, the <system> shall <response>. |
| Unwanted behaviour | If <condition>, then the <system> shall <response>. |
| Optional feature | Where <feature>, the <system> shall <response>. |
| Complex | While <state>, when <trigger>, the <system> shall <response>. |

A complex requirement has more than one clause before the system. Each
clause starts with While, When, Where or If, and a comma follows it. You can
write a list of systems before "shall": "The server, the worker and the CLI
shall ...".

A requirement that does not fit a frame gives one violation. The `fix` names
the change and the closest frame.

| Rule | Problem |
|---|---|
| `EARS-1` | The requirement has no "shall". |
| `EARS-2` | The clause before the system does not start with While, When, Where or If, or the clause has no comma after it. |
| `EARS-3` | The requirement has no system before "shall". |
| `EARS-4` | The requirement has no response after "shall". |
| `EARS-5` | An "If" clause has no "then", or a "then" has no "If" clause. |
| `EARS-6` | The requirement has more than one "shall". Write one requirement in each sentence. |

```text
- Section "Requirements" (ears), EARS-1: "A move names CODE when the item is a task in a done column.". The requirement has no "shall". Put the "when" clause first, then a comma, then the system, "shall" and the response. Closest frame: event-driven, "When <trigger>, the <system> shall <response>."
```

### The mark of a document that does not pass

The server keeps the result of the check of each document whose template
has typed sections (KAIROS-T-0731). It checks the full content at each
write of the content, from the web and from the MCP. A periodic pass
(`section-checks`, each minute) checks the old documents. When the typed
sections of a template change, the pass checks the documents of the template
again. When the dictionary of the organization changes, the pass checks each
document again. The mark does not stop a write.

`get_item` shows the mark under the template line, with one line for each
violation (40 at most):

```text
- template: Brief
- sections: does not pass (2 violations)
  - Section "Goal" (plain), STE-V1: "server". "server" is not in the vocabulary. Use a word of the word list or a term of the dictionary.
  - Section "Goal" (plain), STE-G1: "is checked". Passive: is checked. Use the active voice, and name who does the action.
```

`GET /api/documents/{short_code}` gives the same check as `section_check`,
and the item page of the web shows it. When a write makes the document
pass, the mark goes.

### `append_note`

Appends a note to the item, in one call. Send no version and no anchor
text.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The item's short code. |
| `text` | string | yes | — | The markdown text of the note. Must not be empty. Headings are allowed. |

A note is a row of its own, not part of the content. It gets the next id,
the time and the caller as its author. `get_item` shows it after the
content, under a heading with the date (UTC) and the display name of the
author:

```text
## Note n-3 (2026-10-11, Robin)

The text of the note.
```

The id is `n-` and the number of the note on the item: the first note is
`n-1`. Kairos never gives an id again. Cite a note by its id: "note n-3 on
ACME-T-0012". `get_item` lists the notes with their ids, and the item page
of the GUI anchors each one (`#n-3`).

A note writes no version of the content and no history row. An edit of the
content does not change a note, and a note does not refuse an edit. Two
notes at the same time both stay, each with its own id. The result is one
line:

```text
Appended note n-3 to ACME-T-0012. The content stays at version 4.
```

The text can hold any markdown, `## ` headings too: a heading does not end
the note. Each `@user_name` of the text is a mention.

The edit rule applies, as for `update_item`. REST: `POST
/api/{entity_type}/{short_code}/notes`. CLI: `kairos <noun> note <SHORT_CODE>
--text <TEXT>`. The events are `item_updated` and `note_appended`.

Refuses: `VALIDATION` for an empty `text`. `NOT_FOUND` for an unknown short
code or an archived item. `FORBIDDEN` when the
[edit rule](../capabilities/#the-edit-rule) refuses the caller.

### `promote_note`

Turns a note into a draft ADR or document, in one call. The new item has
the text of the note. A line names the note, its author and its date:

```text
From note n-3 on ACME-T-0012 (Robin, 2026-10-11).
```

The note gets the mark of the new item. `get_item` shows the mark as the
last line of the note:

```text
Promoted to ACME-A-0029.
```

Kairos promotes a note one time only. A second promote of the note gets a
`CONFLICT` refusal, which names the item that the note became:

```text
CONFLICT: The note n-3 on ACME-T-0012 became ACME-A-0029 already. Kairos promotes a note one time only. Read ACME-A-0029, or promote a different note.
```

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The short code of the item that has the note. |
| `note_id` | string | yes | — | The id of the note, as `get_item` lists it: `n-3`. |
| `kind` | string | yes | — | `adr` or `document`. |
| `board` | string | yes | — | The board that gets the new item (slug or UUID): an ADR board for an ADR, the owner board for a document. |
| `title` | string | no | the first line of the note | The title of the new item. |

The new item `supports` the item when the item is a strategy, an initiative
or a task. A note on a document or an ADR gets no edge, and the result says
so. The result is one line:

```text
Promoted note n-3 on ACME-T-0012 to the draft adr ACME-A-0029. ACME-A-0029 supports ACME-T-0012. The note has the line "Promoted to ACME-A-0029."
```

You need the edit rule on the item: you created it, or you hold
`manage_<type>` on its board. You also need the right to create on the
board: `manage_adrs` for an ADR, `manage_documents` for a document. The rule
is that of `POST /api/{family}/{short_code}/notes/{note_id}/promote`.

Refuses: `NOT_FOUND` for an unknown short code, an archived item, or an
unknown board. `NOT_FOUND` for an unknown note id; the refusal lists the
ids. `CONFLICT` for a note that has a mark already; `details.promoted_to`
names the item. `VALIDATION` for a kind that is not `adr` or `document`.
`FORBIDDEN` by the edit rule, or without the capability on the board. The
events are `item_updated` and `note_promoted` for the item of the note.

### `set_lifecycle`

Sets the lifecycle of a document: `draft`, `review`, `published` or
`archived` (KAIROS-A-0018). You can move a document from each value to
each other value. A document is not on a board, so `transition_item` does
not move it. `promote_note` makes a draft;
this tool publishes it.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The short code of the document. |
| `lifecycle` | string | yes | — | `draft`, `review`, `published` or `archived`. |

The result is one line:

```text
Set the lifecycle of ACME-D-0007 to published (was review).
```

A value that the document already has changes nothing:

```text
ACME-D-0007 is already published: nothing changed.
```

The edit rule applies: you created the document, or you hold
`manage_documents` on its owner board. The rule is that of
`PATCH /api/documents/{short_code}/lifecycle`.

Refuses: `NOT_FOUND` for an unknown short code or an archived document.
`VALIDATION` for an item that is not a document. `VALIDATION` for an
unknown value; the refusal names the value and lists the four values.
`FORBIDDEN` by the edit rule.

### `set_metadata`

Sets, updates or clears metadata values on an item, and returns the item's
resulting metadata set.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The item's short code. |
| `values` | object of string to string-or-null | yes | — | Keyed by metadata-definition slug. A null value clears that value. |

Every entry is resolved and validated before anything is written, and the
writes are applied in one transaction, so one bad entry rejects the whole call
and changes nothing.

Refuses: `VALIDATION` for an unknown definition slug, and for a value that
fails its definition's rules — enum membership, or a `YYYY-MM-DD` date.
`NOT_FOUND` for an unknown short code or an archived item. `FORBIDDEN` when
the [edit rule](../capabilities/#the-edit-rule) refuses the caller.

### `set_repository`

Sets or clears the repository of a task. The repository is a link: it says
where the code is. The board and the team of the task do not change.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The short code of the task. |
| `repository` | string | no | clears the link | Slug or UUID. Any live repository, of any team. To clear the link, omit the argument, or send null or an empty string. |

The edit rule applies. The caller created the task, or holds `manage_tasks`
on the board of the task, or is an organization admin.

The tool applies to tasks only. It writes no new version of the task. A call
that sets the repository that the task already has is a success.

Refuses: `NOT_FOUND` for an unknown short code or an archived task.
`VALIDATION` for a patch that changes nothing. `CONFLICT` for a stale
`expected_updated_at`.
`VALIDATION` when the item is not a task, and for an unknown `repository`.
`FORBIDDEN` when the edit rule refuses the caller.

### `set_parent`

Sets the parent of an initiative or a task, or removes it (KAIROS-T-0572).
An item has one parent. The new parent replaces the old one in one
transaction, so no reader sees two parents or none.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The short code of the initiative or the task. |
| `parent` | string or null | yes | — | The short code of the new parent: a strategy for an initiative, an initiative for a task. Send null to remove the parent. A retired code finds its item. |

The [link rule](../capabilities/#who-can-write-relationships) applies to each
edge that changes. The caller can edit the item, or the parent at that end.
So the creator of a task can move it to a different initiative.

The same parent again changes nothing, and the answer says so. A change of
parent is not a move: the board rules and `parent_gate` do not apply. The
`get_item` answer then shows the new parent chain.

The tool refuses with these codes:

| Code | When |
|---|---|
| `VALIDATION` | The call has no `parent` argument. The item is a strategy, a document or an ADR: they have no parent. The parent is unknown or archived. |
| `NOT_FOUND` | The item is unknown or archived, or the call names it by a retired code. |
| `RELATIONSHIP_RULE` | The parent has the wrong type. |
| `FORBIDDEN` | The link rule refuses the caller. |
| `PARENT_EXISTS` | A write at the same time gave the item a parent. |

### `set_work_class`

Changes the lane of a task: planned or support. Moving a support request to
the planned backlog is a lane change. The task stays in its column.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The short code of the task. |
| `work_class` | string | yes | — | `planned` or `support`. |

The caller needs `transition_items` on the board of the task. Creating the
task gives no right here. The rule is that of
`POST /api/tasks/{short_code}/work-class`, and the change writes the same
activity line. A lane the task already has changes nothing.

Refuses: `NOT_FOUND` for an unknown short code or an archived task.
`VALIDATION` when the item is not a task, and for an unknown `work_class`.
`FORBIDDEN` without `transition_items`. `AGENT_TRIAGE_OFF` for an agent's
move of intake to planned while the board lets a person decide the triage.

### `set_complexity`

Sets the complexity of an initiative, or clears it. The complexity is not
content: the change does not need a version, and it does not make a new
version.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The short code of the initiative. |
| `complexity` | string | yes | — | `xs`, `s`, `m`, `l` or `xl`. `none` clears the complexity. |

The tool refuses a call without `complexity`, and a null or empty value. A
call that leaves out the argument does not clear the complexity.

The result is one line:

```text
Set the complexity of ACME-I-0004 to l (was m).
```

```text
Cleared the complexity of ACME-I-0004 (was l).
```

A value that the initiative already has changes nothing:

```text
ACME-I-0004 already has the complexity l: nothing changed.
```

The edit rule applies. You can change the complexity when:

- you created the initiative,
- you are a member of its board,
- you hold `manage_initiatives` on its board, or
- you are an organization admin.

The rule is that of `PUT /api/initiatives/{short_code}/complexity`, and the
change writes the same activity line.

Refuses: `NOT_FOUND` for an unknown short code or an archived initiative.
`VALIDATION` for an item that is not an initiative. `VALIDATION` for an
unknown value; the refusal names the value and lists the five values.
`FORBIDDEN` by the edit rule.

### `set_state`

Writes the live state of a task: the branch, the last commit, pushed or
not, the PR, the next step. The state is one current line, separate from
the notes. `get_item`, `board_items` and the card of the task show it, with
who wrote it when. Write it after each commit, each push, a PR, and before
you stop.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The short code of the task. |
| `branch` | string or `null` | no | kept | The branch of the work. `null` clears it. |
| `commit` | string or `null` | no | kept | The last commit (a hash or a short hash). `null` clears it. |
| `pushed` | boolean | no | kept | True when the last commit is pushed. |
| `pr_url` | string or `null` | no | kept | The URL of the PR. `null` clears it. |
| `next_step` | string or `null` | no | kept | What comes next. `null` clears it. |
| `expected_updated_at` | string | no | — | The `updated_at` of the state you read (RFC 3339). A state that changed since refuses the write with `CONFLICT`; the details carry the current state. |

Send each field as its own argument, not one object or one JSON text
(KAIROS-T-0685).

The write is a patch (KAIROS-T-0533). A field that you send gets the value.
`null` clears the field. A field that you do not send keeps its value. Send
at least one field.

The state is not content: the write makes no new version of the task. The
write emits `item_updated`, so the cards and the item page refresh. The state
stays when the task reaches a done column. A hand-off or a release clears
`next_step`: it was the plan of the previous claimant. The branch, the commit
and the PR stay. `board_items` shows the age of the state in its tag:
`[state: branch x; next: y; 2 days ago]`.

The edit rule applies, as for `append_note`. It passes the creator of the
task, the holder of its claim (KAIROS-T-0532), and `manage_tasks` on its
board. The rule is that of
`PUT /api/tasks/{short_code}/state`. The result is one line:

```text
State of ACME-T-0012: branch acme-t-0012 at abc1234, pushed, PR https://github.com/acme/app/pull/12. Next: Wait for CI. (updated just now by Robin)
```

Refuses: `NOT_FOUND` for an unknown short code or an archived task.
`VALIDATION` when the item is not a task, when no field has a value, and
for an unknown argument. `FORBIDDEN` by the edit rule. `CONFLICT` when the
state changed after `expected_updated_at`.

### `set_review`

Sets the review state of a task (KAIROS-T-0718): `not_needed`, `needed` or
`done`. The review state is a reserved field, like the claim and the live
state. It is not a column and not a metadata value. No value means nobody
decided.

A coordinator sets `needed` or `not_needed` when the work has a PR and no
claim. A reviewer sets `done` after the record is on the task. Set it before
anything moves to Completed; Kairos does not gate the move.

| Argument | Type | Required | Description |
|---|---|---|---|
| `short_code` | string | yes | The short code of the task. |
| `review` | string | yes | `not_needed`, `needed` or `done` (no case, no space at the ends). |

The review state is not content: the write makes no new version of the
task. A write of the value the task has changes nothing. A change writes one
activity row (`review: needed`) and emits `item_updated`, then
`task_review_changed`. It fires the dispatch rules of the board with the
trigger `review_changed` (see [`create_dispatch_rule`](#create_dispatch_rule)).
The value stays when the task reaches a done column.

The edit rule applies, as for `set_state`. The rule is that of
`PUT /api/tasks/{short_code}/review`. The result is one line:

```text
Review of ACME-T-0012: needed (set just now by Robin)
```

Refuses: `NOT_FOUND` for an unknown short code or an archived task.
`VALIDATION` for an item that is not a task, for a bad value, or for an
unknown argument. `FORBIDDEN` by the edit rule.

### The teams of an initiative or a strategy

An initiative or a strategy has two sources of teams. These are its tasks,
and the teams that are set on it by hand:

- An initiative gets the team of the board of each live task below it.
- A strategy gets the teams of the live initiatives below it. It reaches the
  tasks two levels down.
- A team set by hand stays when tasks come.
- An archived task, initiative, board or team gives no team.

A team gives no right on the item. Use `set_team` before the item has tasks.

### `set_team`

Sets a team on an initiative or a strategy by hand.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The short code of the initiative or the strategy. |
| `team` | string | yes | — | Slug or UUID of a live team. |

The edit rule applies. The caller created the item, or holds
`manage_initiatives` or `manage_strategies` on its board, or is an
organization admin. The caller needs no right on the team.

The result is one line:

```text
Set the team weir on ACME-I-0003 by hand.
```

Refuses: `NOT_FOUND` for an unknown short code or an archived item.
`RELATIONSHIP_RULE` when the item is not an initiative and not a strategy.
`VALIDATION` for an unknown team. `ALREADY_LINKED` when the team is set
already. `FORBIDDEN` when the edit rule refuses the caller.

### `clear_team`

Clears a team that is set on an initiative or a strategy by hand. A team that
the item gets from its tasks stays. To remove that team, move or archive the
tasks.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The short code of the initiative or the strategy. |
| `team` | string | yes | — | Slug or UUID. The team can be archived. |

The edit rule applies, as for `set_team`.

Refuses: `NOT_FOUND` for an unknown short code or an archived item.
`NOT_FOUND` for a team that is not set on the item by hand. When the item gets the team from its
tasks, the refusal says so. `RELATIONSHIP_RULE` when the item is not an
initiative and not a strategy. `FORBIDDEN` when the edit rule refuses the
caller.

## Moving work

### `transition_item`

Moves an item to another column on its own board.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The item's short code. |
| `to_column` | string | yes | — | Target column, name or UUID, on the item's own board. |
| `claim` | boolean | no | false | Initiatives: makes you the owner in the same move. |

Requires `transition_items` on the item's board. A transition is a move and
not an edit. The creator of the item gets no right to move it.

A move goes one column at a time, along a transition of the board. To find
the columns and the moves between them, call [`my_boards`](#my_boards) first.
`get_item` also gives the next columns of the item.

A person who moves a task to Active gets the [claim](../glossary/#claim) of the
task. The answer then has the line `claim`. An agent can move the task with
the agent key of a person. Then the claim names the person and has the mark
`(agent)`. A service account gets no claim. The claim ends when the task
leaves Active.

Refuses: `NOT_FOUND` for an unknown short code or an archived item;
`ITEM_NOT_ON_BOARD` for an item with no placement — documents always;
`VALIDATION` for a column that is not on that board, listing the board's
columns; `FORBIDDEN` without `transition_items`; `INVALID_TRANSITION` for a
target outside the board's transition graph, enumerating the allowed targets;
`AGENT_TRIAGE_OFF` for an agent's move of intake (a support-lane task in the
entry column) while the board lets a person decide the triage. A task a
person triaged out of the entry column moves as any other. The
`ITEM_NOT_ON_BOARD` refusal for a document names `set_lifecycle`.

The `INVALID_TRANSITION` refusal gives the shortest path to the target, in
the text and in the detail `path`. Call `transition_item` for each step. A
column can have no transition out of it. Then the text names `reopen_item`
for a task in a done column. For a different item, the text says that
`reopen_item` does not move it.

Kairos also refuses `PARENT_NOT_ACTIVE` for a task that goes into a claims
column under an inactive initiative. The board policy `parent_gate` sets this
rule. See [`set_board_policy`](#set_board_policy).

Kairos refuses `OWNER_REQUIRED` for an initiative that goes into Active with
no owner. The board policy `owner_gate` sets this rule.

A move of an initiative does not set its owner. To become the owner in the
same move, send `claim: true`. The rules of [`claim_item`](#claim_item) apply.
Kairos does the claim and the move in one transaction, so a refused move
keeps no owner.

When you are the owner already, Kairos does not claim again. When a different
person is the owner, Kairos refuses with `CONFLICT`. A task gets your claim
from a move to Active with no flag. A strategy or an ADR with `claim: true`
gets `VALIDATION`.

When the initiative of a task has no owner and its board has `owner_gate`,
the `PARENT_NOT_ACTIVE` refusal names both steps. Its details give
`owner_needed: true` and the `fix` `transition_item <initiative> Active claim: true`.

A task can have open blockers: the items that block it and are not done.
Kairos counts a cancelled blocker as open. Such a task can move into a claims
column or a done column. The answer then ends with a warning line:

```text
Warning: ACME-T-0012 is blocked by ACME-T-0009 (open). Your move into Active is recorded.
```

With the board policy `blockers_gate` on, Kairos refuses the move with
`BLOCKED` instead. The details name each blocker.

### `move_item`

Moves a task to another delivery board. It lands in that board's entry column
and follows that board's team.

The tool also moves a document to a different owner board. It cannot remove
the owner board of a document.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The short code of the task or of the document. |
| `to_board` | string | no in the schema | — | Target board, slug or UUID. A task must have it, and the board is a delivery board. For a document, see below. |
| `rename` | boolean | no | `false` | Give the item the next code of the target board. See [A rename](#a-rename). |

Requires `manage_tasks` on both the task's current board and the target. A
move is not an edit. The creator of the task gets no right to move it.

The task keeps its repository. The move does not look at the repository.

The entry column of the target board can hold claims. Then the policies
`parent_gate` and `blockers_gate` of the target board apply, as for
`transition_item`. The warning of open blockers applies too.

A task in a done column does not move: the refusal is `TASK_DONE`. A move
would put a completed or cancelled task live on the other board. Reopen the
task first with [`reopen_item`](#reopen_item). Then move it.

Column-to-column moves on an item's own board are `transition_item`, not this
tool.

#### A rename

With `rename: true`, the item also gets the next code of the target board, in
the transaction of the move. Without it, the item keeps its code.

- Kairos retires the old code and does not issue it again. `get_item` with
  the old code finds the item.
- Each reference to the old code in the title and the content of each item
  changes to the new code, one time. Each item that changes gets a new
  version. A code in a URL, a path or a file name does not change. The
  footer of an item from the Metis importer does not change.
- The activity log gets an entry with the action `rename`
  (`code:{old}->{new}`). `get_history` lists it under **Renames**, with the
  time and who did it.

The result has a second line:

```text
Moved COLLIERY-T-0100: colliery-io-delivery -> skadi / Backlog.
Renamed COLLIERY-T-0100 -> SKADI-T-0001. The code COLLIERY-T-0100 is retired: a read with it finds the item. The references changed in 1 item(s): COLLIERY-T-0101.
```

A rename of a document needs `to_board`, and the board must be a different
board. Kairos refuses a rename to a board whose prefix the code has already
(`RENAME_NOT_NEEDED`). Then the item does not move.

#### A document

`to_board` is the new owner board of the document. It can be a live board of
each level. The document gets no column. Its edges and its `impacts` links do
not change.

Each document has an owner board, so the tool cannot remove it. A call with
no `to_board`, or with null or an empty string, gets `VALIDATION`, and
`details.argument` is `to_board`.

The move of a document is not an edit. The caller needs `manage_documents` on
two boards:

- the board that owns the document now
- the board that owns the document after the move

An organization admin can move each document. The creator of the document gets
no right to move it.

The tool writes no new version of the document. The activity log gets one
entry with the action `update`.

The result is one line:

```text
Moved ACME-D-0004: owner board platform-delivery -> web-delivery.
```

A call can name the board that the document has. That call is a success, and
it writes nothing.

```text
No change to ACME-D-0004: its owner board is web-delivery already.
```

#### Refusals

The tool refuses with these codes:

| Code | When |
|---|---|
| `NOT_FOUND` | The short code is unknown, or the item is archived. The `to_board` is unknown or archived. |
| `VALIDATION` | The item is not a task and not a document. A task or a document has no `to_board`. |
| `ITEM_NOT_ON_BOARD` | The task has no placement. |
| `FORBIDDEN` | The caller does not hold `manage_tasks` on the two boards of a task. The caller does not hold `manage_documents` on the two boards of a document, and the message names the board. |
| `SAME_BOARD`, `NOT_DELIVERY_BOARD`, `NO_ENTRY_COLUMN` | For a task only. |
| `RENAME_NOT_NEEDED` | A rename to a board whose prefix the code has already, or a rename of a document whose owner board does not change. |

### `claim_item`

Gives you the [claim](../glossary/#claim) of a task in Active that has no
claim (KAIROS-T-0528). Or makes you the [owner](../glossary/#owner) of a
strategy or an initiative that has no owner.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The short code of a task in Active with no claim, or of a strategy or an initiative with no owner. |

A task: you need `transition_items` on the board of the task (KAIROS-T-0642).
The creator of the task gets no claim from the edit rule. The claim names you. With your agent key, it names you and
marks the agent. The answer has the line `claim`.

A task that a different person holds gets `CLAIMED` and the name of the
holder: ask for a `hand_off_item`. A move of a task to Active
gives you the claim too.

A strategy or an initiative: you must be a person and a member of the board
of the item. A member has a grant on the board. An organization admin can also
claim. The owner stays in each column, Completed too. The owner gets no right
from the claim. The answer has the line `owner` with the new owner.

Refuses:

- `NOT_FOUND` for an unknown short code or an archived item.
- `VALIDATION` for an item that is not a task, a strategy or an initiative.
- `VALIDATION` for a task, when the caller is a service account.
- `FORBIDDEN` for a task, without `transition_items` on its board.
- `FORBIDDEN` for a strategy or an initiative, for a service account, and for
  a person who is not a member of the board and not an organization admin.
- `NOT_CLAIMABLE` for a task that is not in Active.
- `CLAIMED` for a task that a different person holds. The text names the
  holder.
- `CONFLICT` when you have the claim, or when the item has an owner. The text
  names the owner.

### `hand_off_item`

Changes the [claim](../glossary/#claim) of a task in Active, or the
[owner](../glossary/#owner) of a strategy or an initiative.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The short code of a task in Active, or of a strategy or an initiative. |
| `to` | string | yes | — | The person who gets the claim or who becomes the owner: a user id, an email or a user name of a person of the organization, or `me`. [`list_members`](#list_members) gives them. A new owner must be a member of the board of the item. A new claim holder must hold `transition_items` on the board of the task. |

A task: the person who has the claim can hand it off. Any other principal needs
`transition_items` on the board of the task. A task in Active with no claim can
get a claim by a hand-off. The task stays in Active. The answer has the line
`claim` with the new claim.

The person who gets the claim must hold `transition_items` on the board of the
task. The team of the board gives it, and so do a grant and the organization
admin role. Else the tool refuses with `VALIDATION` on `to`. The text names the
board and the capability. A person who cannot move the task cannot finish it.
A hand-off to `me` passes only when you hold the capability.

A strategy or an initiative: the owner can hand it off. An organization admin
can set the owner of each item, also of an item with no owner. Nobody else can.

The new owner must be a member of the board of the item. A member has a grant
on the board. The new owner gets a notification. The answer has the line `owner` with the
new owner.

A service account cannot have a claim or be an owner.

Refuses:

- `NOT_FOUND` for an unknown short code or an archived item.
- `VALIDATION` for an item that is not a task, a strategy or an initiative.
- `VALIDATION` for `to` that is not a person of the organization.
- `FORBIDDEN` for a task, without the claim and without `transition_items`.
- `FORBIDDEN` for a strategy or an initiative, for a principal that is not
  the owner and not an organization admin.
- `NOT_BOARD_MEMBER` for a strategy or an initiative, when `to` is not a
  member of the board of the item.
- `NOT_CLAIMABLE` for a task that is not in Active.
- `CONFLICT` when the person has the claim or is the owner already.

### `release_item`

Ends the [claim](../glossary/#claim) of a task in Active, or removes the
[owner](../glossary/#owner) of a strategy or an initiative.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The short code of a task in Active, or of a strategy or an initiative. |

A task: the task stays in Active with no claim, free for anyone. A
`claim_item` takes it. A move of the task to Active gives it too. The person
who has the claim can release it. Any other principal needs `transition_items` on the
board of the task.

A strategy or an initiative: the owner can release it. An organization admin
can remove each owner. Nobody else can. The item then has no owner, and a
member of its board can claim it.

Refuses:

- `NOT_FOUND` for an unknown short code or an archived item.
- `VALIDATION` for an item that is not a task, a strategy or an initiative.
- `FORBIDDEN` for a task, without the claim and without `transition_items`.
- `FORBIDDEN` for a strategy or an initiative, for a principal that is not
  the owner and not an organization admin.
- `NOT_CLAIMABLE` for a task that is not in Active.
- `NO_CLAIM` for a task that has no claim.
- `NO_OWNER` for a strategy or an initiative that has no owner.
- `OWNER_REQUIRED` for an initiative in Active or Monitoring, on a board with
  the policy `owner_gate` on. Hand it off with `hand_off_item`.

### `cancel_item`

[Cancels](../glossary/#cancel) a task or an initiative: the team will not do
it. The item moves to the done column of its board, and it gets the mark
"cancelled" with the reason.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The short code of the task or the initiative. It must not be in a done column. |
| `reason` | string | yes | — | Why the item is cancelled. Must not be empty. |

The transition rules of the board do not apply. You can cancel a task from
each column that is not done. The done column is the first live column with
the done flag, by position. A cancel ends the [claim](../glossary/#claim) of
the task. The activity log gets a row `cancel` with the reason.

`get_item`, `board_items` (the mark `[cancelled]`) and the board of the GUI
show the mark. [`reopen_item`](#reopen_item) brings the task back and removes
the mark. A transition out of the done column also removes it. The activity
log keeps the reason.

An initiative has no claim, and its owner stays. An initiative has no reopen:
it stays in the done column. A transition out of the done column, on a board
where an admin added one, removes the mark.

To retire a different type:

- A strategy or an ADR: move it with `transition_item`, and write the reason
  with `append_note`.
- A document: call `set_lifecycle` with `archived`.

Needs `transition_items` on the board of the item: the capability of a move.
The creator of the item gets no right here.

Refuses:

- `NOT_FOUND` for an unknown short code or an archived task.
- `VALIDATION` for an empty reason, and for an item that is not a task or an
  initiative. The text names the route for that type.
- `FORBIDDEN` without `transition_items`.
- `TASK_DONE` for a task in a done column, and `INITIATIVE_DONE` for an
  initiative in a done column.
- `NO_DONE_COLUMN` for a board with no done column.
- `AGENT_TRIAGE_OFF` for an agent's cancel of intake (a support-lane task in
  the entry column) while the board lets a person decide the triage.

### `reopen_item`

Reopens a done task: a completed task, or a
[cancelled](../glossary/#cancel) one. The task moves to the first column of its
board that is not done.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The task's short code. The task must be in a done column. |

The column is the first live column without the done flag, by position. On a
default delivery board, that column is Backlog. The transition rules of the
board do not apply: a default board has no transition out of Completed. The
policies `parent_gate` and `blockers_gate` apply when the column holds claims,
as for `transition_item`. The warning of open blockers applies too.

The rest is the same as a transition. The activity log gets a row
`transition`. The event is `item_transitioned`. The reopen removes the cancel
mark, and the activity log keeps the reason. A completed blocker blocks again,
and its dependants get `blocked_again`. A cancelled blocker never stopped
blocking, so it sends no `blocked_again`.

The result names the move:

```text
Reopened ACME-T-0012: Completed -> Backlog. The cancel mark is removed; the history keeps the reason.
```

Needs `transition_items` on the board of the task: the capability of a move.
The creator of the task gets no right here.

Refuses:

- `NOT_FOUND` for an unknown short code or an archived task.
- `VALIDATION` for an item that is not a task.
- `FORBIDDEN` without `transition_items`.
- `TASK_NOT_DONE` for a task that is not in a done column.
- `NO_OPEN_COLUMN` for a board with only done columns.
- `PARENT_NOT_ACTIVE` when the column holds claims, the policy `parent_gate`
  is on, and the initiative of the task is not active.
- `BLOCKED` when the column holds claims, the policy `blockers_gate` is on,
  and the task has open blockers.

## Relationships

### `link_items`

Creates a relationship edge between two items.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `source` | string | yes | — | Source item's short code. The edge runs source to target. |
| `target` | string | yes | — | Target item's short code. For `impacts`: the slug or the UUID of a repository. For `external`: a URI. |
| `relationship` | string | yes | — | `parent`, `supports`, `informs`, `supersedes`, `amends`, `blocks`, `impacts`, `external`. |
| `label` | string | no | — | For `external` only: the label of the reference, at most 200 characters. |

The types of the two items must agree with this table (KAIROS-T-0677). The
description of the tool has the same table, and a test compares it with the
rules of the server.

`amends`: the source ADR changes a part of the target ADR, and the rest of the
target stays in force. `supersedes`: the source ADR replaces all of the target
(KAIROS-A-0069). A `parent`, a `blocks` or an `amends` edge must not make a
cycle.

| relationship | source | target |
|---|---|---|
| `parent` | strategy | initiative |
| `parent` | initiative | task |
| `supports` | strategy, initiative, task | document, adr |
| `informs` | document, adr | strategy, initiative, task |
| `informs` | task | task |
| `supersedes` | adr | adr |
| `amends` | adr | adr |
| `blocks` | strategy, initiative, task | strategy, initiative, task |
| `impacts` | document, adr | repository (slug or UUID) |
| `external` | any item | URI (`http`, `https` or `mailto`) |

When the edge is possible in the other direction, the `RELATIONSHIP_RULE`
refusal ends with `swap source and target`:

```text
RELATIONSHIP_RULE: A supports edge cannot go from document to initiative. A supports edge goes from a strategy, an initiative or a task to a document or an adr. The edge is possible in the other direction: swap source and target.
```

A `relationship` that is not in the vocabulary is `VALIDATION`. For a value
that agents often send, the refusal names the nearest relationship:

```text
VALIDATION: The value "relates_to" is not a value of relationship. The values are: parent, supports, informs, supersedes, amends, blocks, impacts, external. For a reference to a document or an adr, or from a task to a task, use informs. For work that must wait for other work, use blocks.
```

The [link rule](../capabilities/#who-can-write-relationships) applies. The
caller can edit the source or the target. One end is sufficient. No type needs
the admin role.

The create of a `blocks` edge needs the target, the blocked item
(KAIROS-T-0569). With the source only, the call makes a proposal and no edge.
The answer names the proposal and says who can confirm it. See
[A `blocks` edge needs the blocked item](../capabilities/#a-blocks-edge-needs-the-blocked-item).

```text
Proposed ACME-T-0012 -[blocks]-> ACME-T-0031 (proposal 4f1c2a8e-0000-4000-8000-000000000001). It is NOT an edge yet. A blocks edge needs the right to edit the blocked item ACME-T-0031. You can edit only ACME-T-0012, so the call made a proposal. A holder of "approve_proposals" on the board web-delivery or on the board platform-delivery can confirm or reject it. An organization admin can too.
```

The caller can edit an item that the caller created. So a request that the
caller sent to a different team can block an item of the caller. The caller
can also edit an item with `manage_<type>` on its authorization board.

A `supports` edge to a document gives no right on the document. See
[The `supports` edge of a document](../capabilities/#the-supports-edge-of-a-document).

When you split work into tasks, add a `blocks` edge from each blocker to each
task that waits. Give no edge to tasks that can go in parallel. The
graph view shows serial work as a row and parallel work in one column.

A `blocks` edge counts only while the items at both ends can move. Complete
work does not block, and nothing blocks complete work. The edge stops counting
when the item at either end is in a done column. The edge stays, and `get_item`
marks the done end `[done]`. A cancelled blocker still blocks, and `get_item`
marks it `[cancelled]` (KAIROS-T-0567). `link_items` does not refuse an edge to
an item in a done column.

The tool refuses with these codes:

| Code | When |
|---|---|
| `VALIDATION` | The `relationship` is not in the vocabulary. The `source` or the `target` does not name a live item. The `source` and the `target` are the same item. |
| `FORBIDDEN` | The caller can edit neither end. The message names the capability for each end. For `blocks`, it names the target first. |
| `CONFLICT` | For `blocks` with the source only: the same proposal is pending, or the item has 10 pending proposals. |
| `RELATIONSHIP_RULE` | That relationship is not possible between those two item types. |
| `CYCLE_DETECTED` | The edge makes a cycle. |
| `ALREADY_LINKED` | The edge is there. |
| `PARENT_EXISTS` | A `parent` edge to an item that has a different parent. An item has one parent (KAIROS-T-0572). `details` names the item and its parent. Use [`set_parent`](#set_parent) to change the parent. |

#### The relationship `external`

`external` links an item of each type (`source`) to a URI (`target`) with a
label. The URI names an RFC in a wiki, a design page or an issue that Kairos
does not hold. See [external reference](../glossary/#external-reference). The caller must be
able to edit the source. The scheme of the URI is `http`, `https` or `mailto`.
The tool refuses each other scheme with `VALIDATION`, and the refusal names the
scheme:

```text
Kairos does not permit the scheme "javascript". The schemes are: http, https, mailto.
```

A URI that the item has is `ALREADY_LINKED`. A `label` with a different
relationship is `VALIDATION`, and the refusal names `label`. The result is one
line:

```text
Linked ACME-T-0012 -[external]-> RFC: zen as the policy engine <https://example.com/wiki/rfc-42>.
```

#### The relationship `impacts`

An `impacts` link says which repository a document or an ADR is about. The
`source` is the short code of the document or of the ADR. The `target` is a
live repository of the organization, of each team.

The [edit rule](../capabilities/#the-edit-rule) of the source applies. The
caller needs no right on the repository. The link gives no right on the
source, and no right on the repository. See
[Who can write an `impacts` link](../capabilities/#who-can-write-an-impacts-link).

A task does not impact a repository. `set_repository` links a task to a
repository.

The result is one line:

```text
Linked ACME-D-0004 -[impacts]-> repository fidius.
```

Refuses for `impacts`: `VALIDATION` for a `source` that does not name a live
item. `RELATIONSHIP_RULE` for a `source` that is a strategy, an initiative or
a task. `FORBIDDEN` when the caller cannot edit the source. `VALIDATION` for a
`target` that is not a live repository. `ALREADY_LINKED` when the link is
there.

The refusal for a task says how to link a task:

```text
RELATIONSHIP_RULE: ACME-T-0012 is a task. Only a document or an ADR can impact a repository. A task links to a repository. To link ACME-T-0012 to a repository, use the tool set_repository.
```

### `unlink_items`

Removes a relationship edge. The arguments and the link rule are those of
`link_items`.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `source` | string | yes | — | Source item's short code. |
| `target` | string | yes | — | Target item's short code. For `impacts`: the slug or the UUID of a repository. For `external`: the URI of the reference. |
| `relationship` | string | yes | — | `parent`, `supports`, `informs`, `supersedes`, `amends`, `blocks`, `impacts`, `external`. |

To remove a `supports` edge of a document, the caller must be able to edit
the document. The right to edit the source is not sufficient.

Each `supports` edge of a document can go, the last one too. The document
keeps its owner board. See
[A document always has an owner](../capabilities/#a-document-always-has-an-owner).

For `impacts`, the rule is that of `link_items`: the caller can edit the
source. The repository can be archived. The result is one line:

```text
Unlinked ACME-D-0004 -[impacts]-> repository fidius.
```

For `external`, the caller can edit the source, and `target` is the URI of the
reference. The result is one line:

```text
Unlinked ACME-T-0012 -[external]-> RFC: zen as the policy engine <https://example.com/wiki/rfc-42>.
```

Refuses: as `link_items`, except that a `relationship` with no such edge
between those items is `NOT_FOUND`. An `impacts` link that is not there is
`NOT_FOUND` too, and an external reference that is not there. The tool gives `FORBIDDEN` for a
`supports` edge of a document that the caller cannot edit.

## Finding related work

### `related_work`

Work that may be related to an item: possible duplicates, prior art in finished
or put-away work, and dependencies nobody drew.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The item to find related work for. |
| `limit` | integer | no | 5 | Proposals to return. Clamped to 1–10. |

Returns a short markdown list. Each line carries a **claim**, and one of three:

| claim | means |
|---|---|
| `possible dependency` | similar, and no edge joins them and they share no parent |
| `possible duplicate` | similar, and they already hang off the same parent |
| `prior art` | similar, and the other item is finished or put away |

Under each, a sentence saying what matched — text, meaning, or both — the literal
heading of the section it matched in, and what the graph did or did not know.

The response begins by saying which sources answered. `Ranked across text and
meaning` is a full answer; `Text only` is a **degraded** one — real, but it will
have missed work phrased differently. See
[Configure semantic retrieval](../../how-to/configure-retrieval/).

A text-only answer has these rules:

- It gives 3 proposals at most, also with a higher `limit`.
- Each word of the title has a weight. A rare word in the tenant weighs more
  than a common word.
- An item gets the full weight of a word in its title. It gets half the weight
  of a word that is only in its content.
- The text score of an item is its share of the total weight, from 0 to 1.
- The tool does not propose an item with a text score below 0.4. So a match
  on common words alone gives no proposal.
- The same repository and the same board move an item up the list. They do
  not lift an item over the bar.
- The `Text only` warning is at the top of the response, one time. It is not
  in the lines of the proposals.
- When no item is over the bar, the response starts with `None found`.

Results are **bounded** and the wording is deliberate: every line says *possible*
or *prior art*, never *blocks* or *duplicates*. At the measured precision about
half of the strongest matches are genuinely related, so these are suggestions to
check rather than facts to act on — the reasoning is
[why](../../explanation/finding-related-work/).

Items already joined by an edge are not returned: there is nothing to propose and
nothing you cannot already see.

Refuses: `NOT_FOUND` for an unknown short code. A fault of the server is
`INTERNAL`, not `NOT_FOUND`. `GET /api/items/{short_code}/related` has the
same codes.

The response ends with the section `External references` when the item or a
proposal has one. Each line has the short code, the label and the URI of one
reference. The search does not read those documents. The section is there when
the search found no related work too.

```text
## External references

- ACME-T-0012: RFC: zen as the policy engine <https://example.com/wiki/rfc-42>
- ACME-T-0009: Design page <https://example.com/design>
```

Returns a plain note, not an error, when the deployment has embeddings disabled.

### `propose_edge`

Proposes a `parent` or `blocks` edge for a holder of `approve_proposals` to
confirm. It does not create the edge.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `source` | string | yes | — | Source item's short code. For `parent`, this is the parent. |
| `target` | string | yes | — | Target item's short code. |
| `relationship` | string | yes | — | `parent` or `blocks`. Nothing else may be proposed. |
| `why` | string | yes | — | Your reasoning, in your own words. Kept verbatim and shown to whoever decides. |

There is deliberately **no confirm or reject tool**. A decision is a separate
step, in the web interface or over REST. A wrong `parent` edge moves work to a
board that reports to people who believe it. Nobody reads an edge again once it
exists.

The proposal appears on both items in the interface, with your reasoning, for
someone to accept or decline. [`get_item`](#get_item) lists the proposals of an
item in the section `## Proposals`, with the state of each one (KAIROS-T-0558).
After the decision, Kairos sends you a `proposal_decided`
[notification](#your-notifications). It names the edge, the state and the
decider. You get no notification for a decision that you made yourself. Confirming creates the real edge, and is refused by
the same cycle and rule checks that refuse any other edge.

To propose, a caller needs no capability: a proposal writes no edge and changes
no item.

A holder of `approve_proposals` on the board of the source or of the target
confirms or rejects. An organization admin can also decide. The agent key of a
holder, a service account that holds the grant, and the proposer can decide.
The answer of `propose_edge` names the two boards. A caller with no grant gets
403 `FORBIDDEN`, and the proposal stays pending. See
[The decision of an edge proposal](../capabilities/#the-decision-of-an-edge-proposal).

Refuses: `VALIDATION` for a relationship outside `parent`/`blocks`;
`NOT_FOUND` for a short code that names no live item; `CONFLICT` when an
identical proposal is already waiting, or when the item already holds ten
undecided ones — decide some rather than adding more.

Only `parent` and `blocks` are proposable. `supports`, `informs`, `supersedes`
and `amends` are editorial, cheap to undo, and remain a person's to draw with
[`link_items`](#link_items).

## Archiving and deleting

Three tools end an item in different ways:

- `delete_item` archives an item. The name says delete, but the item is not
  deleted: `restore_item` brings it back.
- `cancel_item` (above) moves a task to done with a reason.
- `purge_task` deletes a task for good. Nothing can bring it back.

`reopen_item` (above) brings a done task back to work.

`archive_completed` archives each task in the done columns of a board.

### `delete_item`

Archives an item. The response lists each item that the cascade archived, and
names each descendant that the archive did not reach. `restore_item` brings
each one back.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The item's short code. |
| `confirm` | boolean | no | `false` | `true` archives the item. |
| `preview` | boolean | no | `false` | `true` shows the cascade and changes nothing. Not with `confirm: true`. |

Send `preview: true` before the archive (KAIROS-T-0558). The preview uses the
plan of the archive, for you as the caller. It lists the descendants that the
archive would take. It names each descendant that would stay, and the reason.

The preview changes no item. When the edit rule refuses you the named item,
the preview says so. REST `GET /api/{entity_type}/{short_code}/cascade-preview`
gives the same cascade.

```text
Preview of an archive of ACME-I-0001. Nothing is archived.
The cascade would archive 2 descendants: ACME-T-0001, ACME-T-0002.
The archive would not reach 1 item. They would stay live and keep their parent.
- WEB-T-0003: You need `manage_tasks` on the board <board-id>.
To archive, call delete_item with short_code ACME-I-0001 and confirm=true.
```

The archive cascades through `parent` edges to each descendant that the caller
can edit. Archived items remain retrievable by short code and searchable with
`include_deleted`; they are hidden from default listings. The tool keeps the
name `delete_item` for the agents that use it. See the
[Glossary](../glossary/#archived).

The [edit rule](../capabilities/#the-edit-rule) applies to the named item, and
then to each descendant. The rule is the same as for REST `DELETE`.

- The tool archives a descendant that the caller can edit.
- The tool stops at a descendant that the caller cannot edit. That descendant
  stays live, and each item below it stays live.
- A descendant that stays keeps its `parent` edge.

When some descendants stay, the output has these lines after the line of the
descendants:

```text
The archive did not reach 2 items. They stay live and keep their parent.
- ACME-I-0002: You need `manage_initiatives` on the board <board-id>.
- ACME-T-0009: It is below ACME-I-0002.
```

The output has no such lines when the archive reached each descendant.

Refuses: `VALIDATION` when `confirm` and `preview` are both `false` or both
`true` — checked before the short code is looked up, so such a call never
reports an unknown item; `NOT_FOUND` for an
unknown short code or an already-archived item; `FORBIDDEN` when the
[edit rule](../capabilities/#the-edit-rule) refuses the caller;
`AGENT_TRIAGE_OFF` for an agent's archive of intake while the board lets a
person decide (KAIROS-T-0537). A call with no `confirm` and no `preview`
gets `VALIDATION`. The text names `confirm=true` and `preview=true`.

### `restore_item`

Puts an archived item back on its board.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The archived item's short code. |

Restores only the named item. A cascade archive was an act on a subtree, so
archived descendants stay archived; the response names them.

The edit rule applies: a caller who can archive an item can restore it. The
tool changes the named item only, so the rule applies to the named item only.
A live descendant that an archive did not reach stays as it is.

Refuses: `NOT_FOUND` for an unknown short code; `VALIDATION` when the item is
not archived; `FORBIDDEN` when the edit rule refuses the caller;
`RESTORE_BLOCKED` when the item's board, column, owning team or
repository has since been removed, naming what is missing. For a document,
the board is its owner board. The `impacts` links of an item block no
restore.

### `archive_completed`

Archives each task in the done columns of one delivery board: "Archive
completed". A [cancelled](../glossary/#cancel) task is in a done column, so it
goes too.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `board` | string | yes | — | The delivery board, by slug or UUID. |
| `confirm` | boolean | yes | — | Must be `true`. |
| `older_than_days` | integer | no | each done task | Archive only the done tasks that entered their done column at least this many days ago. A whole number from 1 to 36500. |

The time that a task entered its column is the time of its last move. A task
that never moved has the time of its creation. With `older_than_days` and no
task that old, the output says so, and Kairos archives nothing.

Each task goes through the archive of `delete_item`. The cascade, the end of
the claim and the activity row are the same as for one task. `restore_item`
brings back each task. The call is one transaction: it archives each task, or
it archives none. The output gives the count and the codes:

```text
Archived 2 completed tasks of web-delivery: WEB-T-0004, WEB-T-0007.
To get one back, restore it by its short code.
```

Needs `manage_tasks` on the board: the capability that archives each task of
the board.

Refuses: `VALIDATION` when `confirm` is `false`, or when `older_than_days` is
not from 1 to 36500; `NOT_FOUND` for an unknown board; `FORBIDDEN` without
`manage_tasks`; `NOT_DELIVERY_BOARD` for a board that is not a delivery board.

### `purge_task`

Deletes an archived task for good. **You cannot undo this.** No restore brings
the task back. Archive the task first with `delete_item`, which a restore can
undo.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The task's current short code. A retired code is not followed. |
| `confirm` | boolean | yes | — | Must be `true`. |

The task must be archived (KAIROS-T-0570). The purge removes these rows of the
task:

- the task, its content history and its metadata values;
- its edges and its edge proposals;
- its forge links, its external references and its embeddings;
- its retired codes and its forge intake row. Thus a new import of the issue
  makes a task again;
- its claim and its cancel mark.

The items at the other end of the edges stay. The rows of the activity log
about the task stay. The purge adds one row `purge` with the code and the
title: `short_code:ACME-T-0012 title:Old task`.

Needs the capability `purge` on the board of the task. Team membership does not
give it, and `manage_tasks` is not enough. Organization admins have it. The
creator of the task gets no right here.

Refuses:

- `VALIDATION` when `confirm` is `false`.
- `NOT_FOUND` for an unknown short code.
- `FORBIDDEN` without `purge`.
- `TASK_LIVE` for a live task.
- `AGENT_TRIAGE_OFF` for an agent's purge of intake while the board lets a
  person decide (KAIROS-T-0537).

## The structure of the organization

These tools change the teams, the boards and the grants of the
organization. An organization admin can use each tool. A principal with the
capability [`manage_structure`](../capabilities/#manage_structure) can use them
too, at the scope of the grant:

| Grant of `manage_structure` | What the tools can do |
|---|---|
| On the organization | Each tool, on each team and each board. |
| On one board | `grant_capability` on that board only. |
| None | Nothing. Each tool refuses with `FORBIDDEN`. |

Only an organization admin grants `manage_structure`. A holder of it cannot
grant it. See [Run an admin agent](../../how-to/run-an-admin-agent/).

Each tool calls the function of its REST route. The rule, the refusals and
the activity rows are those of the route. The activity log names the
principal of the call.

### `create_team`

Creates a team and its delivery board, as `POST /api/teams` does. With
`adr_board` `true`, it also creates the ADR board of the team, in the same
transaction. That board is the board of `POST /api/boards` with `board_level`
`adr` and the team.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `name` | string | yes | — | The name of the team. |
| `slug` | string | yes | — | The slug of the team. It must match `^[a-z][a-z0-9_-]{1,62}$`. The delivery board gets the slug `{slug}-delivery`. The ADR board gets `{slug}-adrs`. |
| `code_prefix` | string | yes | — | The short-code prefix of the boards of the team, for example `OPS`. It must match `^[A-Z][A-Z0-9]{1,9}$`. It does not change later. |
| `team_type` | string | no | `stream_aligned` | `stream_aligned`, `platform`, `enabling` or `complicated_subsystem`. |
| `adr_board` | boolean | no | `false` | `true` also makes the ADR board of the team. |

The new board has no members. Use `add_team_member` and `grant_capability`.

The result is two lines:

```text
Created team home-ops (Home operations, stream_aligned). Delivery board: home-ops-delivery (prefix OPS).
The board has no members. Use add_team_member and grant_capability.
```

With `adr_board` `true`, the first line ends with `ADR board: home-ops-adrs.`

Requires the organization admin role or `manage_structure` on the
organization.

Refuses: `VALIDATION` for a slug or a prefix that does not have the form, and
for an unknown `team_type`. `CONFLICT` when a live team has the slug. `CONFLICT`
when a live board has a slug of the new boards. `CONFLICT` when a live delivery
board has the prefix. `FORBIDDEN` without the capability:

```text
FORBIDDEN: This action requires the capability "manage_structure" on the organization, or the organization admin role. A grant of "manage_structure" on one board does not give it. Ask an organization admin to do this, or to grant you "manage_structure" on the organization.
details: {"board_id":null,"required_capability":"manage_structure","scope":"organization"}
```

When Kairos refuses one of the boards, it writes nothing.

### `create_board`

Creates one board, as `POST /api/boards` does. The board gets the default
columns and transitions of its level.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `name` | string | yes | — | The name of the board. |
| `slug` | string | yes | — | The slug of the board. It must match `^[a-z][a-z0-9_-]{1,62}$`. |
| `board_level` | string | yes | — | `strategy`, `initiative`, `delivery` or `adr`. |
| `code_prefix` | string | yes | — | The short-code prefix of the board. It must match `^[A-Z][A-Z0-9]{1,9}$`. The ADR board of a team takes the prefix of the team. |
| `team` | string | no | none | The owner team. Slug or UUID. A delivery board must have a team, and the team must have no delivery board. A strategy or an initiative board with no team gets its own empty team, `<level>-<slug>`. An ADR board with a team is the ADR board of that team. |

Requires the organization admin role or `manage_structure` on the
organization. A grant on one board does not create a board.

Refuses: the refusals of `POST /api/boards` (`VALIDATION`, `CONFLICT`,
`TEAM_HAS_DELIVERY_BOARD`, `TEAM_HAS_ADR_BOARD`), and `FORBIDDEN` as for
`create_team`.

### `add_team_member`

Adds a member of the organization to a team, as
`POST /api/teams/{id}/members` does. A team member gets `manage_tasks`,
`manage_documents` and `transition_items` on the boards of the team with no
grant. On the ADR board of the team, the member also gets `manage_adrs`.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `team` | string | yes | — | The team. Slug or UUID. |
| `user` | string | yes | — | `me`, a user id, an email, a user name, or the name of a service account. |

Requires the organization admin role or `manage_structure` on the
organization.

Refuses: `VALIDATION` for an unknown team, and for a `user` that is not a
member of the organization. `CONFLICT` when the user is a member of the team
already. `FORBIDDEN` as for `create_team`.

A person is not a member of the organization before the first sign-in or a
provisioning step. The refusal of an unknown `user` says how a person becomes
a member on this deployment (KAIROS-T-0679). It names only the routes that the
deployment has: the identity provider, SCIM, and local accounts. For an email,
it also says if an account has that email. `grant_capability` gives the same
refusal. A deployment with an identity provider and no local accounts gives:

```text
VALIDATION: No member of the organization has the id, the email, the user name or the service-account name "sam@example.com". No account has the email "sam@example.com" yet. A person becomes a member of the organization in one of these ways: (1) the person signs in to Kairos one time through the identity provider, and then an organization admin adds the person to the organization (`kairos members add --email <email>`, or POST /api/members); (2) the identity provider provisions the person through SCIM (/scim/v2). Then send the reference again.
```

### `grant_capability`

Grants one capability to a member of the organization.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `user` | string | yes | — | `me`, a user id, an email, a user name, or the name of a service account. |
| `capability` | string | yes | — | A board capability or a glob of [the vocabulary](../capabilities/#the-grantable-vocabulary), or `manage_structure`. |
| `board` | string | no | none | The board. Slug or UUID. A board capability needs it. For `manage_structure`, no `board` is a grant on the organization. |

A board capability is a grant of `POST /api/boards/{id}/members`. It requires
`administer_members` on the board, or `manage_structure` on the board or on
the organization.

`manage_structure` is a grant of `POST /api/structure-grants`. Only an
organization admin can grant it.

Refuses: `VALIDATION` for a capability outside the vocabulary, and for a board
capability with no `board`. `VALIDATION` for a `user` that is not a member of
the organization. `NOT_FOUND` for an unknown board. `CONFLICT` when the member has
the grant already. `FORBIDDEN` without the capability on the board:

```text
FORBIDDEN: This action requires the capability "administer_members" on the board 0193a1c2-0000-7000-8000-000000000003, or the capability "manage_structure" on that board or on the organization. Ask an organization admin for one of them.
details: {"board_id":"0193a1c2-0000-7000-8000-000000000003","or_capability":"manage_structure","required_capability":"administer_members"}
```

`FORBIDDEN` for `manage_structure` from a principal that is not an
organization admin.

### Team and board administration

These tools do what the REST routes of teams and boards do (KAIROS-T-0443).
Each tool calls the function of its route, so the gate is the same. A `user`
is `me`, a user id, an email, a user name, or the name of a service account. A
`board` or a `team` is a slug or a UUID. A `column` is a name or a UUID.

### `update_team`

Changes a team, as `PATCH /api/teams/{id}` does.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `team` | string | yes | — | Slug or UUID. |
| `name` | string | no | no change | The new name. |
| `slug` | string | no | no change | The new slug. |
| `team_type` | string | no | no change | `stream_aligned`, `platform`, `enabling` or `complicated_subsystem`. |
| `site_enabled` | boolean | no | no change | The published site of the team: on or off. |

Requires the organization admin role or `manage_structure` on the
organization. A value that the team has already changes nothing.

### `remove_team_member`

Takes a member off a team, as `DELETE /api/teams/{id}/members/{user_id}` does.
The person loses the rights that the team gives. Grants on a board stay.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `team` | string | yes | — | Slug or UUID. |
| `user` | string | yes | — | The member. A UUID also finds a person who left the organization. |

Requires the organization admin role or `manage_structure` on the
organization. Refuses: `NOT_FOUND` when the person is not a member of the
team.

### `set_board_capabilities`

Sets the board capabilities of a person on a board, as
`PATCH /api/boards/{id}/members/{user_id}` does.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `board` | string | yes | — | Slug or UUID. |
| `user` | string | yes | — | A member of the organization. |
| `capabilities` | array of strings | yes | — | The full set. It replaces the set of the person. An empty list revokes each capability. |

A person with no capability on the board gets the set, as `grant_capability`
gives it. Requires `administer_members` on the board, or `manage_structure`
on the board or on the organization. Refuses: `VALIDATION` for a capability
outside the vocabulary.

### `add_column`

Adds a column to a board, as `POST /api/boards/{id}/columns` does. The new
column has no transition.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `board` | string | yes | — | Slug or UUID. |
| `name` | string | yes | — | The name of the column. |
| `position` | integer | no | the last | From 0, the first column. |

Requires `configure_boards` on the board. Refuses: `DUPLICATE_COLUMN_NAME`.

### `update_column`

Changes a column, as `PATCH /api/boards/{id}/columns/{col_id}` does.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `board` | string | yes | — | Slug or UUID. |
| `column` | string | yes | — | Name or UUID. |
| `name` | string | no | no change | The new name. |
| `position` | integer | no | no change | From 0, the first column. |
| `is_done` | boolean | no | no change | A done column. |
| `claims` | boolean | no | no change | A task in the column has a claim. |
| `keeps_claims` | boolean | no | no change | A task with a claim keeps it in the column, and a task with no claim gets none there (Blocked, KAIROS-T-0529). |
| `wip_limit` | integer | no | no change | The WIP limit, 1 or more. |
| `no_wip_limit` | boolean | no | `false` | `true` removes the WIP limit. Not with `wip_limit`. |

Requires `configure_boards` on the board.

### `remove_column`

Removes a column, as `DELETE /api/boards/{id}/columns/{col_id}` does. Its
transitions stop.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `board` | string | yes | — | Slug or UUID. |
| `column` | string | yes | — | Name or UUID. |

Requires `configure_boards` on the board. Refuses: `COLUMN_NOT_EMPTY` while
the column holds a live item.

### `set_transition`

Allows or stops a move between two columns, as
`POST /api/boards/{id}/transitions` and
`DELETE /api/boards/{id}/transitions/{transition_id}` do.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `board` | string | yes | — | Slug or UUID. |
| `from_column` | string | yes | — | The column where the move starts. Name or UUID. The tool also accepts the name `from`. |
| `to_column` | string | yes | — | The column where the move ends. Name or UUID. The tool also accepts the names `to` and `column`. |
| `allowed` | boolean | yes | — | `true` adds the transition. `false` removes it. |

Requires `configure_boards` on the board. Refuses: `DUPLICATE_TRANSITION`.

### `save_metadata_definition`

Makes or changes a metadata definition, as `POST` and
`PATCH /api/metadata-definitions` do.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `definition` | string | no | none | The definition to change. Slug or UUID. With no value, the tool makes a definition. |
| `name` | string | for a new one | no change | The name. |
| `slug` | string | for a new one | no change | The slug. |
| `field_type` | string | for a new one | — | `text`, `number`, `date`, `enum` or `boolean`. It does not change later. |
| `enum_options` | array of strings | no | no change | The values of an enum, in their order. It replaces the list. |
| `entity_types` | array of strings | no | no change | The entity types of the definition. Empty: each type. It replaces the list. |

For organization admins only.

### `delete_metadata_definition`

Deletes a metadata definition, as `DELETE /api/metadata-definitions/{id}`
does.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `definition` | string | yes | — | Slug or UUID. The tool also accepts the name `reference`. |

For organization admins only. Refuses: `DEFINITION_IN_USE` while an item has a
value or a template has the field. The message names them.

### `save_template`

Makes or changes a template, as `POST` and `PATCH /api/templates` do.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `template` | string | no | none | The template to change. Slug or UUID. With no value, the tool makes a template. |
| `name` | string | for a new one | no change | The name. |
| `slug` | string | for a new one | no change | The slug. |
| `content` | string | no | no change | The markdown of a new item of the template. |
| `metadata` | array | no | no change | The fields: `definition` (the slug of a metadata definition; the old name `definition_slug` is an alias), optional `default_value`, `required`. It replaces the list. |

For organization admins only.

### `delete_template`

Deletes a template, as `DELETE /api/templates/{id}` does. Items made from it
keep their content and their metadata.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `template` | string | yes | — | Slug or UUID. The tool also accepts the name `reference`. |

For organization admins only.

## Team pages

The team page of a team holds its charter and a tree of folders and pages that
the team grows. A folder holds folders and pages. A page holds nothing.

These tools call the functions of the `/api/teams/{id}/pages` routes, so the
rights and the refusals are the same. Every member of the organization can
read. To write, you must be a member of the team or an organization admin. A
board capability or `manage_structure` does not give the right to write.

A `page` or `parent` argument is a path of slugs from the root or the UUID of
the page. An example of a path is `documentation/runbooks`. The path `charter`
is the charter.

### `team_pages`

Reads the team page of a team: the tree, or one page or folder in full.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `team` | string | yes | — | Slug or UUID. |
| `page` | string | no | — | A path or a UUID. With it, the result is the content, the version and the id of that node. |

The tree has one line for each node: the slug, the title, `[charter]` or
`[folder]`, the version and the id. The charter is first. Siblings are in
position order.

The documents and the ADRs of the team are nodes of the tree too. An item is
the team's when a board of the team owns it. Its line has the short code, the
title, the type and the status: `[document, draft]`, or the column of an ADR.
The board keeps the item and its lifecycle. The tree only gives its place. An
item that nobody placed is at the end of the root.

Refuses: `VALIDATION` for an unknown team or page.

### `create_team_page`

Creates a page or a folder, as `POST /api/teams/{id}/pages` does.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `team` | string | yes | — | Slug or UUID. |
| `kind` | string | yes | — | `page` or `folder`. |
| `title` | string | yes | — | The title. |
| `slug` | string | no | from the title | Unique among the siblings. |
| `parent` | string | no | the root | The folder. A page cannot be a parent. |
| `content` | string | no | empty | The markdown of a page. |
| `position` | integer | no | after the last sibling | The position among the siblings. |

Refuses: `FORBIDDEN` for a caller who is not a member of the team and not an
organization admin. `VALIDATION` for an unknown team or parent, and for a parent
that is a page. `VALIDATION` also for a slug that a sibling has, and for a
reserved slug at the root.

### `edit_team_page`

Edits the content of a page, as `PATCH /api/teams/{id}/pages/{page_id}` does
with `content` and `version`.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `team` | string | yes | — | Slug or UUID. |
| `page` | string | yes | — | A path or a UUID. |
| `content` | string | yes | — | The whole new content. |
| `version` | integer | yes | — | The version that you read. |
| `title` | string | no | — | A new title. |

Each edit writes a version to the history of the page. Refuses: `CONFLICT`
when the page has a newer version. The error has the current page. `FORBIDDEN`
as for `create_team_page`.

### `move_team_page`

Moves, reorders or renames a page or a folder, as
`PATCH /api/teams/{id}/pages/{page_id}` does with `slug`, `parent_id`,
`move_to_root` and `position`.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `team` | string | yes | — | Slug or UUID. |
| `page` | string | yes | — | A path or a UUID. Or the short code of a document or an ADR of the team, to put that item in the tree. |
| `parent` | string | no | — | The new folder. |
| `to_root` | boolean | no | `false` | `true` moves the node to the root. Not with `parent`. |
| `slug` | string | no | — | A new slug. A document or an ADR has no slug in the tree. |
| `position` | integer | no | — | A new position among the siblings. |

Refuses: `VALIDATION` for a move into the node itself or into a folder in it.
`VALIDATION` also for the charter, which cannot move. `FORBIDDEN` as for
`create_team_page`.

### `delete_team_page`

Archives a page or a folder, as `DELETE /api/teams/{id}/pages/{page_id}` does.
A folder is archived with everything in it. A person restores archived nodes in
the Archived list of the Pages view of the team (`/teams/<slug>/pages/`).

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `team` | string | yes | — | Slug or UUID. |
| `page` | string | yes | — | A path or a UUID. |

Refuses: `VALIDATION` for the charter. `FORBIDDEN` as for `create_team_page`.

## Your notifications

A notification tells a person about an event of their work (KAIROS-T-0381):

| Kind | You get it when |
|---|---|
| `hand_off` | A person or a service account gives you the claim of a task. |
| `blocker_cleared` | An open blocker of your item stops blocking it. The blocker goes into a done column, or a person archives or purges it, or removes the `blocks` link. A cancel sends `blocker_cancelled` in its place. Your item is a task that you have the claim on, or a strategy or initiative that you own. |
| `blocked_again` | A done blocker of your item leaves the done column, so it blocks again. Your item is a task that you have the claim on, or a strategy or initiative that you own. With no claim and no owner, the person who created the item gets it. |
| `cancelled` | A person cancels a task that you created. The text has the reason. |
| `blocker_cancelled` | A person cancels a task that blocks your item. The cancelled task still blocks, and Kairos unblocks nothing. Remove the `blocks` link, or reopen the blocker. You get it when you have the claim on the item, own it, or created it. The text has the reason. |
| `mention` | A person writes `@your_user_name` in a note or in the content of an item. A mention in code does not count. An update tells only the new mentions. |
| `p0_bug` | A bug with the priority `critical` (or `p0`) lands on the board of your team: its priority changes to P0, or it moves to the board. |
| `delivery_failing` | Kairos turns off a webhook target that you made, or a target of a board that you manage, after failed deliveries. See [Webhook deliveries](../webhooks/). |
| `agent_key_revoked` | An organization admin revokes an agent key of yours. The text names the key and its prefix. |
| `claim_dropped` | A person leaves the organization, and their claim on a task of the board of your team ends. The task stays in its column, free. |
| `claim_stale` | A claim has no write and no state change for `KAIROS_STALE_CLAIM_DAYS` days (default 14). You get it for your claim, or for a claim on a task of an initiative that you own. The claim stays. Kairos sends it once for each quiet period. |
| `key_expiring` | An agent key of yours expires in 7 days or less. An organization admin also gets it for a key of a service account. Kairos sends it one time for each key. The text names the key and the `kairos keys rotate` command. |
| `proposal_decided` | A person confirms or rejects an [edge proposal](#propose_edge) that you made (KAIROS-T-0558). The text names the edge, the state and the decider. A proposal of your agent key is your proposal. |
| `notice` | A person, or an agent key of a person, sends you a [notice](#send_notice): to you, or to a team of yours (KAIROS-T-0737). The text names the sender, and the key when an agent sent it. |

You get no notification for a change that you made. You can turn a kind off
with `PUT /api/me/notification-settings` or on the Notifications page of the
GUI.

The block at the end of an answer. It is a content block of its own, after
the answer (KAIROS-T-0549). The content that `get_item` gives stays the
content of the item. Its lines have no id: `get_events` gives the ids.

The heading gives the count up to 5. With more than 5, it says `5+ unread`
(KAIROS-T-0612). `get_events` gives the full count.

```text
## Your notifications (5+ unread)
- 2026-10-12 09:15Z · hand_off · ACME-T-0012: Robin gave you the claim on ACME-T-0012: Fix the login
- ...
More: call get_events
get_events gives each with its id; mark them read with mark_events_read when you have acted on them.
```

### The read state

Each reader keeps its own read state (KAIROS-T-0550). The person is one
reader: the bell of the web app and the login of the person. Each agent key
of the person is a different reader. When an agent marks a notification
read, the bell of the person still shows it unread.

- A new agent key starts with the state of the person at that time. A
  notification that the person read before the key was made is read for the
  key.
- A rotated key keeps the reads of the key that it replaces.
- `get_events`, `mark_events_read` and the block at the end of an answer
  use the state of the caller. So do the two REST routes.

### `get_events`

Gives your notifications: each unread one first (newest first, 100 at most),
then the 20 newest read ones. Each line has the id, the time, the kind, the
item and the text.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `before` | string | no | — | The id of a notification. The page has the unread notifications older than it, and no read ones. |
| `unread` | boolean | no | false | `true`: only the unread notifications, and no read ones (KAIROS-T-0683). |

With `unread: true`, the first line after the heading names the newest unread
id:

```text
Newest unread id: "4f1c2a8e-0000-4000-8000-000000000001". Send it as `up_to` with `all: true` to mark_events_read.
```

With no unread notification, the answer is `No unread ones.`

The answer ends with the `up_to` value for `mark_events_read`: the id of the
newest unread notification. When the page has 100 unread notifications, the
answer also gives the `before` value of the next page.

Refuses: `NOT_FOUND` when `before` is not one of your notifications.

REST: `GET /api/me/notifications`, with the query parameter `before`.

### `mark_events_read`

Marks your notifications read for the caller. Send `ids`, or `all: true`
with `up_to`.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `ids` | array of strings | no | — | The ids of your notifications, as `get_events` gives them. |
| `all` | boolean | no | — | `true`: each of your unread notifications up to `up_to`. |
| `up_to` | string | with `all` | — | The id of the newest notification that you read. Kairos marks it and each older unread notification. A newer notification stays unread. `get_events` with `unread: true` names it. |

Kairos refuses `all` without `up_to`. A notification can come after your list.
Without a bound, Kairos would mark it read before you see it.

`up_to` also marks the older unread notifications that are not on your
page. Read each page before you send `all`.

The result is one line: `Marked 2 notification(s) read. You have 5 unread.`

Refuses: `VALIDATION` when the call has neither `ids` nor `all: true`, or has
both. `VALIDATION` when `all` has no `up_to`, or `up_to` comes with `ids`.
`NOT_FOUND` when an id is not one of your notifications. The message names
the id, and nothing changes. An id that is read already does not change.

REST: `POST /api/me/notifications/read`.

### `watch_item`

Starts your watch on an item (KAIROS-T-0736). These changes of the item give
you a notification of the kind `watch`:

- moved: the item goes to a different column, or to a different board;
- edited: the title or the content changes;
- note added: a person or an agent adds a note;
- done: the item goes into a done column. A cancel is done.

A change that you make gives you no notification. Other changes (metadata,
links, the live state) give no notification.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The code of the item. |
| `with_children` | boolean | no | false | `true`: a change of a child of the item gives you a notification too. A child is an item that has the item as its parent. |

A second call changes `with_children`. An agent key watches for its person:
the watch is the person's. The REST routes show it.

The result is one line: `You watch ACME-T-0001. A move, an edit, a note or a
done of the item gives you a notification.`

Refuses: `NOT_FOUND` for an unknown code, a retired code or an archived item.
`FORBIDDEN` for a service account.

REST: `PUT /api/items/{short_code}/watch`, with the body
`{"with_children": true}`. `GET` on the same path gives your watch.

### `unwatch_item`

Stops your watch on an item.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The code of the item. |

The result is `You do not watch ACME-T-0001 now.` When you do not watch the
item, the result is `You did not watch ACME-T-0001. Nothing changed.`

Refuses: `NOT_FOUND` when no item has the code. `FORBIDDEN` for a service
account.

REST: `DELETE /api/items/{short_code}/watch`.

### `list_watches`

Gives your watches, one line for each item. A line has the code, the type and
the title of the item. It has `with children` when the watch includes the
children. The tool has no arguments.

```text
# Your watches (2)

- ACME-I-0003 (initiative) · with children — The new search
- ACME-T-0012 (task) — Fix the login
```

With no watch, the result is `You watch no item. watch_item starts a watch.`

Refuses: `FORBIDDEN` for a service account.

REST: `GET /api/me/watches`.
### `send_notice`

Sends a notice to a person or to each member of a team (KAIROS-T-0737). A
notice is a short text with an optional item link. Each recipient gets a
notification of the kind `notice` in the bell. The open pages of the
recipient get the push.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `to` | string | `to` or `team` | — | A person of the organization: `me`, an id, an email or a user name. |
| `team` | string | `to` or `team` | — | A team: its slug or its id. Each member gets the notice. |
| `text` | string | yes | — | The text: 1 to 2000 characters. |
| `short_code` | string | no | — | The short code of a live item to link. |
| `request_key` | string | no | — | A key for a retry (KAIROS-A-0065). |

A notice from an agent key names the key and its person. `to: me` sends the
notice to your own person: an agent can tell its person that a task is done.
A person with no key does not get their own notice.

Each agent key can send 20 notices in one hour. The operator sets the limit
with `KAIROS_NOTICE_LIMIT_PER_HOUR`. A person with no key counts as one
sender. A rotated key keeps the count of the keys that it replaced. A notice
to a team counts as one.

A recipient who turned the kind `notice` off, or muted the key, gets nothing.

The result names the notice and the target. It gives the count of the
persons who got it. It also gives the notices that you can send in this
hour:

```text
Sent the notice 6f1c2a8e-0000-4000-8000-000000000002 to the team platform, with the item ACME-T-0012.
3 persons got a notification.
You can send 19 more notice(s) in this hour.
```

Refuses:

- `VALIDATION` when the call has neither `to` nor `team`, or has both.
- `VALIDATION` when `to` is not a person of the organization.
- `VALIDATION` when the text is empty or longer than 2000 characters.
- `NOT_FOUND` when Kairos does not find the team or the item.
- `FORBIDDEN` for a service account.
- `KEY_SCOPE` when the scope of the agent key does not have `member`.
- `NOTICE_RATE_LIMITED` when the key sent the limit in the last hour.
  Kairos sends nothing. `details.retry_after_secs` gives the seconds to wait.

REST: `POST /api/notices`.

## Agent dispatch

A board manager sets rules that send a task to an agent runner
(KAIROS-T-0385). A task can enter the column of a rule and match its filter.
Then Kairos sends one signed delivery to a webhook target. Kairos does not
start a session itself: the runner does. See
[Webhook deliveries](../webhooks/) for the request, the signature and the
tries. To push the deliveries into a running Claude Code session, see
[Receive dispatch in a Claude Code session](../../how-to/receive-dispatch-in-a-session/).

The event is `task.entered_column`. On an initiative or a strategy board
(KAIROS-T-0719) the rule fires when an initiative or a strategy enters the
column. Then the event is `initiative.entered_column` or
`strategy.entered_column`. Such a rule cannot filter on a task type or a
repository. Its `data` has these fields:

| Field | Description |
|---|---|
| `entity_type` | `task`, `initiative` or `strategy`. |
| `code`, `title`, `task_type` | The item. `task_type` is `null` for a strategy or an initiative. |
| `work_class` | The lane of the task: `planned` or `support`. `null` for a strategy or an initiative. |
| `link` | The URL of the task in the GUI. |
| `board` | `id`, `slug` and `name` of the board. |
| `column` | `id` and `name` of the column. |
| `repository` | `slug`, `full_name`, `forge` and `url` of the repository of the task, or `null`. |
| `actor` | `id` and `name` of the person who moved the task. |
| `rule_id` | The rule. |

A task enters a column in 3 ways:

- a transition;
- a move to the board, into its entry column;
- a cancel, into the done column.

A rule with the trigger `unblocked` (KAIROS-T-0427) fires when an item loses
its last open blocker. The blocker goes into a done column, or a person
archives or purges it. A person can also remove the `blocks` link with
`unlink_items` (KAIROS-T-0568). A cancel does not fire the rule: a cancelled
blocker still blocks (KAIROS-T-0567). The item can be a task, an initiative or
a strategy.

The event is `task.unblocked` for each type. Its `data` has the
fields above and `blocker`, the code of the blocker. The column in `data` is
the column of the item.

The rule names no done column: an item in a done column waits for nothing.

The rule can name a column: the task must be in it. With no column, a task
in each column that is not done matches. A task with two blockers fires once,
when the second closes. A blocker that opens again blocks the task again, so
its next close fires again.

A rule with the trigger `review_changed` (KAIROS-T-0718) fires when the
review state of a task changes ([`set_review`](#set_review)). The event is
`task.review_changed`. Its `data` has the fields above and `review`, the new
value: `not_needed`, `needed` or `done`. The rule can name a column, as an
`unblocked` rule can, and names no done column. A write of the value the
task has fires nothing.

### `set_agent_triage`

Sets who decides the triage of intake on a board (KAIROS-T-0428). Intake is a
task in the support lane. Triage moves it into the planned lane, or into
another column.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `board` | string | yes | — | Slug or UUID. |
| `agents_decide` | boolean | yes | — | `true`: an agent decides. `false`: an agent proposes, and a person decides. |

An agent is a caller with an agent key, or a service account. Intake is a
task in the support lane in the entry column of its board. Its triage is
`set_work_class` to planned, `transition_item` out of the entry column, or
`move_item` to another board. A `cancel_item`, a `delete_item` and a
`purge_task` are triage too (KAIROS-T-0537).

With the setting off (the default), Kairos refuses an agent's triage with
`AGENT_TRIAGE_OFF`. The agent then writes its proposal with `append_note`: planned backlog, work now,
another team, or won't do, and why. A person moves the task.

Once a person has triaged the task out of the entry column, an agent moves it
as any other task. The dispatch payload carries the setting as
`board.agents_decide`.

With the setting on, the agent's move works, and the activity log gets a line
`agent_triage:` for it. A person's triage works in each case.
`list_dispatch_rules` shows the setting.

Requires `configure_boards` on the board. REST:
`GET` and `PUT /api/boards/{id}/agent-triage`.

### `set_board_policy`

Sets the policies of a board (KAIROS-T-0641, KAIROS-T-0643, KAIROS-T-0565).
Each policy is on or off per board.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `board` | string | yes | — | Slug or UUID. |
| `parent_gate` | boolean | no | — | A delivery board only. `true`: a task moves into a claims column only while its parent initiative is active. `false`: no check. |
| `owner_gate` | boolean | no | — | An initiative board only. `true`: an initiative moves into Active only with an owner, and the owner of an initiative in Active or Monitoring cannot release it. `false`: no check. |
| `blockers_gate` | boolean | no | — | A delivery board only. `true`: a task with open blockers does not move into a claims column or a done column. `false`: the move gets a warning. |

`parent_gate` is on by default. With it on, a task moves into a claims column
only while its parent initiative is in Active or Monitoring. A bucket
initiative is always active. A task with no parent initiative moves freely.

The check runs on a transition, a move to the board and a create. A refused
move is `PARENT_NOT_ACTIVE`. A task that is in Active already stays there.

`owner_gate` is on by default. With it on, an initiative moves into its Active
column only with an owner. A move from Active to Monitoring needs no check:
the initiative has an owner already.

In Active and in Monitoring, Kairos refuses a release of the owner, also for
an organization admin. A hand-off to a member
of the board changes the owner. The refusal is `OWNER_REQUIRED`. An
initiative in Active with no owner stays there.

`blockers_gate` is off by default. An open blocker is an item that blocks the
task and is not done. Kairos counts a cancelled blocker as open.

With the policy off, a move into a claims column or a done column succeeds
with a warning. With it on, Kairos refuses the move with `BLOCKED`. The check
runs on a transition, a reopen and a move to the board. A cancel is no move of
this kind. A task in Active with open blockers stays there.

Send at least one policy. A policy of a different level of board is
`VALIDATION`. `my_boards` shows the policies of each board.

Requires `configure_boards` on the board. REST: `PATCH /api/boards/{id}`.

### `list_dispatch_rules`

Gives the rules of a board, the webhook targets of the board, and the 20
newest deliveries of the log. A person who belongs to the board can read
them:

- an organization admin;
- a person with a grant on the board;
- a member of the team of the board.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `board` | string | yes | — | Slug or UUID. |

A person who cannot configure the board gets the URL of a target with no
query.

Refuses: `NOT_FOUND` for an unknown board; `FORBIDDEN` for a person who does
not belong to the board.

REST: `GET /api/boards/{id}/dispatch-rules`, `GET /api/boards/{id}/webhook-targets`
and `GET /api/boards/{id}/webhook-deliveries`.

### `create_dispatch_rule`

Makes a rule. Send `target_id` or `url`, not both. With `url`, Kairos makes a
new target, and the result has its secret. Kairos shows the secret this one
time. Requires `configure_boards` on the board.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `board` | string | yes | — | Slug or UUID. |
| `trigger` | string | no | `entered_column` | `entered_column`: an item enters `column` (a task on a delivery board, an initiative or a strategy on its board). `unblocked`: the last open blocker of a task closes. `review_changed`: the review state of a task changes. |
| `column` | string | for `entered_column` | — | The name or the UUID of a column of the board. For `unblocked`, the column the task must be in. |
| `target_id` | string | no | — | A webhook target of the board. |
| `url` | string | no | — | The URL of a new target (`http` or `https`). |
| `target_name` | string | no | `Agent runner` | The name of the new target. |
| `task_type` | string | no | — | Only tasks of this type: `task`, `bug`, `tech_debt` or `support`. Refused on an initiative or a strategy board. |
| `definition` | string | no | — | Only tasks with this metadata: the slug of a definition. Send it with `metadata_value`. The old name `metadata_slug` is an alias. |
| `metadata_value` | string | no | — | The value that matches. Case is not significant. |
| `repository` | string | no | — | Only tasks of this repository: its slug or UUID. Refused on an initiative or a strategy board. |

Each part of the filter that you set must match.

Refuses:

- `FORBIDDEN` without `configure_boards`;
- `NOT_FOUND` for an unknown board, target or repository;
- `VALIDATION` for an unknown trigger, column, task type or metadata
  definition, and for an `entered_column` rule with no column;
- `VALIDATION` for a URL that is not `http` or `https`, or a private address;
- `SECRETS_NOT_CONFIGURED` for a new target with no `KAIROS_SECRETS_KEY`.

REST: `POST /api/boards/{id}/webhook-targets` and
`POST /api/boards/{id}/dispatch-rules`.

### `delete_dispatch_rule`

Removes a rule. Its deliveries stay in the log, and its target stays.
Requires `configure_boards` on the board.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `board` | string | yes | — | Slug or UUID. |
| `rule_id` | string | yes | — | The id of the rule, from `list_dispatch_rules`. |

Refuses: `FORBIDDEN` without `configure_boards`; `NOT_FOUND` for an unknown
board or rule.

REST: `DELETE /api/boards/{id}/dispatch-rules/{rule_id}`.

The next three tools work on a webhook target of a board (KAIROS-T-0558). They
are in the admin profile. Each one requires `configure_boards` on the board, as
REST does. Each one takes these arguments:

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `board` | string | yes | — | Slug or UUID. |
| `target_id` | string | yes | — | The id of the target, from `list_dispatch_rules`. |

Each one refuses:

- `FORBIDDEN` without `configure_boards`;
- `NOT_FOUND` for an unknown board, or a target of a different board;
- `VALIDATION` for a `target_id` that is not a UUID, and for an unknown
  argument.

### `remove_webhook_target`

Removes a target, with its rules and its delivery log. You cannot undo it.

REST: `DELETE /api/boards/{id}/webhook-targets/{target_id}`.

### `enable_webhook_target`

Turns a target on again and sets its failed tries in a row to 0. Kairos queues
the `skipped` deliveries of the last 30 days again, oldest first. The answer
gives their count.

REST: `POST /api/boards/{id}/webhook-targets/{target_id}/enable`.

### `test_webhook_target`

Sends a test delivery with the event `ping` to a target. A target that is off
gets a `skipped` delivery. The answer gives the id and the status of the
delivery. `list_dispatch_rules` shows the delivery in the log.

REST: `POST /api/boards/{id}/webhook-targets/{target_id}/test`.

## Read tokens of the organization

The code index builder of the server fetches each repository with git
(KAIROS-T-0431). An organization admin can keep one read token for each
forge host, for example `github.com`. The builder uses the token of the host
of a repository first. It uses the token of the repository only for a host
that has no token of the organization.

With no token, the fetch has no credential. The log of each fetch, and the
note of a failed build, say which token the fetch used. They never show the
token.

Kairos encrypts each token with `KAIROS_SECRETS_KEY`. No tool and no route
gives a token back. Each set, change and removal writes an activity row with
the host.

The host is the host of the URL of the repository, in lowercase. It has a
port only when the port is not 443 (or 80 for `http`). Kairos changes
`GitHub.com`, `https://github.com/` and `github.com:443` to `github.com`.

### `list_org_read_tokens`

Gives the hosts that have a token, and for each host who set the token and
when. Only an organization admin. No arguments.

Refuses: `FORBIDDEN` for a person who is not an organization admin.

REST: `GET /api/org/read-tokens`.

### `set_org_read_token`

Sets the token of a host, or replaces it. Only an organization admin.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `host` | string | yes | — | The host only, for example `github.com`. |
| `token` | string | yes | — | A token that can only read the code. |

Refuses:

- `FORBIDDEN` for a person who is not an organization admin;
- `VALIDATION` for a host with a path, a user name or a scheme that is not
  `http` or `https`, and for a token that is empty, too long or has a space;
- `SECRETS_NOT_CONFIGURED` when the deployment has no `KAIROS_SECRETS_KEY`.

REST: `PUT /api/org/read-tokens/{host}`.

### `remove_org_read_token`

Removes the token of a host. The next fetch of a repository on the host uses
the token of the repository, or no token. Only an organization admin.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `host` | string | yes | — | The host only, for example `github.com`. |

Refuses: `FORBIDDEN` for a person who is not an organization admin;
`NOT_FOUND` for a host with no token.

REST: `DELETE /api/org/read-tokens/{host}`.

## The count of the refusals

Kairos keeps one row for each tool call that ends in a tool error
(KAIROS-T-0694, KAIROS-A-0068). The row has the tool, the code of the
refusal and the `field` of its details. It also has the kind of the caller:
`person`, `agent_key` or `service_account`. It has no argument value, no text
of the refusal and no person. The retention pass deletes the rows after 90
days. Kairos does not count the refusals of the REST API.

### `refusal_counts`

Gives the count of the refusals for each tool and code in a range of days,
the highest first. Each row has the field that the refusals name most, and
the count for each kind of caller. Use it to find the refusals that agents
meet most. Only an organization admin.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `days` | integer | no | `7` | The number of days of the range, 1 to 90. |
| `to` | string | no | now | The last day of the range, `YYYY-MM-DD` in UTC. The range includes that day. |
| `limit` | integer | no | `20` | The number of rows, 1 to 100. |

```text
Refusals from 2026-10-18 16:00 UTC to 2026-10-25 16:00 UTC: 214 in all, in 31 pairs of tool and code. The top 2 of 31 pairs:

| tool | code | count | field named most | person | agent key | service account |
|---|---|---|---|---|---|---|
| link_items | RELATIONSHIP_RULE | 41 | - | 3 | 38 | 0 |
| create_item | VALIDATION | 29 | parent | 1 | 28 | 0 |
```

A range with no refusal gives `No refusals from <start> to <end>.`

Refuses:

- `FORBIDDEN` for a person who is not an organization admin;
- `VALIDATION` for `days` or `limit` out of range, and for a `to` that is
  not a date. `details.field` names the argument.

## Related reading

- [Archiving](../../explanation/archiving/) — why archiving is not a permission
  boundary
- [Capabilities and access](../../explanation/capabilities-and-access/) — the
  capability vocabulary and the rule for a request to a different team
- [Repositories as execution scope](../../explanation/repositories-as-execution-scope/)
  — why the team decides the board and the repository is a link
- [Glossary](../glossary/)
- [REST API](../rest-api/)

## Related guides

- [Connect over MCP](../../how-to/connect-over-mcp/)
- [Give an agent machine access](../../how-to/give-an-agent-machine-access/)
- [Run an admin agent](../../how-to/run-an-admin-agent/)
- [Find archived work](../../how-to/find-archived-work/)
