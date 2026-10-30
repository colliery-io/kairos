---
title: "Glossary"
---

Terms as Kairos uses them, alphabetically. Where a term means different things
on different surfaces, every name is given along with the surface that uses it.

Three collisions account for most confusion, and each has a full entry below:
**archived** names two unrelated states; **board** names three different kinds
of board; **delete** and **archive** name the same act.

## activity log

The record of the changes in an organization. Each entry has an actor, an
action, a time, and details. An entry can name an entity: an item, a board, a
team, a delivery stream, a membership or a repository. `GET /api/activity` and
the Activity page of the GUI show the entries. The newest entry is first.

An agent can make a change with the agent key of a person. Then the actor is
the person, and the entry also has the key (`agent_key_id`). The GUI shows the
actor as "Alice (agent)". The versions of the history of an item have the same
mark.

An entry has one of these actions:

| Action | The change |
|---|---|
| `create` | A person made an item, a board, a team, a delivery stream, a team page, a membership, an API key or a SCIM token. |
| `update` | A person changed a team, a delivery stream, the role of a member, or the profile of a user. A person changed the owner board of a document. |
| `delete` | A person deleted or archived one of them. |
| `restore` | A person restored an archived item. |
| `transition` | An item moved to a different column of its board. |
| `board_move` | A task moved to a different board. |
| `claim` | A person moved a task to Active and got its [claim](#claim). |
| `hand_off` | A person gave the claim of a task to a different person. |
| `release` | The claim of a task ended. The details give the reason: a release, a move out of Active, an archive, a cancel, a service account, or a person who left (`left`). |
| `cancel` | A person [cancelled](#cancel) a task. The details give the columns and the reason. |
| `purge` | A person deleted a task for good (a [purge](#purge)). The details give the code and the title. |
| `board_config` | A person changed the name, the slug, a column or a transition of a board. |
| `work_class` | A task changed its lane. |
| `lifecycle` | A document changed its [editorial lifecycle](#editorial-lifecycle). |
| `repository` | A person added, changed or deleted a repository, or changed the repository of a task. |
| `relationship_add` | A person added an edge between two items, or an [`impacts`](#impacts) link. |
| `relationship_remove` | A person removed an edge between two items, or an `impacts` link. |
| `capability_grant` | A person gave a capability on a board to a user. |
| `capability_revoke` | A person removed a capability on a board from a user. |

An entry about an edge between two items names no entity. Its details have the
two short codes. An entry about an `impacts` link names the document or the
ADR as its entity.

Before COLLIERY-T-0265, a change to a team, a delivery stream, a role or a user
had the action `create`. Kairos does not change old entries.

The filters of the feed are the entity, the actor, the action, the start time
and the team. You can use 2 or more filters at the same time. The server
applies the filters, and `total` is the count after the filters.

The filter by team (`team`, a slug or an id) shows the entries by the members
of the team. The members are those that the team has at the time of the
request. The entity of the entry has no effect on the filter.

- The filter shows the work of a member on the board of a different team.
- The filter does not show the work of other persons on the board of the team.

To see the entries about one item, use the filter by entity.

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
request. This is what an archive does (the MCP tool `delete_item`, REST
`DELETE`, the GUI button Archive). It is a visibility default and nothing
more: it is not a permission boundary, and whoever could read the item before
can read it after. The three surfaces name it differently:

| Surface | Name |
|---|---|
| GUI | The button is Archive, and its confirmation says that you can restore the item. A put-away item shows a gold `put away` pill and is read-only while it is put away. |
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

See [Archiving](../../explanation/archiving/) for why the state is defined this
way.

## authorization board

The board that the server reads to decide who can write an item. A principal
with `manage_<type>` on that board can edit the item.

| Item | Authorization board |
|---|---|
| Strategy, initiative, task, ADR on a board | The board of the item |
| Document | Its [owner board](#owner-board) |

An ADR that is on no board has no authorization board. See
[Capabilities](../capabilities/#how-a-board-is-resolved).

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

A board has a slug, and the GUI, the MCP tools and the CLI find a board by its
slug. Two live boards cannot have the same slug. Kairos refuses the create and
the update of a board with the slug of a live board. The refusal is 409
`CONFLICT`, and it names the board that has the slug. A deleted board does not
keep its slug.

A slug that a caller sends must agree with the rule of a [slug](#slug).

Every board has a team. The rule has two forms.

- A delivery board has a delivery [team](#team). The board holds the team in
  `team_id`. Kairos refuses to create a delivery board with no team. A live
  delivery board cannot lose its team.

  A team has only one delivery board. Kairos refuses to create a second
  delivery board for a team. Kairos refuses to delete the only delivery board
  of a team. The team of a board does not change.
- An ADR board can have a team: the ADR board of the team, for the delivery
  ADRs of the team. It has the prefix of the team. A team has only one ADR
  board. When the team is deleted, its ADR board is deleted too.
- A strategy or initiative board has a team (KAIROS-T-0440): the team in the
  create request, or its own empty team, `<level>-<board slug>`. The members of
  that team can edit and move the items of the board. Each person with a grant
  on the board can do the same.
- An ADR board with no team is the ADR board of the organization. Its team is
  the list of the members of the board.

Documents sit on no board at all. A document has an owner, and the owner is a
board: see [owner board](#owner-board).

## board policy

A rule of a board that an admin with `configure_boards` turns on or off
(KAIROS-T-0641). Each policy is a field of the board. `PATCH /api/boards/{id}`,
the MCP tool `set_board_policy` and `kairos boards policy` set it. `my_boards`
shows it.

- **`parent_gate`**, a policy of a delivery board. With it on, a task moves
  into a claims column only while its parent initiative is active. An
  initiative is active in Active or Monitoring. A [bucket](#bucket) is always
  active. A task with no parent initiative moves freely. The refusal is
  `PARENT_NOT_ACTIVE`. The policy is on by default.
- **`owner_gate`**, a policy of an initiative board (KAIROS-T-0643). With it
  on, an initiative moves into Active only with an [owner](#owner). The owner
  of an initiative in Active or Monitoring cannot release it. A hand-off to a
  member of the board can change the owner. The refusal is `OWNER_REQUIRED`.
  The policy is on by default.
- **`blockers_gate`**, a policy of a delivery board (KAIROS-T-0565). With it
  on, a task with open blockers does not move into a claims column or a done
  column. Kairos counts a cancelled blocker as open. The refusal is
  `BLOCKED`. With it off, the move gets a warning that names each blocker.
  The policy is off by default.

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
[Capabilities](../capabilities/) for the names and the matching rules, and
[Capabilities and access](../../explanation/capabilities-and-access/) for why
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
[Archiving](../../explanation/archiving/#the-cascade-takes-only-what-the-caller-can-edit).

## cancel

A task that the team will not do (KAIROS-T-0362). A cancel needs a reason. The
task moves to the [done column](#done-column) of its board, and it gets the
mark "cancelled" with the reason. The board shows the chip "Cancelled".

The
transition rules of the board do not apply. You can cancel a task from each
column that is not done. A cancel ends the [claim](#claim).

A [reopen](#reopen) of the task removes the mark, and so does a transition out
of the done column. The activity log keeps the row `cancel` with the reason. "Archive completed" archives a
cancelled task, as it archives a finished task. To cancel a task, a principal needs
`transition_items` on the board: the capability of a move.

## claim

The person who works on a task in Active (KAIROS-T-0359). A column can have
the flag `claims`. The Active column of a delivery board has it. The person who
moves a task with no claim into such a column gets the claim. A person can also
take a free task in the column with a claim (`claim_item`, `kairos tasks claim`).

A column can also have the flag `keeps_claims`: the Blocked column has it
(KAIROS-T-0529). A task keeps its claim there, and a task with no claim gets none.

An agent can move the task with the agent key of a person. Then the claim names
the person, and the GUI shows "Alice (agent)". A service account gets no claim.

A hand-off gives the claim to a person who holds `transition_items` on the
board of the task (KAIROS-T-0530). A person who cannot move the task cannot
finish it.

The claim ends when the task leaves the columns that hold or keep claims. It
also ends at a hand-off, at a release, and at an archive. It ends when its
person leaves the organization (KAIROS-T-0531). Then the task stays in its
column, free, and the team of the board gets a notification.

A claim does not end by age. A claim with no write for some days is **stale**:
Kairos tells the holder and the owner of the initiative. A different person can
change the task. Kairos records the change
and gives a warning. See
[Teams and boards](../../explanation/teams-and-boards/#who-works-on-a-task-the-claim).

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

Two different acts.

**The archive.** The MCP tool `delete_item` and REST `DELETE` do a soft
delete. The CLI verb is `archive`, and `delete` is an alias of it. A soft
delete is the same act as archiving in sense 1 above.

The item is stamped, hidden from default
listings, and remains readable and restorable. It [cascades](#cascade) through
`parent` edges to each descendant that the caller can edit. `restore` reverses
it for one item; archived descendants stay archived. The GUI calls this
Archive.

**The delete for good** (a [purge](#purge)). Only an archived task, and only by
a holder of `purge`. The GUI calls this Delete.

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

A slug that a caller sends must agree with the rule of a [slug](#slug).

## dictionary

A document that gives the meaning of the terms of a team or of the
organization (KAIROS-T-0733). Make it from the template "Dictionary". Its
entries are the rows of a table with the columns Word, Part of speech and
Meaning. A term can have more than one word, for example "production
document".

The [owner board](#owner-board) says whose dictionary it is:

- The delivery board of a team owns the dictionary of the team.
- The board `strategy` owns the dictionary of the organization.

A board has one live dictionary. The server refuses a second one with
`DICTIONARY_EXISTS`.

The terms of a team are the terms of the organization and the terms of the
team. When the two dictionaries have the same word, the team term is used.
The case of the letters is not important.

## document

One of the five work-item types, letter `D`. Documents have no board
placement and therefore no column and no
transitions; instead they carry an editorial lifecycle. A document may be
stamped from a template.

A document can support a strategy, an initiative or a task, through a
`supports` edge. That item is a parent of the document. A document can have
more than one parent, and it can have none.

Each document has an [owner board](#owner-board). The create of a document
needs a board, and nobody can remove the owner board later. The code of a
document has the prefix of its owner board.

A document can [impact](#impacts) a repository. That link says what the
document is about. See
[The owner of a document](../capabilities/#the-owner-of-a-document).

## done column

A column that the board marks as done. It is a terminal column: work in it is
complete. The mark is a flag on the column (`is_done`), and the board sets it.
The name of the column does not decide it. The default boards set the flag on
Completed. The default ADR board sets it on Decided and on Superseded.

Two results follow from the flag:

- The children-progress rollup counts a child in a done column as done. An
  archived child in a done column counts as done too.
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
a task, or the editorial lifecycle of a document. An `impacts` link of a
document or of an ADR is an edit of that item. An archive and a restore are
edits also. A move is not an edit. The change of the owner board of a
document is a move. See [Capabilities](../capabilities/#the-edit-rule).

## entry column

A board's first column by position, among its live columns. A new task and a
new initiative start there. Kairos refuses a create into a different column
(KAIROS-T-0566, KAIROS-T-0643). Create the item, then move it.

A new strategy or ADR goes there by default. A task that moves to a different delivery board
goes there. A [request](#request) goes there.

## external reference

A link from an item to a URI with a label: the relationship `external`. An RFC
in a wiki, a design page or an issue that Kairos does not hold. The label is
the text that a page shows for the link. A task that applies an RFC names the
RFC, so `get_item` and `related_work` show it next to the Kairos items.

An item of each type can have external references. The [edit rule](#edit-rule)
of the item decides who can write one. The scheme of the URI is `http`,
`https` or `mailto`. Kairos refuses each other scheme and names it. A page
renders the URI as a link, so a scheme that a browser runs as code cannot
pass. Kairos does not read the document.

A reference is not an edge between two items. A URI has no short code. So the
graph view, the traversal of a search and the cascade of an archive do not
show it. These reads show it:

- `get_item`: one line `external` per reference
- `related_work`: the section `External references`
- `GET /api/{type}/{short_code}/external`
- the item page: the panel "External references"

A reference stays when the item is archived. A link in the text of an item is
not a reference. It is for a reader, and the tools do not return it.

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
edit it and archive it. See [Capabilities](../capabilities/).

## flight level

The three altitudes of work, from Flight Levels methodology:

| Level | Item type | Short-code letter |
|---|---|---|
| Flight Level 3 | strategy | `S` |
| Flight Level 2 | initiative | `I` |
| Flight Level 1 | task | `T` |

Documents (`D`) and ADRs (`A`) carry short codes but are not flight levels.

See [Flight levels](../../explanation/flight-levels/).

## graph view

The picture of the work around one item: its `blocks` edges, its parent and
its siblings. Open it from the Graph tab of an item.

- A task is one column right of each task that blocks it. Serial work is a row.
  Tasks with no edge between them are in one column: they can go in parallel.
- The tasks of an initiative are in a box. The header of the box names the
  initiative and its strategy.
- The colour of an item is its state: todo, active, blocked or done.
- An open blocker is a heavy arrow. An edge that does not count is a dashed
  arrow.
- When a task has more than 5 siblings with no edge to it, the rest fold into
  `+N`.
- Documents and ADRs are in the side panel, not on the canvas.

To zoom, use the buttons, or hold Ctrl or Cmd and turn the wheel. To pan, drag
the background, or use the arrow keys.

## impacts

The relationship from a document or an ADR to a repository. It says what the
item is about. The vision of the repository `fidius` impacts `fidius`.

The link gives no right. The owner team of the repository gets no right on the
item. A principal who can edit the item can write the link, and needs no right
on the repository. See
[Capabilities](../capabilities/#who-can-write-an-impacts-link).

An item can impact more than one repository. A task does not impact a
repository: a task links to one repository, which says where its code is.

An `impacts` link is not an edge between two items. A repository is not an
item and has no short code. So these reads do not show the link:

- the graph view
- `GET /api/{type}/{short_code}/relationships`
- the traversal of a search
- the cascade of an archive

These reads show the link:

- the item: `impacts` of a document or of an ADR, and the line `impacts` of
  `get_item`
- the repository: `impacted_by` of `GET /api/repositories/{slug}`, and
  `get_repository`
- the filter `repository` of a search, and of the list of documents and of ADRs

A link stays when the repository is deleted, and when the item is archived.
A read shows the repository or the item with a mark.

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
one end. The [edit rule](#edit-rule) decides each end. The create of a
`blocks` edge needs the target, and the source alone gives a proposal. See
[Capabilities](../capabilities/#who-can-write-relationships).

The confirm of an edge proposal does not take this rule. The grant
`approve_proposals` on the board of one end decides it: see
[Capabilities](../capabilities/#the-decision-of-an-edge-proposal).

The `supports` edge of a document is the one exception: the principal must be
able to edit the document.

An [`impacts`](#impacts) link is not an edge between two items. The edit rule
of the document or of the ADR decides who can write it.

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
and work. An item with no board takes the prefix of the organization: the
organization slug in capitals, with letters and digits only. Each other item
takes the prefix of its board. See [short code](#short-code).

## owner

The one person who answers for a strategy or an initiative (KAIROS-T-0411,
KAIROS-A-0025). An item has one owner at most, and it can have none. A new item
and an imported item have no owner.

A member of the board can claim an item with no owner. A member has a grant on
the board. The owner can hand it off to a different member of the board, or
release it. An organization admin can set or clear it. A new owner is always a
member of the board. Nobody else can change it.

The owner stays in each column, Completed too. The owner gets no right: the
grants of the owner decide what the owner can do. Only the `member` grant
gives the edit and the move of the items (KAIROS-T-0541).

An active initiative has an owner (KAIROS-T-0643). With the board policy
`owner_gate` on, an initiative moves into Active only with an owner. In Active
and in Monitoring, the owner cannot release it, and an organization admin
cannot clear it. A hand-off changes the owner. An initiative that is in Active
with no owner stays there.

An initiative starts in the entry column of its board (Discovery). Kairos
refuses a create into a different column with `VALIDATION`. When the owner leaves the
organization, the item has no owner. Do not confuse it with the
[owner board](#owner-board) of a document, or with the [claim](#claim) of a
task.

## owner board

The board that a document names as its owner. The owner board gives the right
to edit the document: a principal with `manage_documents` on it can edit the
document. It is the [authorization board](#authorization-board) of the
document.

Each live board can be an owner board. Its level can be `strategy`,
`initiative`, `delivery` or `adr`.

The document is not a card of its owner board. It has no column and no
transition, and the board view does not show it.

Each document has an owner board. The create of a document needs it, and a
change of the owner board cannot remove it.

The change of the owner board is a move. The principal needs
`manage_documents` on the board that owns the document now and on the new
board. See [Capabilities](../capabilities/#who-can-change-the-owner).

## purge

The delete of a task for good (KAIROS-T-0362): REST
`POST /api/tasks/{code}/purge`, MCP `purge_task`, CLI `kairos tasks purge`, and
the GUI button Delete. Nothing can bring the task back.

Archive the task first. Kairos refuses a live task with `TASK_LIVE`
(KAIROS-T-0570). The purge removes the task, its history, its metadata, its
edges and its forge links. It also removes its external references, its forge
intake row and its claim. The activity log keeps its rows, and gets one row
`purge` with the code and the title.

To purge a task, a principal needs the capability `purge` on the board of the
task. Team membership does not give it. An organization admin has it. The other
destructive operation is dropping a tenant.

## put away

See sense 1 of [archived](#archived).

## relationship

A directed edge between two items. Six types of edge exist. A seventh
relationship goes to a repository, and an eighth to a URI:

| Type | Meaning |
|---|---|
| `parent` | Hierarchy. A delete [cascades](#cascade) along these edges, and they are what a traversal follows to build a parent chain. |
| `supports` | A document or an ADR supporting the item it is attached to. The source is the item, and the target is the document. For a document, the edge has [rules of its own](../capabilities/#the-supports-edge-of-a-document). |
| `informs` | One item informs another without owning it. |
| `supersedes` | One ADR replaces all of another ADR. |
| `amends` | One ADR changes a part of another ADR. The rest of the other ADR stays in force (KAIROS-A-0069). |
| `blocks` | The source blocks the target. |
| [`impacts`](#impacts) | A document or an ADR is about a repository. The target is a repository and not an item. Kairos does not keep this link as an edge between two items. |
| [`external`](#external-reference) | An item names a document outside Kairos. The target is a URI with a label, and not an item. |

Which types are legal between which item types is enforced, as is cycle
prevention. The [link rule](#link-rule) decides who can write an edge, and it
is the same for the five types. The `supports` edge of a document is the one
exception. The [edit rule](#edit-rule) of the document or of the ADR decides
who can write an `impacts` link.

**Complete work does not block, and nothing blocks complete work.** A `blocks`
edge counts only while the items at both ends can move. The edge stops counting
when:

- the item at either end is in a [done column](#done-column), or
- the item at the other end is [put away](#put-away).

A cancelled blocker is the exception (KAIROS-T-0567). A cancel means "won't
do", not done. So a cancel does not resolve a blocker, and its edge still counts.
A person decides: remove the edge, put the blocker away, or reopen it. The list
marks a cancelled end `cancelled` in REST and `[cancelled]` in MCP.

The edge stays in the relationship list of each item, as history. The list
marks the end in a done column (`done` in REST, `[done]` in MCP). It marks the
end that is put away (`archived_at` in REST, `[archived]` in MCP). The counts on
a board card ("blocked by", "blocks") include only the edges that count. Kairos
stores nothing for this rule. When an item moves out of a done column, its
edges count again.

The graph view shows an edge that does not count as a dashed arrow. The legend
of the graph view names the two styles.

## reopen

The move of a done task back to work (KAIROS-T-0564). A reopen takes a
completed task or a [cancelled](#cancel) task. The task moves to the first column of its board
that is not done: Backlog on a default delivery board. The transition rules of
the board do not apply, because a default board has no edge out of Completed.
The policy `parent_gate` applies when that column holds claims.

The reopen removes the cancel mark. The activity log gets a row `transition`,
and the event is `item_transitioned`. A task in a done column cannot move to
another board: reopen it first. The tools are `reopen_item`,
`kairos tasks reopen` and `POST /api/tasks/{short_code}/reopen`. A principal
needs `transition_items` on the board.

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

### The slug of a repository

The slug of a repository must match `^[a-z0-9][a-z0-9-]{1,62}$`, and it cannot
have the form of a UUID. The slug has 2 to 63 characters. The first character
is a lowercase letter or a digit. Each other character is a lowercase letter,
a digit or `-`.

This rule is different from the rule of a [slug](#slug) of a board, a team or
a delivery stream. The slug of a repository comes from the name of the
repository on the forge. That name can start with a digit. The slug of a
repository cannot have the character `_`.

A slug that does not agree with the rule gets the refusal 422 `VALIDATION`
with `details.field` = `slug`. Send a different slug.

### The form of the fields of a repository

The create and the update of a repository refuse a value that does not have
the form of its field. The refusal is `422 VALIDATION`, and `details.field`
names the field. REST, the CLI, the GUI and the MCP tools have the same rules.

Kairos does not remove spaces from a value. It refuses a value that is empty.
It refuses a value that has a space, a tab, a line break or a control
character.

| Field | Form | Maximum |
|---|---|---|
| `repo_full_name` | The name on the forge, as the forge sends it in a webhook. The parts have `/` between them, and each part has text. The name does not end with `.git`. | 255 characters |
| `repo_url` | An absolute `http` or `https` URL with a host. It has no user name and no password. | 2048 characters |
| `default_branch` | A branch name that git accepts. With no value, the code index builder follows the default branch of the forge, and the value is `main` until the first fetch. | 255 characters |

The number of parts of `repo_full_name` comes from the forge:

| Forge | Parts | Example |
|---|---|---|
| `github` | 2 | `acme/payments-api` |
| `gitlab` | 2 or more | `acme/portal/web` |
| `other` | 1 or more | `acme/site` |

Each member of the organization can read `repo_url`. Thus Kairos refuses a URL
such as `https://user:token@host/acme/site`, and the refusal does not show the
value.

A `default_branch` follows these rules of `git check-ref-format`:

- It does not start with `-` or `/`.
- It does not end with `/`, `.` or `.lock`.
- It does not have `..`, `//` or `@{`, and it is not `@`.
- No part between two `/` starts with `.`.
- It has none of the characters `~`, `^`, `:`, `?`, `*`, `[` and `\`.

An update examines a field only when its value is different from the value of
the repository. A repository from before these rules keeps its values. You can
read it, change its other fields, and delete it.

An update with no different value is a success, and Kairos writes nothing.

See [Repositories as execution scope](../../explanation/repositories-as-execution-scope/).

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

The identifier of an item, with the form `{PREFIX}-{LETTER}-{NNNN}`, for
example `SKADI-T-0577`. The prefix is the code prefix of the board of the
item. The letter is the type of the item. The number comes from the sequence
of the prefix and the type. Each API, each CLI command and each MCP tool
accepts a short code.

A move with a rename gives an item a new code and retires the old code. A
read with a retired code finds the item. See
[Short codes](../short-codes/).

## slug

The short name of a board, a team or a delivery stream in a URL and in a
command. A slug that a caller sends must match `^[a-z][a-z0-9_-]{1,62}$`, and
it cannot have the form of a UUID. The slug has 2 to 63 characters. The first
character is a lowercase letter. Each other character is a lowercase letter, a
digit, `-` or `_`.

Kairos applies the rule to the create and to the update of a board, a team and
a delivery stream. A slug that does not agree with the rule gets the refusal
422 `VALIDATION` with `details.field` = `slug`. The message gives the rule.
Send a different slug.

- The rule applies only to a slug that the request changes. A board, a team or
  a delivery stream with a slug from before the rule keeps that slug. You can
  read it, change its other fields and delete it.
- The rule does not apply to the slug `{team-slug}-delivery` that Kairos makes
  for the delivery board of a team. That slug can have more than 63 characters.
- SCIM applies the rule to the slug of the group `kairos-team-<slug>`.
- An [organization](#organization) has a different rule.

A [repository](#repository) has a different rule. The table shows the two
rules.

| Slug of | Rule | First character | `_` |
|---|---|---|---|
| A board, a team or a delivery stream | `^[a-z][a-z0-9_-]{1,62}$` | A lowercase letter | Permitted |
| A repository | `^[a-z0-9][a-z0-9-]{1,62}$` | A lowercase letter or a digit | Not permitted |

The slug of a repository comes from the name of the repository on the forge.
That name can start with a digit, such as `3scale-apicast`. Thus the first
character of the slug of a repository can be a digit. The two rules refuse a
slug with the form of a UUID.

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

A team has a slug, and the page of the team is `/teams/{slug}`. A slug that a
caller sends must agree with the rule of a [slug](#slug).

Every board has a team. A delivery board, a team ADR board, a strategy board and
an initiative board have a team of this kind. The team of the ADR board of the
organization is the list of the members of that board. That list is not a
record in the team directory, and it has no team page.
See [board](#board).

See [Teams and boards](../../explanation/teams-and-boards/).

## template

A reusable starting point for a document: content to stamp, plus a set of
metadata definitions to collect.

A new tenant has the templates "Product Vision" and "Company Vision". The two
give the document type `vision`. See [vision](#vision). A new tenant also has
the template "Dictionary". See [dictionary](#dictionary).

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
snapshot (stored as a diff, with a full copy at most each 10 versions); edits are optimistically concurrent, so an edit based on a stale
version is refused with `CONFLICT` carrying the current version. Setting a
document's editorial lifecycle is not a content edit and does not bump it.

## vision

A document that says why a thing exists. It has the document type `vision`,
which is a value of the metadata definition `document_type`.

A vision can be about a repository, a product or a capability. A team writes
it from the template "Product Vision". The template has these sections: Purpose,
Who It Is For, Current State, Future State, Principles, What It Is Not.

Such a vision names the board of a team as its
[owner board](#owner-board). It [impacts](#impacts) the repository that it is
about. It is not the vision of the organization.

The vision of the organization is a document from the template "Company
Vision".

## work class

Which lane a task is counted in: `planned` or `support`. Defaults to `support`
for a task whose task type is `support`, and to `planned` otherwise. A
[request](#request) is always `support`. Distinct
from [task type](#task-type): a `bug` may be planned work, and a task of type
`task` may be support work.

## work item

See [item](#item).

## Related reading

- [Capabilities](../capabilities/) — the capability vocabulary and glob rules
- [Errors](../errors/) — every refusal code these terms appear in
- [Flight levels](../../explanation/flight-levels/),
  [Teams and boards](../../explanation/teams-and-boards/),
  [Archiving](../../explanation/archiving/) — why the things named here are
  shaped as they are
