# MCP tools

Kairos serves the Model Context Protocol at `/mcp`. The surface is exactly
eighteen tools, frozen by name and shape; a drift gate in the test suite
asserts that `tools/list` returns these eighteen and no others.

This page describes Kairos 0.1.0. Argument names, types and defaults are those
of the JSON schema the server sends in `tools/list`.

## Conventions

These hold for every tool.

| Aspect | Rule |
|---|---|
| Tenant | There is no tenant argument. The tenant is the one resolved from the connection's host or the `X-Tenant` header. |
| Identity | Every tool runs as the authenticated principal — a human user or a service account — under the same attribute-based access control as the REST handlers. Archiving is not a permission boundary: whoever could read an item before it was archived can read it after. |
| Item identity | Items are named by short code, e.g. `ACME-T-0012`, in every input and every output. |
| Board and team references | A `board`, `to_board`, `team` or `repository` argument accepts either a slug or a UUID. A `column` or `to_column` argument accepts either a column name, case-insensitively, or a UUID. |
| Listing weight | Listings are compact: short code, title and key fields. Full markdown content arrives only from `get_item` and from `get_history` with a `version`. |
| Errors | A refusal comes back as an MCP tool error whose text carries the same stable code as the REST API's error envelope. |
| Audit | Writes are recorded in the activity log by the same code path as the REST API. |

### Refusal codes

Codes an agent can receive, and what each means.

| Code | Meaning |
|---|---|
| `NOT_FOUND` | A short code, board or repository named as the subject of the call does not exist; an `unlink_items` edge does not exist; or a `search` `traverse.from` does not resolve. On a write tool it also means the item exists but is archived: writes resolve live items only, and the message reads `no live item with short code …`. |
| `VALIDATION` | An argument is malformed, an enum value is outside its vocabulary, an argument does not apply to the item type, or something named as a *filter or reference* — a team, a repository filter, a parent, a metadata definition, a column — does not exist. A reference that does not resolve is `VALIDATION`; the call's own subject not existing is `NOT_FOUND`. **One exception:** `search`'s `traverse.from` is a reference and still answers `NOT_FOUND`, because a traversal's root is the subject of that traversal. |
| `FORBIDDEN` | The principal lacks the required board capability. The message names the capability. For a cross-team filer holding only the computed `file_backlog` capability, the message states the Backlog-only rule instead of the bare capability name. |
| `CONFLICT` | An optimistic-concurrency version mismatch. The refusal carries the current version and content. |
| `INVALID_TRANSITION` | The target column is not reachable from the item's current column in the board's transition graph. The refusal enumerates the allowed target columns. |
| `ITEM_NOT_ON_BOARD` | The item has no board placement, so it cannot be transitioned or moved. |
| `SAME_BOARD` | A `move_item` whose target is the board the task is already on. |
| `NOT_DELIVERY_BOARD` | A `move_item` whose target board is not a delivery board. |
| `NO_ENTRY_COLUMN` | The target delivery board has no entry column to land the task in. |
| `REPOSITORY_OWNER_MISMATCH` | The task is bound to a repository owned by a different team than the target board's. |
| `RESTORE_BLOCKED` | The archived item's board, column, owning team or repository no longer exists. The refusal names what is missing. |
| `RELATIONSHIP_RULE` | The relationship type is not allowed between those two item types. |
| `CYCLE_DETECTED` | The edge would create a cycle. |
| `ALREADY_LINKED` | That edge already exists. |

`DEFINITION_IN_USE`, the refusal that protects a metadata definition carrying
values, belongs to the REST surface — `DELETE /api/metadata-definitions/{id}`.
No MCP tool deletes a definition, so no MCP tool returns it. See
[Tenant configuration](rest/tenant-configuration.md).

## Orientation

### `whoami`

Identity, organization role, teams, those teams' repositories, the boards where
the caller holds write capabilities, and the capabilities every member holds
implicitly.

No arguments.

Refuses: nothing beyond transport-level authentication.

### `my_boards`

Boards in the organization, grouped by level, with column names and per-column
item counts for the caller's delivery boards.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `level` | string | no | all levels | One of `strategy`, `initiative`, `delivery`, `adr`. |

Refuses: `VALIDATION` for a `level` outside that vocabulary.

### `list_repositories`

The repository directory. Per repository: slug, forge, full name, the single
owning team, the delivery board tasks filed against it land on, the open task
count, and whether webhooks are connected.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `team` | string | no | all teams | Narrow to one team's repositories. Slug or UUID. |

Refuses: `VALIDATION` for an unknown `team` — it is a filter value, not a
path.

### `get_repository`

One repository in full: owning team, delivery board, default branch, the team's
description of how to work in it, and its in-flight branches and pull requests
with the work items they belong to.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `repository` | string | yes | — | Slug or UUID. |

Refuses: `NOT_FOUND` for an unknown repository.

## Reading

### `board_items`

The items on a board, grouped by column: short code, type and title.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `board` | string | yes | — | Slug or UUID. |
| `column` | string | no | all columns | Column name or UUID. |
| `repository` | string | no | all | Narrow the tasks to those issued against this repository. Slug or UUID. |
| `include_deleted` | boolean | no | `false` | Add archived cards back, each marked `[archived]`, in the column they were put away in. Columns that have since been removed appear only when this is true, and only carrying archived cards. |

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

Archived items are returned, marked with the instant they were put away. The
column reported for an archived card is the name of the column it was put away
in, which may since have been removed from the board: a removed column is
soft-deleted rather than dropped, and this lookup deliberately ignores that so
"which column was this in?" stays answerable.

Refuses: `NOT_FOUND` only when the short code names nothing at all.

### `get_history`

An item's content version history — version, editor, timestamp, newest first.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The item's short code. |
| `limit` | integer | no | `20` | Maximum versions to list. Clamped to 1–200. |
| `version` | integer | no | — | Return that snapshot's full title and content instead of the list. |

Archived items' history is returned, marked archived.

Refuses: `NOT_FOUND` for an unknown short code, or for a `version` with no
snapshot.

## Searching

### `search`

Full-text query, structured filter and graph traversal, composing freely. At
least one of `q`, a constraining `filter`, or `traverse` is required. Results
are compact and grouped by type.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `q` | string | no | — | Full-text query. Websearch syntax: quoted phrases, `OR`, `-negation`. Must not be blank. |
| `filter` | object | no | — | See below. Fields AND together. |
| `traverse` | object | no | — | See below. |
| `sort` | object | no | `created_at` descending | See below. |
| `limit` | integer | no | `25` | Page size, 1–100. |
| `offset` | integer | no | `0` | Offset into the combined result set. Must not be negative. |

`filter`:

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `entity_type` | array of string | no | all | `strategy`, `initiative`, `task`, `document`, `adr`. Must not be an empty array. |
| `board_id` | UUID string | no | — | Items on this board. |
| `column_id` | UUID string | no | — | Items in this column. |
| `team_id` | UUID string | no | — | Tasks of this team. |
| `repository` | string | no | — | Tasks issued against this repository. Slug or UUID. |
| `task_type` | array of string | no | all | `task`, `bug`, `tech_debt`, `support`. Must not be an empty array. |
| `work_class` | array of string | no | all | `planned`, `support`. Must not be an empty array. |
| `is_bucket` | boolean | no | both | Bucket or non-bucket initiatives. |
| `metadata` | object of string to string | no | — | Conditions keyed by metadata-definition slug. Values allow a trailing `*` glob. Keys must not be blank. |
| `created_after` | RFC 3339 string | no | — | Items created strictly after. |
| `created_before` | RFC 3339 string | no | — | Items created strictly before. Must be later than `created_after`. |
| `include_deleted` | boolean | no | `false` | Include archived items, marked `[archived]`. Composes with everything, `q` and `traverse` included, and counts on its own as a constraining filter. |

`traverse`:

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `from` | string | yes | — | The starting item's short code. |
| `relationships` | array of string | yes | — | `parent`, `supports`, `informs`, `supersedes`, `blocks`. Must not be empty. |
| `direction` | string | yes | — | `outbound`, `inbound`, `both`. |
| `depth` | integer | no in the schema | — | 1–10. Optional in the schema, but a traversal without it is refused: the field is deliberately not defaulted so that a missing depth is a typed refusal rather than a silent choice. |

`sort`:

| Field | Type | Required | Description |
|---|---|---|---|
| `field` | string | yes | `created_at`, `updated_at`, `title`. |
| `order` | string | yes | `asc`, `desc`. |

Refuses: `VALIDATION` for a blank `q`, an empty enum array, a blank metadata
key, an inverted date range, a `limit` outside 1–100, a negative `offset`, a
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
| `board` | string | no | see below | Target board, slug or UUID. Ignored for documents. |
| `parent` | string | no | — | Parent item's short code. Creates the `parent` edge, or the `supports` edge for documents, where it is required. |
| `content` | string | no | empty, or the template's | Initial markdown content. |
| `template` | string | no | — | Documents only. Template id, slug or name. |
| `task_type` | string | no | `task` | Tasks only. `task`, `bug`, `tech_debt`, `support`. |
| `repository` | string | no | — | Tasks only. Slug or UUID. Routes the task to the repository's owning team's delivery board, which makes `board` optional and, when `board` is also given, requires the two to agree. |
| `work_class` | string | no | `support` for support-type tasks, otherwise `planned` | Tasks only. `planned`, `support`. |
| `hypothesis` | string | no | — | Strategies only. |
| `complexity` | string | no | — | Initiatives only. `xs`, `s`, `m`, `l`, `xl`. |
| `decision_maker` | string | no | — | ADRs only. |

`board` may be omitted when the tenant has exactly one live board of the
matching level. Documents take no board: they inherit their parent's board for
authorization.

Any member may create a task against another team's repository. It lands in
that board's Backlog behind the owning team's triage, through the computed
`file_backlog` capability.

`create_item` has no column argument: a new item always lands in its board's
first column. It also has no `bucket_type` and no `decision_date`, both of which
the CLI's `create` verbs accept — an initiative created over MCP is never a
bucket, and an ADR created over MCP carries no decision date. Both fields are
readable through `get_item` and settable through the REST API.

Refuses: `VALIDATION` for an unknown `item_type`; for a type-specific argument
passed with the wrong `item_type`, naming the type it belongs to; for a document
without `parent`, or whose parent is not a strategy, initiative or task; for a
`parent` that does not name a live item; for an unknown template; when no live
board of the required level exists; when several do, listing their slugs; and
for an unknown `repository`. `NOT_FOUND` for an unknown `board`. `FORBIDDEN` when the caller
lacks `manage_<type>` on the resolved board, or lacks the capability to write
the requested `parent` edge — the edge is gated before the item is written, so
a refusal leaves no orphan.

### `update_item`

Replaces an item's full content, and optionally its title, under optimistic
concurrency.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The item's short code. |
| `content` | string | yes | — | The full replacement markdown content, not a patch. |
| `version` | integer | yes | — | The version this edit is based on, as read from `get_item`. |
| `title` | string | no | unchanged | New title. |

Refuses: `NOT_FOUND` for an unknown short code or an archived item;
`FORBIDDEN` without `manage_<type>` on the item's authorization board;
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
`update_item`.

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
`NOT_FOUND` for an unknown short code or an archived item. `FORBIDDEN` without
`manage_<type>` on the item's authorization board.

## Moving work

### `transition_item`

Moves an item to another column on its own board.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The item's short code. |
| `to_column` | string | yes | — | Target column, name or UUID, on the item's own board. |

Requires `transition_items` on the item's board.

Refuses: `NOT_FOUND` for an unknown short code or an archived item;
`ITEM_NOT_ON_BOARD` for an item with no placement — documents always;
`VALIDATION` for a column that is not on that board, listing the board's
columns; `FORBIDDEN` without `transition_items`; `INVALID_TRANSITION` for a
target outside the board's transition graph, enumerating the allowed targets.

### `move_item`

Moves a task to another delivery board. It lands in that board's entry column
and follows that board's team.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The task's short code. |
| `to_board` | string | yes | — | Target delivery board, slug or UUID. |

Requires `manage_tasks` on both the task's current board and the target.

Refuses: `NOT_FOUND` for an unknown short code, an archived task, or an unknown
or archived `to_board`; `VALIDATION` when the item is not a task;
`ITEM_NOT_ON_BOARD` when the task has no placement; `FORBIDDEN` without
`manage_tasks` on either side; `SAME_BOARD`; `NOT_DELIVERY_BOARD`;
`NO_ENTRY_COLUMN`; `REPOSITORY_OWNER_MISMATCH` when the task is bound to a
repository owned by another team — the binding must be cleared first.

Column-to-column moves on an item's own board are `transition_item`, not this
tool.

## Relationships

### `link_items`

Creates a relationship edge between two items.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `source` | string | yes | — | Source item's short code. The edge runs source to target. |
| `target` | string | yes | — | Target item's short code. |
| `relationship` | string | yes | — | `parent`, `supports`, `informs`, `supersedes`, `blocks`. |

`parent` and `blocks` may be written by anyone who manages either item's board,
or who created the source item — so a task filed against another team's
repository can block the filer's own item. The other three types are org-admin
only.

Refuses: `VALIDATION` for a `relationship` outside the vocabulary, for a
`source` or `target` that does not name a live item, and for a self-link where
`source` and `target` are the same item; `FORBIDDEN` when the edge rule's gate
is not met; `RELATIONSHIP_RULE` when that relationship is not allowed between
those two item types; `CYCLE_DETECTED`; `ALREADY_LINKED`.

### `unlink_items`

Removes a relationship edge. Arguments and gating are identical to
`link_items`.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `source` | string | yes | — | Source item's short code. |
| `target` | string | yes | — | Target item's short code. |
| `relationship` | string | yes | — | `parent`, `supports`, `informs`, `supersedes`, `blocks`. |

Refuses: as `link_items`, except that a `relationship` with no such edge
between those items is `NOT_FOUND`.

## Archiving

### `delete_item`

Soft-deletes an item. The response lists everything that was cascade-deleted.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The item's short code. |
| `confirm` | boolean | yes | — | Must be `true`. |

The delete cascades to every descendant reachable through `parent` edges.
Deleted items remain retrievable by short code and searchable with
`include_deleted`; they are hidden from default listings. "Deleted" and
"archived" name the same act — see the [Glossary](glossary.md).

Refuses: `VALIDATION` when `confirm` is `false`; `NOT_FOUND` for an unknown
short code or an already-archived item; `FORBIDDEN` without `manage_<type>` on
the item's authorization board. An *absent* `confirm` is a schema violation
rather than a refusal — it is a required field, so the call is rejected before
the tool body runs and carries no Kairos error code.

### `restore_item`

Puts an archived item back on its board.

| Argument | Type | Required | Default | Description |
|---|---|---|---|---|
| `short_code` | string | yes | — | The archived item's short code. |

Restores only the named item. A cascade delete was an act on a subtree, so
archived descendants stay archived; the response names them.

Refuses: `NOT_FOUND` for an unknown short code; `VALIDATION` when the item is
not archived; `FORBIDDEN` without `manage_<type>` on the item's authorization
board; `RESTORE_BLOCKED` when the item's board, column, owning team or
repository has since been removed, naming what is missing.

## Related reading

- [Archiving](../explanation/archiving.md) — why archiving is not a permission
  boundary
- [Capabilities and access](../explanation/capabilities-and-access.md) — the
  capability vocabulary and the cross-team filing rule
- [Glossary](glossary.md)
- [REST API](rest-api.md)

## Related guides

- [Connect over MCP](../how-to/connect-over-mcp.md)
- [Give an agent machine access](../how-to/give-an-agent-machine-access.md)
- [Find archived work](../how-to/find-archived-work.md)
