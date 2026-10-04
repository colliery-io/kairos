# MCP tools

Kairos serves the Model Context Protocol at `/mcp`. The surface is exactly
twenty-three tools. A drift gate in the test suite
asserts that `tools/list` returns these twenty-three and no others. The same
gate asserts that this page has one section for each tool.

The promise is for one release: this page agrees with `tools/list`. The count
is not a promise for later releases. A later release can add a tool or an
argument.

This page describes Kairos 0.5.0. Argument names, types and defaults are those
of the JSON schema the server sends in `tools/list`.

## Conventions

These hold for every tool.

| Aspect | Rule |
|---|---|
| Tenant | There is no tenant argument. The tenant is the one resolved from the connection's host or the `X-Tenant` header. |
| Identity | Every tool runs as the authenticated principal — a human user or a service account — under the same attribute-based access control as the REST handlers. Archiving is not a permission boundary: whoever could read an item before it was archived can read it after. |
| Item identity | Items are named by short code, e.g. `ACME-T-0012`, in every input and every output. |
| Retired codes | When an item gets a new short code, its old code is retired. Kairos does not issue a retired code again. A read tool (`get_item`, `get_history`, `search`) finds the item from a retired code and shows the current code. A write tool refuses a retired code with `NOT_FOUND`, and the refusal gives the current code. |
| Board and team references | A `board`, `to_board`, `team` or `repository` argument accepts either a slug or a UUID. A `column` or `to_column` argument accepts either a column name, case-insensitively, or a UUID. |
| Listing weight | Listings are compact: short code, title and key fields. Full markdown content arrives only from `get_item` and from `get_history` with a `version`. |
| Errors | A refusal comes back as an MCP tool error whose text is `CODE: message`, the code being the same stable one as the REST API's error envelope. Where that code has structured `details`, a second line follows: `details: ` and the JSON object. See [Errors](errors.md) for what each code's `details` carries. |
| Audit | Writes are recorded in the activity log by the same code path as the REST API. |

### Refusal codes

Codes an agent can receive, and what each means.

| Code | Meaning |
|---|---|
| `NOT_FOUND` | A short code, board or repository named as the subject of the call does not exist; an `unlink_items` edge does not exist; or a `search` `traverse.from` does not resolve. On a write tool it also means the item exists but is archived: writes resolve live items only, and the message reads `no live item with short code …`. |
| `VALIDATION` | An argument is malformed, an enum value is outside its vocabulary, an argument does not apply to the item type, or something named as a *filter or reference* — a team, a repository filter, a parent, a metadata definition, a column — does not exist. A reference that does not resolve is `VALIDATION`; the call's own subject not existing is `NOT_FOUND`. **One exception:** `search`'s `traverse.from` is a reference and still answers `NOT_FOUND`, because a traversal's root is the subject of that traversal. |
| `FORBIDDEN` | The rule for the write refuses the principal. For an edit, the [edit rule](capabilities.md#the-edit-rule): the principal did not create the item and lacks `manage_<type>` on its board. For an edge, the [link rule](capabilities.md#who-can-write-relationships): the principal can edit neither end. For a move or a create: the principal lacks the board capability. The message names the capability. For `add_repository` and `update_repository`: the principal is not a member of the owner team and is not an organization admin. The message names the team. One case has a longer message: a move (`transition_item`, `move_item`) of a request that the caller created, while the request is in the entry column. That message says that the item is a request, that the team of the board moves it, that the caller can edit, link and archive it, and which capability the move needs. A `create_item` that sends `work_class: planned` to a board that the caller does not manage is also `FORBIDDEN`. |
| `CONFLICT` | An optimistic-concurrency version mismatch. The refusal carries the current version and content. From `add_repository`: the directory has the slug already, or it has the repository already. |
| `INVALID_TRANSITION` | The target column is not reachable from the item's current column in the board's transition graph. The refusal enumerates the allowed target columns. |
| `ITEM_NOT_ON_BOARD` | The item has no board placement, so it cannot be transitioned or moved. |
| `SAME_BOARD` | A `move_item` whose target is the board the task is already on. A document is different: see [`move_item`](#move_item). |
| `NOT_DELIVERY_BOARD` | A `move_item` whose target board is not a delivery board. |
| `NO_ENTRY_COLUMN` | The target delivery board has no entry column to land the task in. |
| `RESTORE_BLOCKED` | The archived item's board, column, owning team or repository no longer exists. The refusal names what is missing. |
| `RELATIONSHIP_RULE` | The relationship type is not allowed between those two item types. For `impacts`: the source is not a document and not an ADR. |
| `CYCLE_DETECTED` | The edge would create a cycle. |
| `ALREADY_LINKED` | That edge already exists. For `impacts`: the item impacts that repository already. |
| `LAST_PARENT` | An `unlink_items` of the last `supports` edge of a document that names no board. |
| `LAST_OWNER` | A `move_item` that removes the owner board of a document that supports no item. |
| `RENAME_NOT_NEEDED` | A `move_item` with `rename` to a board whose prefix the code has already. |

**Each tool refuses an argument that it does not know.** Each tool has the
rule of the routes of the REST API, the tools that read too. The tool does not
ignore the argument, and it writes nothing. The schema of each tool shows
`additionalProperties: false`. The refusal is `VALIDATION`. Its text names the
argument and gives the arguments of the tool:

```text
VALIDATION: The call has the argument "column". This tool does not accept that argument. The arguments of this tool are: item_type, title, board, …
details: {"allowed":["item_type","title","board", …],"argument":"column"}
```

An argument can be in an object of the call, such as `filter` of `search`. Then
the list is the list of that object. `whoami` has no arguments, and its refusal
says `This tool has no arguments.`

A call without an argument that the tool must have has a refusal of the same
form: `VALIDATION: The call does not have the argument "short_code". This tool
must have that argument. …`. An argument with the wrong JSON type is
`VALIDATION` too, and the text gives the fault. See
[An input that a route does not accept](errors.md#an-input-that-a-route-does-not-accept)
for the rule of the REST API.

`DEFINITION_IN_USE`, the refusal that protects a metadata definition carrying
values, belongs to the REST surface — `DELETE /api/metadata-definitions/{id}`.
No MCP tool deletes a definition, so no MCP tool returns it. See
[Tenant configuration](rest/tenant-configuration.md).

## Orientation

### `whoami`

Identity, organization role, teams, those teams' repositories, the boards where
the caller holds write capabilities, and the capabilities every member holds
implicitly. Each board of a team of the caller shows the capabilities that the
team gives with no grant (`(team <slug>, no grant)`). On the ADR board of the
team, these include `manage_adrs`.

No arguments.

Refuses: nothing beyond transport-level authentication.

### `my_boards`

Boards in the organization, grouped by level, with column names and per-column
item counts for the caller's delivery boards. Each ADR board names its team
(`ADR board of the team <slug>`), or the organization. A team ADR board holds
the delivery ADRs of the team, with the prefix of the team. Each board of a team
of the caller has a `team capabilities:` line. This line shows the
capabilities that the team gives with no grant. On the ADR board of the team,
these include `manage_adrs`.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `level` | string | no | all levels | One of `strategy`, `initiative`, `delivery`, `adr`. |

Refuses: `VALIDATION` for a `level` outside that vocabulary.

### `list_repositories`

The repository directory. Each line of the output has these parts:

- the slug, the forge and the full name
- `owner`: the single owning team
- `owner's board`: the delivery board of that team
- `open tasks`: the open task count
- `webhooks connected`, when a webhook connection exists

The open task count includes the linked tasks on all boards. The owner of a
repository does not choose the board of a task.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `team` | string | no | all teams | Narrow to one team's repositories. Slug or UUID. |

Refuses: `VALIDATION` for an unknown `team` — it is a filter value, not a
path.

### `get_repository`

One repository in full. The output has these parts:

- the slug, the forge, the full name, the URL and the default branch
- `owner team`: the single owning team
- `owner's delivery board`: the delivery board of that team
- `open tasks (all boards)`: the open task count
- `webhooks`: `connected` or `not connected`
- `read token`: `not set`, or who set the read token of the builder of the
  code index, when, and the result of the last check. No tool gives or sets
  the token.
- the team's description of how to work in the repository
- the documents and the ADRs that impact the repository
- the in-flight branches and pull requests, each with its work item

The delivery board is the board of the owner. A task that links to the
repository can be on the board of any team. The output has no list of the
tasks that link to the repository.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `repository` | string | yes | — | Slug or UUID. |
| `include_deleted` | boolean | no | `false` | Add the archived documents and ADRs that impact the repository, each with the mark `[archived]`. |

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

Read these items with `get_item` before you plan work in the repository. See
[impacts](glossary.md#impacts).

Refuses: `NOT_FOUND` for an unknown repository, and for a repository that is
not live.

## The repository directory

`list_repositories` and `get_repository` read the directory. These two tools
write it. They do not link a task to a repository: `set_repository` does
that.

No tool deletes a repository. No tool changes the owner team or the slug of a
repository. A person does these:

| Surface | Where |
|---|---|
| GUI | The page Admin → Repositories. |
| CLI | `kairos repos update` and `kairos repos delete`. See [CLI](cli.md). |
| REST | `PATCH /api/repositories/{slug}` and `DELETE /api/repositories/{slug}`. |

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
| `default_branch` | string | no | `main` | The default branch: a branch name that git accepts. |
| `description` | string | no | empty | How to work in the repository. `get_repository` shows it to each agent. |

The glossary gives the full rule of `repo_full_name`, `repo_url` and
`default_branch`: see
[the form of the fields](glossary.md#the-form-of-the-fields-of-a-repository).

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

Changes the description, the default branch and the URL of a repository. The
rule is that of `PATCH /api/repositories/{slug}`, for the current owner team.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `repository` | string | yes | — | Slug or UUID. |
| `description` | string | no | no change | The new text on how to work in the repository. It replaces the full text. An empty string removes the text. |
| `default_branch` | string | no | no change | The new default branch: a branch name that git accepts. |
| `repo_url` | string | no | no change | The new URL for a browser: `http` or `https`, with no user name and no password. |

The call must have one or more of `description`, `default_branch` and
`repo_url`. The tool has no `slug` argument and no `team` argument. Each
checkout keeps the slug in `.claude/kairos.local.md`, and a new slug breaks
that reference.

Requires membership of the owner team, or the organization admin role.

The result names the arguments that changed the repository:

```text
Updated repository fidius: description, default_branch.
```

The tool writes only when a value is different. A call can have only the
values that the repository has. That call is a success and it writes nothing.
`PATCH /api/repositories/{slug}` does the same.

```text
No change to repository fidius: it has these values already.
```

Refuses: `VALIDATION` with `details.field` for a new value that does not have
[the form of its field](glossary.md#the-form-of-the-fields-of-a-repository).
Its result is `No change to repository fidius: it has these values already.`

Refuses: `VALIDATION` when the call has none of the three arguments.
`NOT_FOUND` for an unknown repository. `FORBIDDEN` when the caller is not a
member of the owner team and is not an organization admin.

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

The result has 200 items at most by default. The filters (`column`,
`repository`, `include_deleted`) apply before `limit` and `offset`. The order of
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

A card with dependencies that count carries `[blocked by N]`, `[blocks N]`, or
both. A `blocks` edge does not count when the item at either end is in a done
column. It does not count when the item at the other end has the `[archived]`
mark. A card in a done column carries neither tag.

Refuses: `NOT_FOUND` for an unknown or archived board; `VALIDATION` for a
`column` that is not on that board, and the refusal lists the board's columns,
and for an unknown `repository`.

### `get_item`

Full detail of one item: type, board and column, version, full markdown
content, metadata values, and relationships — parent chain, children, blockers,
supporting documents.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The item's short code. |

A document has no column. It shows its owner in the line `owner board`. The
line has one of three forms:

| Line | Meaning |
|---|---|
| `- owner board: platform-delivery` | The document names this board. |
| `- owner board: web-delivery (the board of ACME-T-0007, which the document supports)` | The document names no board. The board of its earliest parent is its owner. |
| `- owner board: (none)` | The document names no board and supports no item. |

A document or an ADR that impacts a repository has the line `impacts` in the
section of the relationships. The mark `[archived]` shows a repository that is
not live.

```text
- impacts: repository fidius (github colliery-io/fidius); repository old-lib (github acme/old-lib) [archived]
```

Archived items are returned, marked with the instant they were put away. The
column reported for an archived card is the name of the column it was put away
in, which may since have been removed from the board: a removed column is
soft-deleted rather than dropped, and this lookup deliberately ignores that so
"which column was this in?" stays answerable.

The `blocked by` and `blocks` lines list each `blocks` edge of the item. The
mark `[done]` shows an item in a done column. The mark `[archived]` shows an
item that someone put away. An edge to an item with a mark does not block. When
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

Refuses: `NOT_FOUND` only when the short code names nothing at all.

### `get_history`

An item's content version history — version, editor, timestamp, newest first.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The item's short code. |
| `limit` | integer | no | `20` | Maximum versions to list. Clamped to 1–200. |
| `version` | integer | no | — | Return that snapshot's full title and content instead of the list. |

Archived items' history is returned, marked archived.

The list of an item that a move renamed has the section **Renames**. Each
line has the old code, the new code, the time and who did the move:

```text
## Renames
- COLLIERY-T-0100 -> SKADI-T-0001 — 2026-10-03T18:20:00Z by Dylan
```

Refuses: `NOT_FOUND` for an unknown short code, or for a `version` with no
snapshot.

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
| `sort` | object | no | `created_at` descending | See below. |
| `limit` | integer | no | `25` | Page size, 1–100. |
| `offset` | integer | no | `0` | Offset into the combined result set. Must not be negative. |

`filter`:

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `entity_type` | array of string | no | all | `strategy`, `initiative`, `task`, `document`, `adr`. Must not be an empty array. |
| `board_id` | UUID string | no | — | Items on this board. A document is not a card, so this filter gives no document. |
| `column_id` | UUID string | no | — | Items in this column. |
| `team_id` | UUID string | no | — | Tasks of this team. |
| `repository` | string | no | — | The items of this repository: see below. Slug or UUID of a live repository. |
| `task_type` | array of string | no | all | `task`, `bug`, `tech_debt`, `support`. Must not be an empty array. |
| `work_class` | array of string | no | all | `planned`, `support`. Must not be an empty array. |
| `is_bucket` | boolean | no | both | Bucket or non-bucket initiatives. |
| `metadata` | object of string to string | no | — | Conditions keyed by metadata-definition slug. Values allow a trailing `*` glob. Keys must not be blank. |
| `created_after` | RFC 3339 string | no | — | Items created strictly after. |
| `created_before` | RFC 3339 string | no | — | Items created strictly before. Must be later than `created_after`. |
| `include_deleted` | boolean | no | `false` | Include archived items, marked `[archived]`. Composes with everything, `q` and `traverse` included, and counts on its own as a constraining filter. |

`filter.repository` gives three types of item:

- the tasks that link to the repository, on all boards
- the documents that impact the repository
- the ADRs that impact the repository

The filter gives no strategy and no initiative. With `team_id`, `task_type` or
`work_class`, only tasks match. `entity_type` makes the result smaller, as for
each filter.

`traverse`:

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `from` | string | yes | — | The starting item's short code. |
| `relationships` | array of string | yes | — | `parent`, `supports`, `informs`, `supersedes`, `blocks`. Must not be empty. |
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
key, an inverted date range, a
missing `traverse.depth`, a `depth` of 0 or above 10, an empty
`relationships`, a request with no query, no constraining filter and no
traversal, or an unknown `filter.repository`. `NOT_FOUND` for a `traverse.from`
that does not resolve — the one reference on this surface that answers
`NOT_FOUND` rather than `VALIDATION`.

Every request except one carrying `filter.repository` is validated before a
database connection is taken; that one filter needs a connection to resolve the
slug, so it is validated afterwards.

## Writing content

### `create_item`

Creates a work item and returns its new short code.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `item_type` | string | yes | — | `strategy`, `initiative`, `task`, `document`, `adr`. |
| `title` | string | yes | — | The item's title. |
| `board` | string | no | see below | Target board, slug or UUID. For a task, the board decides the team of the task. For a document, it is the owner board, and it has no default. |
| `parent` | string | no | — | Parent item's short code. Creates the `parent` edge. For a document or an ADR, creates the `supports` edge. |
| `content` | string | no | empty, or the template's | Initial markdown content. |
| `template` | string | no | — | Documents only. Template id, slug or name. |
| `task_type` | string | no | `task` | Tasks only. `task`, `bug`, `tech_debt`, `support`. |
| `repository` | string | no | — | Tasks only. Slug or UUID. An optional link that says where the code is. Any live repository, of any team. It does not choose the board. |
| `work_class` | string | no | see below | Tasks only. `planned`, `support`. |
| `hypothesis` | string | no | — | Strategies only. |
| `complexity` | string | no | — | Initiatives only. `xs`, `s`, `m`, `l`, `xl`. |
| `bucket_type` | string | no | — | Initiatives only. `tech_debt`, `bug`, `ad_hoc`. Makes the initiative a bucket rather than a dated one; `is_bucket` is derived from it. |
| `decision_maker` | string | no | — | ADRs only. |
| `decision_date` | string | no | — | ADRs only. `YYYY-MM-DD`. |

`board` may be omitted when the tenant has exactly one live board of the
matching level. For a task, that level is `delivery`. A `repository` does not
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

A document must have an owner. The call has `board`, or `parent`, or the two.

| The call has | The owner of the document | The caller needs |
|---|---|---|
| `board` | The board that `board` names. It can be a live board of each level. | `manage_documents` on that board |
| `parent` | The board of the parent. The parent is a strategy, an initiative or a task. | `manage_documents` on the board of the parent |
| `board` and `parent` | The board that `board` names. The document supports the parent too. | `manage_documents` on the board that `board` names |

The document is not a card of its owner board. It has no column, and
`board_items` does not show it. See [owner board](glossary.md#owner-board).

`repository` does not apply to a document. To say that a document impacts a
repository, create the document. Then call [`link_items`](#link_items) with
the relationship `impacts`.

The result for a document is one line. It has one of three forms:

```text
Created document ACME-D-0004: The vision of fidius (version 1), owner board platform-delivery.
Created document ACME-D-0005: PRD of the portal (version 1), supports ACME-I-0002.
Created document ACME-D-0006: Rollout plan (version 1), owner board platform-delivery, supports ACME-I-0002.
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
argument passed with the wrong `item_type`, naming the type it belongs to; for a
document with no `board` and no `parent`, or whose parent is not a strategy, initiative or
task; for a document with `repository`; for a `parent` relationship that the type rules do not allow, with the
rule in the message; for a `parent` that does not name a live item; for an unknown template,
and for a template *name* that matches more than one template, which asks for
the id or slug instead; when no live board of the required level exists; when
several do, listing their slugs; and for an unknown `repository`.
`NOT_FOUND` for an unknown `board`. `FORBIDDEN` when the caller
lacks `manage_<type>` on the resolved board. A task is different: a caller without
`manage_tasks` sends a request. That caller gets `FORBIDDEN` for a board that
is not a delivery board, and for `work_class: planned`.

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

The [edit rule](capabilities.md#the-edit-rule) applies. The caller created the
item, or holds `manage_<type>` on its authorization board, or is an
organization admin.

Refuses: `NOT_FOUND` for an unknown short code or an archived item;
`FORBIDDEN` when the edit rule refuses the caller;
`CONFLICT` for a stale `version`, carrying the current version and content.

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
the [edit rule](capabilities.md#the-edit-rule) refuses the caller.

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
`VALIDATION` when the item is not a task, and for an unknown `repository`.
`FORBIDDEN` when the edit rule refuses the caller.

## Moving work

### `transition_item`

Moves an item to another column on its own board.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The item's short code. |
| `to_column` | string | yes | — | Target column, name or UUID, on the item's own board. |

Requires `transition_items` on the item's board. A transition is a move and
not an edit. The creator of the item gets no right to move it.

Refuses: `NOT_FOUND` for an unknown short code or an archived item;
`ITEM_NOT_ON_BOARD` for an item with no placement — documents always;
`VALIDATION` for a column that is not on that board, listing the board's
columns; `FORBIDDEN` without `transition_items`; `INVALID_TRANSITION` for a
target outside the board's transition graph, enumerating the allowed targets.

### `move_item`

Moves a task to another delivery board. It lands in that board's entry column
and follows that board's team.

The tool also moves a document to a different owner board, or removes the
owner board of a document.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The short code of the task or of the document. |
| `to_board` | string | no in the schema | — | Target board, slug or UUID. A task must have it, and the board is a delivery board. For a document, see below. |
| `rename` | boolean | no | `false` | Give the item the next code of the target board. See [A rename](#a-rename). |

Requires `manage_tasks` on both the task's current board and the target. A
move is not an edit. The creator of the task gets no right to move it.

The task keeps its repository. The move does not look at the repository.

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

To remove the owner board, omit `to_board`, or send null or an empty string.
The board of the earliest item that the document supports becomes the owner.
A document that supports no item keeps its board: the refusal is `LAST_OWNER`.

The move of a document is not an edit. The caller needs `manage_documents` on
two boards:

- the board that owns the document now
- the board that owns the document after the move

An organization admin can move each document. The creator of the document gets
no right to move it. For a document that names no board, the board of its
earliest parent owns it now. For a remove, that board owns it after the move.

The tool writes no new version of the document. The activity log gets one
entry with the action `update`.

The result is one line. `(none)` shows a document that names no board.

```text
Moved ACME-D-0004: owner board platform-delivery -> web-delivery.
Moved ACME-D-0005: owner board (none) -> web-delivery.
Moved ACME-D-0005: owner board web-delivery -> (none).
```

A call can name the board that the document has. That call is a success, and
it writes nothing. The same applies to a remove for a document that names no
board.

```text
No change to ACME-D-0004: its owner board is web-delivery already.
No change to ACME-D-0005: it names no owner board.
```

#### Refusals

The tool refuses with these codes:

| Code | When |
|---|---|
| `NOT_FOUND` | The short code is unknown, or the item is archived. The `to_board` is unknown or archived. |
| `VALIDATION` | The item is not a task and not a document. A task has no `to_board`. |
| `ITEM_NOT_ON_BOARD` | The task has no placement. |
| `FORBIDDEN` | The caller does not hold `manage_tasks` on the two boards of a task. The caller does not hold `manage_documents` on the two boards of a document, and the message names the board. |
| `SAME_BOARD`, `NOT_DELIVERY_BOARD`, `NO_ENTRY_COLUMN` | For a task only. |
| `LAST_OWNER` | The call removes the owner board of a document that supports no item. |
| `RENAME_NOT_NEEDED` | A rename to a board whose prefix the code has already, or a rename of a document whose owner board does not change. |

## Relationships

### `link_items`

Creates a relationship edge between two items.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `source` | string | yes | — | Source item's short code. The edge runs source to target. |
| `target` | string | yes | — | Target item's short code. For `impacts`: the slug or the UUID of a repository. |
| `relationship` | string | yes | — | `parent`, `supports`, `informs`, `supersedes`, `blocks`, `impacts`. |

The [link rule](capabilities.md#who-can-write-relationships) applies. The
caller can edit the source or the target. One end is sufficient. The rule is
the same for each relationship type, and no type needs the admin role.

The caller can edit an item that the caller created. So a request that the
caller sent to a different team can block an item of the caller. The caller
can also edit an item with `manage_<type>` on its authorization board.

One exception is a `supports` edge to a document with no parent and no owner
board. The caller must be able to edit the document. See
[The `supports` edge of a document](capabilities.md#the-supports-edge-of-a-document).

A `blocks` edge counts only while the items at both ends can move. Complete
work does not block, and nothing blocks complete work. The edge stops counting
when the item at either end is in a done column. The edge stays, and `get_item`
marks the done end `[done]`. `link_items` does not refuse an edge to an item in
a done column.

Refuses: `VALIDATION` for a `relationship` outside the vocabulary, for a
`source` or `target` that does not name a live item, and for a self-link where
`source` and `target` are the same item; `FORBIDDEN` when the caller can edit
neither end, and the message names the capability for each end; `FORBIDDEN`
for `supports` to a document with no parent and no owner board, when the
caller cannot edit the document;
`RELATIONSHIP_RULE` when that relationship is not allowed between
those two item types; `CYCLE_DETECTED`; `ALREADY_LINKED`.

#### The relationship `impacts`

An `impacts` link says which repository a document or an ADR is about. The
`source` is the short code of the document or of the ADR. The `target` is a
live repository of the organization, of each team.

The [edit rule](capabilities.md#the-edit-rule) of the source applies. The
caller needs no right on the repository. The link gives no right on the
source, and no right on the repository. See
[Who can write an `impacts` link](capabilities.md#who-can-write-an-impacts-link).

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
| `target` | string | yes | — | Target item's short code. For `impacts`: the slug or the UUID of a repository. |
| `relationship` | string | yes | — | `parent`, `supports`, `informs`, `supersedes`, `blocks`, `impacts`. |

To remove a `supports` edge of a document, the caller must be able to edit
the document. The right to edit the source is not sufficient.

A document always has an owner. For a document that names no board, the tool
refuses to remove the last `supports` edge. Do one of these steps first:

- Link the document to a different item.
- Name an owner board for the document with [`move_item`](#move_item).

If the document has no more use, archive it. A document that names a board
can lose its last `supports` edge. See
[A document always has an owner](capabilities.md#a-document-always-has-an-owner).

For `impacts`, the rule is that of `link_items`: the caller can edit the
source. The repository can be archived. The result is one line:

```text
Unlinked ACME-D-0004 -[impacts]-> repository fidius.
```

Refuses: as `link_items`, except that a `relationship` with no such edge
between those items is `NOT_FOUND`. An `impacts` link that is not there is
`NOT_FOUND` too. The tool gives `FORBIDDEN` for a
`supports` edge of a document that the caller cannot edit. The tool gives
`LAST_PARENT` for the last `supports` edge of a document that names no board.

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
[Configure semantic retrieval](../how-to/configure-retrieval.md).

Results are **bounded** and the wording is deliberate: every line says *possible*
or *prior art*, never *blocks* or *duplicates*. At the measured precision about
half of the strongest matches are genuinely related, so these are suggestions to
check rather than facts to act on — the reasoning is
[why](../explanation/finding-related-work.md).

Items already joined by an edge are not returned: there is nothing to propose and
nothing you cannot already see.

Returns a plain note, not an error, when the deployment has embeddings disabled.

### `propose_edge`

Proposes a `parent` or `blocks` edge for a **human** to confirm. It does not
create the edge.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `source` | string | yes | — | Source item's short code. For `parent`, this is the parent. |
| `target` | string | yes | — | Target item's short code. |
| `relationship` | string | yes | — | `parent` or `blocks`. Nothing else may be proposed. |
| `why` | string | yes | — | Your reasoning, in your own words. Kept verbatim and shown to whoever decides. |

There is deliberately **no confirm or reject tool**. Deciding is a person's, and
an agent cannot rule on its own suggestion — a wrong `parent` edge re-parents
work onto a board that reports to people who will believe it, and nobody
re-reads an edge once it exists.

The proposal appears on both items in the interface, with your reasoning, for
someone to accept or decline. Confirming creates the real edge, and is refused by
the same cycle and rule checks that refuse any other edge.

To propose, a caller needs no capability: a proposal writes no edge and changes
no item. The confirm takes the
[link rule](capabilities.md#who-can-write-relationships). The person who
confirms must be able to edit the item at one end. A refused confirm leaves the
proposal pending.

Refuses: `VALIDATION` for a relationship outside `parent`/`blocks` or a short
code naming no live item; `CONFLICT` when an identical proposal is already
waiting, or when the item already holds ten undecided ones — decide some rather
than adding more.

Only `parent` and `blocks` are proposable. `supports`, `informs` and `supersedes`
are editorial, cheap to undo, and remain a person's to draw with
[`link_items`](#link_items).

## Archiving

### `delete_item`

Soft-deletes an item. The response lists everything that was cascade-deleted,
and names each descendant that the archive did not reach.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The item's short code. |
| `confirm` | boolean | yes | — | Must be `true`. |

The delete cascades through `parent` edges to each descendant that the caller
can edit. Deleted items remain retrievable by short code and searchable with
`include_deleted`; they are hidden from default listings. "Deleted" and
"archived" name the same act — see the [Glossary](glossary.md).

The [edit rule](capabilities.md#the-edit-rule) applies to the named item, and
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

Refuses: `VALIDATION` when `confirm` is `false` — checked before the short code
is looked up, so such a call never reports an unknown item; `NOT_FOUND` for an
unknown short code or an already-archived item; `FORBIDDEN` when the
[edit rule](capabilities.md#the-edit-rule) refuses the caller. An *absent* `confirm` is a schema violation
rather than a refusal — it is a required field, so the call is rejected before
the tool body runs and carries no Kairos error code.

### `restore_item`

Puts an archived item back on its board.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The archived item's short code. |

Restores only the named item. A cascade delete was an act on a subtree, so
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

## Related reading

- [Archiving](../explanation/archiving.md) — why archiving is not a permission
  boundary
- [Capabilities and access](../explanation/capabilities-and-access.md) — the
  capability vocabulary and the rule for a request to a different team
- [Repositories as execution scope](../explanation/repositories-as-execution-scope.md)
  — why the team decides the board and the repository is a link
- [Glossary](glossary.md)
- [REST API](rest-api.md)

## Related guides

- [Connect over MCP](../how-to/connect-over-mcp.md)
- [Give an agent machine access](../how-to/give-an-agent-machine-access.md)
- [Find archived work](../how-to/find-archived-work.md)
