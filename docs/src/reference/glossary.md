# Glossary

Terms as Kairos uses them, alphabetically. Where a term means different things
on different surfaces, every name is given along with the surface that uses it.

Three collisions account for most confusion, and each has a full entry below:
**archived** names two unrelated states; **board** names three different kinds
of board; **delete** and **archive** name the same act.

## ADR

Architecture Decision Record. One of the five work-item types, letter `A` in a
short code. An ADR normally sits on an ADR board; an organization admin may also create
one off-board, with no board and no column.

This book names the ADRs of the Kairos project by short code. An ADR with the
prefix `KAIROS-A-` is a file in the archive of the repository. The book links
to that file. An ADR with the prefix `COLLIERY-A-` is in the Kairos deployment
where the project tracks its work. It has no file in the repository, so the
book gives its short code with no link.

Where a page needs the reason of such a decision, the page gives the reason in
its own words.

## archived

Two unrelated states share this word.

**Sense 1 — the put-away state.** Any of the five work-item types can be put
away: hidden from default listings, still readable, still searchable on
request. This is what a delete does. It is a visibility default and nothing
more: it is not a permission boundary, and whoever could read the item before
can read it after. The three surfaces name it differently:

| Surface | Name |
|---|---|
| GUI | "put away". A put-away item shows a gold `put away` pill and is read-only while it is put away. |
| REST API, MCP tools, CLI `--json` | `archived_at`, the instant it was put away, on the item; `include_deleted` on a listing or search to ask for these items; `[archived]` as the marker on a rendered row. |
| Database | `deleted_at`. |

**Sense 2 — a document's editorial lifecycle value.** A document carries a
lifecycle label of `draft`, `review`, `published` or `archived`. The
transitions are free — any value to any value — and setting one is not a
content edit: no version bump, no history row.

**The two are independent.** A published document can be editorially archived
and perfectly live; a draft document can be put away. Nothing about one implies
the other. Only the first sense hides an item from listings, and only the first
is what `include_deleted` and `archived_at` refer to.

An archive is an edit, so the [edit rule](#edit-rule) decides who can put an
item away. An archive of a parent also puts away the descendants that the
caller can edit: see [cascade](#cascade).

See [Archiving](../explanation/archiving.md) for why the state is defined this
way.

## board

Four board **levels** exist — `strategy`, `initiative`, `delivery`, `adr` —
grouping into three kinds of board.

- **Flight-level boards** — a strategy board and an initiative board hold
  Flight Level 3 and Flight Level 2 work. A tenant is provisioned with one of
  each.
- **A team's delivery board** — Flight Level 1. Every team gets exactly one
  when it is created, slugged `{team-slug}-delivery`, and it is the only kind
  of board that tasks live on. It is also the unit of authorization: board
  capabilities are granted per board.
- **The ADR board** — holds ADRs.

Every board has a team. The rule has two forms.

- A delivery board has a delivery [team](#team). The board holds the team in
  `team_id`. Kairos refuses to create a delivery board with no team. A live
  delivery board cannot lose its team.

  A team has only one delivery board. Kairos refuses to create a second
  delivery board for a team. Kairos refuses to delete the only delivery board
  of a team. The team of a board does not change.
- A board of the organization has the level `strategy`, `initiative` or `adr`.
  It has no delivery team and no `team_id`. Its team is the list of the members
  of the board. A person joins that team when an administrator adds the person
  to the board.
  Kairos refuses to create a board of the organization with a `team_id`.

Documents sit on no board at all. They inherit their parent item's board for
authorization purposes.

## bucket

An initiative marked as a standing container for work that arrives without a
plan, rather than as a planned piece of work. A bucket carries a bucket type:
`tech_debt`, `bug` or `ad_hoc`. An initiative is a bucket exactly when it has a
bucket type.

## capability

A named permission held on one board — the unit of write access. Capability
names are of the form `manage_tasks` or `configure_boards`, and a grant may use
a trailing glob to cover a family of them. Some capabilities are not granted at
all: team membership implies a set of them on that team's own delivery board,
and [`file_backlog`](#file_backlog) is computed and not stored.

Distinct from [role](#role). A role is organization-wide; a capability is per
board.

The vocabulary itself, the glob semantics, the team-implied set and the
org-admin bypass are not enumerated here — see
[Capabilities](capabilities.md) for the names and the matching rules, and
[Capabilities and access](../explanation/capabilities-and-access.md) for why
the model is shaped that way.

## cascade

What an archive does below the named item. The archive follows `parent` edges
from the named item to its descendants. It applies the [edit rule](#edit-rule)
to each descendant, for the caller:

- It archives a descendant that the caller can edit.
- It stops at a descendant that the caller cannot edit, and archives nothing
  below it.

The response names each descendant that stays, in `not_reached`. A descendant
that stays keeps its `parent` edge. `supports` edges are not part of the
cascade: an archive of an item does not archive its documents and ADRs. A
restore does not cascade. See
[Archiving](../explanation/archiving.md#the-cascade-takes-only-what-the-caller-can-edit).

## column

A named position on a board. Columns are ordered, and a board's transition
graph says which column-to-column moves are legal. A column can be removed,
which soft-deletes it rather than dropping it: put-away cards still point at it,
so its name remains readable as the answer to "which column was this in?" even
though it no longer appears on the board. A board can mark a column as a
[done column](#done-column).

## complexity

T-shirt sizing on an initiative: `xs`, `s`, `m`, `l`, `xl`.

## creator

The principal who created an item: a person or a service account. Each item
records its creator, and the creator does not change.

Creation is the primary mechanism of ownership. The creator can
[edit](#edit-rule) the item with no capability on its board. The creator keeps
that right when the item moves to a different board.

Creation grants no movement. The creator needs the capability of the board to
move the item between columns, to change its lane, or to move it to a
different board.

## delete

Always a soft delete, and the same act as archiving in sense 1 above: the item
is stamped, hidden from default listings, and remains readable and restorable.
It [cascades](#cascade) through `parent` edges to each descendant that the
caller can edit. `restore` reverses it for one item; archived descendants stay
archived.

Nothing in the product hard-deletes a work item. The one genuinely destructive
operation is dropping a tenant.

## delivery board

A board of the level `delivery`. Tasks are on delivery boards only. A delivery
board always has a team, and a task has the team of its board. Kairos refuses
to create a delivery board with no team, and a live delivery board cannot lose
its team. A team has exactly one delivery board.

See [board](#board).

## delivery stream

A grouping of teams, with a name, slug and description. Membership is
many-to-many: a team may belong to several streams and a stream holds several
teams. Streams carry no boards and no work of their own.

## document

One of the five work-item types, letter `D`. A document supports a strategy,
an initiative or a task, through a `supports` edge that is required at
creation. That item is the parent of the document. Documents have no board
placement and therefore no column and no
transitions; instead they carry an editorial lifecycle. A document may be
stamped from a template.

A document always has a parent. It can have more than one. The board of the
earliest parent is the authorization board of the document. The server refuses
to remove the last `supports` edge of a document (`LAST_PARENT`).

## done column

A column that the board marks as done. It is a terminal column: work in it is
complete. The mark is a flag on the column (`is_done`), and the board sets it.
The name of the column does not decide it. The default boards set the flag on
Completed. The default ADR board sets it on Decided and on Superseded.

Two results follow from the flag:

- The children-progress rollup counts a child in a done column as done.
- A `blocks` edge does not count when the item at either end is in a done
  column. See [relationship](#relationship).

## editorial lifecycle

See sense 2 of [archived](#archived).

## edit rule

The rule that decides who can edit an item. A principal can edit an item when
one of these conditions is true:

- The principal is the [creator](#creator) of the item.
- The principal holds `manage_<type>` on the authorization board of the item.
- The principal is an organization admin.

An edit is a change of the title, the content, the metadata, the repository of
a task, or the editorial lifecycle of a document. An archive and a restore are
edits also. A move is not an edit. See [Capabilities](capabilities.md#the-edit-rule).

## entry column

A board's first column by position, among its live columns. A new item goes
there by default. A task that moves to a different delivery board goes there. A
[request](#request) goes there.

## file_backlog

The computed capability that permits a [request](#request). Every member of
the organization holds it on every live delivery board. The server consults it
only when the caller creates a task and does not hold `manage_tasks` on the
board. It applies when all of these conditions are true:

- The board is a delivery board.
- The column is the entry column of the board. No column in the call means the
  entry column.
- The caller did not send the work class `planned`.

The repository is not part of the condition. The capability permits the create
only. The [edit rule](#edit-rule) lets the [creator](#creator) of the request
edit it and archive it. See [Capabilities](capabilities.md).

## flight level

The three altitudes of work, from Flight Levels methodology:

| Level | Item type | Short-code letter |
|---|---|---|
| Flight Level 3 | strategy | `S` |
| Flight Level 2 | initiative | `I` |
| Flight Level 1 | task | `T` |

Documents (`D`) and ADRs (`A`) carry short codes but are not flight levels.

See [Flight levels](../explanation/flight-levels.md).

## initiative

One of the five work-item types, letter `I`. Flight Level 2. May carry a
complexity and may be a bucket.

## item

Any of the five content-bearing types — strategy, initiative, task, document,
ADR. All five carry a short code, a version, markdown content and a version
history. Also called a *work item*.

## link rule

The rule that decides who can write a [relationship](#relationship). A
principal can create or remove an edge when the principal can edit the item at
one end. The [edit rule](#edit-rule) decides each end. The rule is the same
for each relationship type. The confirm of an edge proposal writes an edge, so
the rule applies to the confirm also. See
[Capabilities](capabilities.md#who-can-write-relationships).

The `supports` edge of a document is the one exception: the principal must be
able to edit the document.

## metadata definition

A tenant-scoped custom field: a slug, a type of `string`, `enum` or `date`, and
for an enum its permitted values. Items carry values keyed by definition slug;
templates can collect a set of definitions. A definition cannot be deleted while
any item value or template association references it — **and an archived carrier
still counts**. The refusal names the carriers and marks the archived ones,
which is what keeps that honest: an archived item is still readable by short
code, and restoring it is what makes its value clearable.

## organization

The tenant. One organization per tenant, holding its own schema, boards, teams
and work. Every short code's prefix defaults to the organization slug,
upper-cased and reduced to `A-Z0-9`.

## put away

See sense 1 of [archived](#archived).

## relationship

A directed edge between two items. Five types exist:

| Type | Meaning |
|---|---|
| `parent` | Hierarchy. A delete [cascades](#cascade) along these edges, and they are what a traversal follows to build a parent chain. |
| `supports` | A document or an ADR supporting the item it is attached to. The source is the item, and the target is the document. For a document, the edge has [rules of its own](capabilities.md#the-supports-edge-of-a-document). |
| `informs` | One item informs another without owning it. |
| `supersedes` | One item replaces another. |
| `blocks` | The source blocks the target. |

Which types are legal between which item types is enforced, as is cycle
prevention. The [link rule](#link-rule) decides who can write an edge, and it
is the same for the five types. The `supports` edge of a document is the one
exception.

**Complete work does not block, and nothing blocks complete work.** A `blocks`
edge counts only while the items at both ends can move. The edge stops counting
when:

- the item at either end is in a [done column](#done-column), or
- the item at the other end is [put away](#put-away).

The edge stays in the relationship list of each item, as history. The list
marks the end in a done column (`done` in REST, `[done]` in MCP). It marks the
end that is put away (`archived_at` in REST, `[archived]` in MCP). The counts on
a board card ("blocked by", "blocks") include only the edges that count. Kairos
stores nothing for this rule. When an item moves out of a done column, its
edges count again.

The graph view shows an edge that does not count as a dashed arrow. The legend
of the graph view names the two styles.

## repository

A codebase registered in Kairos, with a forge (`github`, `gitlab`, `other`), an
`owner/repo` full name matching what the forge sends in webhooks, a browser
URL, a default branch, a slug, and a free-text description of how to work in
it.

A repository has exactly one owning team. The owning team has three duties:
the review of code, the release, and the standards of the repository.

A task can link to one repository, which makes the repository an **execution
scope**: the link says where the code is. The link is optional. It does not
choose the board or the team of the task, and a task on the board of any team
can link to any live repository. A null, empty or blank repository clears the
link.

See [Repositories as execution scope](../explanation/repositories-as-execution-scope.md).

## request

A task that a person creates on the delivery board of a team, when the person
does not hold `manage_tasks` on that board. A request goes to the
[entry column](#entry-column). Its [work class](#work-class) is `support`. A
repository is optional. The person who sent the request is its
[creator](#creator). That person can edit it, link it and archive it, and
cannot move it. A member of the team that receives it can change the work
class to `planned`. The capability is [`file_backlog`](#file_backlog).

## role

An organization-wide role: `admin` or `member`. An organization must retain at
least one admin — demoting or removing the last one is refused with
`LAST_ADMIN`. Separate from a **deployment admin**, which is not a role in any
organization but a list of OIDC subjects in the server's configuration, and
which only grants access to the cross-tenant tenant-provisioning routes.

Distinct from [capability](#capability).

## service account

A machine principal, authenticated by an API key rather than by OIDC. It holds
board capabilities exactly as a user does. Its keys are shown once at creation
and afterwards only by prefix.

## short code

An item's stable, human-usable identifier, formatted
`{PREFIX}-{LETTER}-{NNNN}` — for example `ACME-T-0012`. The prefix defaults
from the organization slug, the letter is the item type, and the number comes
from a per-type sequence. Short codes identify items in every API, every CLI
command and every MCP tool.

## strategy

One of the five work-item types, letter `S`. Flight Level 3. May carry a
hypothesis.

## task

One of the five work-item types, letter `T`. Flight Level 1. The only type that
sits on a team delivery board. A task has the team of its board. It carries a
task type and a work class, and it can link to one
[repository](#repository).

## task type

What kind of ticket a task is: `task`, `bug`, `tech_debt`, `support`. Distinct
from [work class](#work-class), which is the lane it is counted in.

## team

A group of people with exactly one delivery board and, optionally, membership
of one or more delivery streams. A team carries a **team type** from Team
Topologies:

| Type | Wire value |
|---|---|
| Stream-aligned | `stream_aligned` |
| Platform | `platform` |
| Enabling | `enabling` |
| Complicated subsystem | `complicated_subsystem` |

`stream_aligned` is the default. The type is descriptive: it does not change
permissions or the board of a task.

Every board has a team, but only a delivery board has a team of this kind. The
team of a board of the organization is the list of the members of that board.
That list is not a record in the team directory, and it has no team page.
See [board](#board).

See [Teams and boards](../explanation/teams-and-boards.md).

## template

A reusable starting point for a document: content to stamp, plus a set of
metadata definitions to collect.

## tenant

See [organization](#organization). A deployment resolves which tenant a request
belongs to from the host subdomain, from a fixed single-tenant setting, or from
the `X-Tenant` header.

## transition

Moving an item from one column to another on its own board, subject to the
board's transition graph. Distinct from a **move**, which sends a task to a
different delivery board entirely and lands it in that board's entry column.

## version

An item's content revision counter. Content edits bump it and write a history
snapshot; edits are optimistically concurrent, so an edit based on a stale
version is refused with `CONFLICT` carrying the current version. Setting a
document's editorial lifecycle is not a content edit and does not bump it.

## work class

Which lane a task is counted in: `planned` or `support`. Defaults to `support`
for a task whose task type is `support`, and to `planned` otherwise. A
[request](#request) is always `support`. Distinct
from [task type](#task-type): a `bug` may be planned work, and a task of type
`task` may be support work.

## work item

See [item](#item).

## Related reading

- [Capabilities](capabilities.md) — the capability vocabulary and glob rules
- [Errors](errors.md) — every refusal code these terms appear in
- [Flight levels](../explanation/flight-levels.md),
  [Teams and boards](../explanation/teams-and-boards.md),
  [Archiving](../explanation/archiving.md) — why the things named here are
  shaped as they are
