# Glossary

Terms as Kairos uses them, alphabetically. Where a term means different things
on different surfaces, every name is given along with the surface that uses it.

Three collisions account for most confusion, and each has a full entry below:
**archived** names two unrelated states; **board** names three different kinds
of board; **delete** and **archive** name the same act.

## ADR

Architecture Decision Record. One of the five work-item types, letter `A` in a
short code. An ADR normally sits on an ADR board; an org admin may also create
one off-board, with no board and no column.

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
and `file_backlog` is computed from the request rather than stored.

Distinct from [role](#role). A role is organization-wide; a capability is per
board.

The vocabulary itself, the glob semantics, the team-implied set and the
org-admin bypass are not enumerated here — see
[Capabilities](capabilities.md) for the names and the matching rules, and
[Capabilities and access](../explanation/capabilities-and-access.md) for why
the model is shaped that way.

## column

A named position on a board. Columns are ordered, and a board's transition
graph says which column-to-column moves are legal. A column can be removed,
which soft-deletes it rather than dropping it: put-away cards still point at it,
so its name remains readable as the answer to "which column was this in?" even
though it no longer appears on the board.

## complexity

T-shirt sizing on an initiative: `xs`, `s`, `m`, `l`, `xl`.

## delete

Always a soft delete, and the same act as archiving in sense 1 above: the item
is stamped, hidden from default listings, and remains readable and restorable.
It cascades to every descendant reachable through `parent` edges. `restore`
reverses it for one item; archived descendants stay archived.

Nothing in the product hard-deletes a work item. The one genuinely destructive
operation is dropping a tenant.

## delivery board

See [board](#board).

## delivery stream

A grouping of teams, with a name, slug and description. Membership is
many-to-many: a team may belong to several streams and a stream holds several
teams. Streams carry no boards and no work of their own.

## document

One of the five work-item types, letter `D`. A document supports exactly one
strategy, initiative or task, through a `supports` edge that is required at
creation. Documents have no board placement and therefore no column and no
transitions; instead they carry an editorial lifecycle. A document may be
stamped from a template.

## editorial lifecycle

See sense 2 of [archived](#archived).

## entry column

A board's first column by position, among its live columns. It is where a task
lands when it is moved to another delivery board, and where a cross-team filer
may file.

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
| `parent` | Hierarchy. A delete cascades along these edges, and they are what a traversal follows to build a parent chain. |
| `supports` | A document supporting the item it is attached to. |
| `informs` | One item informs another without owning it. |
| `supersedes` | One item replaces another. |
| `blocks` | The source blocks the target. |

Which types are legal between which item types is enforced, as is cycle
prevention. `parent` and `blocks` may be written by anyone who manages either
item's board or who created the source item; the other three are org-admin
only.

## repository

A codebase registered in Kairos, with a forge (`github`, `gitlab`, `other`), an
`owner/repo` full name matching what the forge sends in webhooks, a browser
URL, a default branch, a slug, and a free-text description of how to work in
it.

A repository has exactly one owning team, which makes it an **execution
scope**: a task issued against a repository is routed to that team's delivery
board, and it may afterwards be moved only to that team's board unless the
binding is cleared first. Any tenant member may file a task against another
team's repository; it lands in that board's entry column for the owning team to
triage.

See [Repositories as execution scope](../explanation/repositories-as-execution-scope.md).

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
sits on a team delivery board. Carries a task type and a work class, and may be
bound to a repository.

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
permissions or routing.

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
for a task whose task type is `support`, and to `planned` otherwise. Distinct
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
