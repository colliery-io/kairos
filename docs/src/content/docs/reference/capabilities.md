---
title: "Capabilities"
---

The names Kairos authorises against, how a stored grant is matched against a
required capability, and which capabilities are computed rather than granted.

Capabilities are scoped to a **board**, not to a user globally. For why the
model is shaped that way — and why it is not a role system — see
[Capabilities and access](../../explanation/capabilities-and-access/). Granting
and revoking are tasks, not facts: see
[the admin endpoints](../rest/boards-and-teams/).

## The grantable vocabulary

These are the values `board_member_capabilities.capability` may hold. Anything
else is refused at grant time.

| Capability | Authorises |
|---|---|
| `manage_strategies` | Create, edit and delete strategies on the board |
| `manage_initiatives` | Create, edit and delete initiatives on the board |
| `manage_tasks` | Create, edit and archive tasks on the board |
| `manage_documents` | Create, edit and delete documents that have the board as their owner |
| `manage_adrs` | Create, edit and delete ADRs on the board |
| `transition_items` | Move items between the board's columns, and between the Planned and Support lanes; cancel a task |
| `configure_boards` | Add, rename, reorder and remove columns and transitions |
| `administer_members` | Add and remove board members, and grant and revoke their capabilities |
| `member` | On a strategy or initiative board only: membership of the board. A member edits and moves each item on the board. See [Computed capabilities](#computed-capabilities). |
| `approve_proposals` | Confirm and reject the edge proposals that have an item of the board at one end. See [The decision of an edge proposal](#the-decision-of-an-edge-proposal). |
| `pages` | Publish and unpublish the sites of the repositories of the team of the board |
| `purge` | Delete an archived task of the board for good (KAIROS-T-0570) |

Team membership does not give `pages` or `purge`. No glob covers them except
`*`. An organization admin has each capability. `manage_tasks` lets a person
archive a task, and a restore undoes an archive. No restore undoes a purge, so
`purge` is a separate grant.

A `manage_<type>` capability gates the create of an item on the board. For an
edit or a delete, the capability is one of three ways in: see
[The edit rule](#the-edit-rule).

### The capabilities of each board level

A board holds one item type and documents. Thus not each capability applies
on each board. These capabilities apply:

| Board level | Capabilities that apply |
|---|---|
| `strategy` | `manage_strategies`, `manage_documents`, `transition_items`, `configure_boards`, `administer_members`, `member`, `approve_proposals` |
| `initiative` | `manage_initiatives`, `manage_documents`, `transition_items`, `configure_boards`, `administer_members`, `member`, `approve_proposals` |
| `delivery` | `manage_tasks`, `manage_documents`, `transition_items`, `configure_boards`, `administer_members`, `pages`, `purge`, `approve_proposals` |
| `adr` | `manage_adrs`, `manage_documents`, `transition_items`, `configure_boards`, `administer_members`, `approve_proposals` |

The grants page of a person uses this table (see below). A change on that page
can grant only a capability of the table.

## Give one person grants on many boards

Use the grants page of the person to set the grants of one person on many
boards in one step.

1. Open **Admin → Members**, and click the name of the person. The page is
   `/admin/members/<user id>`.
2. In the grid, each row is a board, and each column is a capability. The
   rows are in groups by board level. A cell is off when its capability does
   not apply at the level of the board.
3. To grant a capability on each board, tick its column in the **All boards**
   row. Kairos ticks the cell on each board where the capability applies. Click
   the column again to clear it. Then change single rows if necessary.
4. Click **Save**. Kairos sends only the changed cells. It applies all of them
   in one transaction, or none of them. Each grant and each revoke writes an
   activity row, as a single grant does.
5. In **Teams**, add the person to a team or remove the person from a team.

An organization admin sees each board and the teams. A person with
`administer_members` on a board sees the **Members** tab too. The grid shows
only the boards where that person has `administer_members`, and no teams. A
stored glob (`*`, `manage_*`) is not a column. The page shows it as text. To
change it, use the members panel of the board.

The REST calls are `GET /api/members/{user_id}/grants` and
`PATCH /api/members/{user_id}/grants`: see
[Boards and teams](../rest/boards-and-teams/#board-members). Kairos refuses all
of the change with 403 when the caller cannot administer a board of it.
`details.board_slug` names the board.

### Templates and metadata definitions are not delegable

Writes to document templates and metadata definitions are **org-admin only**,
and there is no capability that delegates them. Releases up to and including 0.2.0
offered `configure_templates` and `configure_metadata`; both authorised nothing,
and were removed rather than wired up.

The reason is structural rather than an oversight. A grant is a
`(board, user, capability)` triple, and neither templates nor metadata
definitions belong to a board — they are tenant-wide, scoped at most by item
type. So "configure metadata on this board" could only ever have meant "edit
definitions that affect every board", which is the authority an organization admin already
has. Delegating it properly would mean scoping those resources to boards first.

If you granted either capability, nothing changes for the grantee: the grant
never worked. The inert rows are removed by migration.

A grant is a `(board, user, capability)` triple. The model is a **whitelist**:
a user with no grants on a board has no write access to it. Reads are open
tenant-wide.

## Globs

A stored grant may be a glob. These four are the whole glob vocabulary: they
are the only patterns a grant may carry.

| Grant | Satisfies |
|---|---|
| `*` | Every capability |
| `manage_*` | `manage_strategies`, `manage_initiatives`, `manage_tasks`, `manage_documents`, `manage_adrs` |
| `configure_*` | `configure_boards` (its only member today) |
| `transition_*` | `transition_items` |

Each glob is a genuine family: `manage_*` gives the five work-item capabilities
and nothing else.

That is worth stating because it was once not true. Board administration used to
be called `manage_members`, and matching is textual rather than family-aware, so
`manage_*` covered it — granting what looked like "all the work-item
capabilities" also granted the power to grant and revoke other people's
capabilities. It is now `administer_members`, outside the prefix, and takes its
own grant. If you want someone to have both, grant `manage_*` and
`administer_members`.

**Upgrading:** explicit `manage_members` grants are renamed for you. A holder of
`manage_*` loses board administration, which is the fix rather than a side
effect — grant `administer_members` to anyone who should keep it. `*` holders
are unaffected.

### Matching rules

`*` matches any sequence of characters, including an empty one. Every other
character matches itself. Matching is **case-sensitive**.

The API accepts only the sixteen values in the two tables above as a stored
grant; anything else — including a pattern such as `manage_*s` — is refused at
grant time with `VALIDATION`. The matcher itself is general, and the rules
below describe it, because it is the matcher that decides authorisation and it
is mirrored in SQL as well as in Rust.

The rule worth stating precisely, because it is the one that surprises people:
**`%` and `_` are literal.** The check is implemented as a SQL `LIKE` whose
wildcards are escaped, so a stored grant of `manage_%` matches the capability
literally named `manage_%` — which does not exist — and therefore authorises
nothing. It does not behave as a SQL wildcard. The same applies to `_` and to
`\`.

Degenerate cases fall out of the same rules rather than being special-cased:
an empty grant matches only an empty required capability, and an empty
required capability is matched by any grant whose literal parts are all empty
(`*`, `**`, and so on).

## Computed capabilities

These are never stored and cannot be granted. `board_member_capabilities`
cannot carry them; grant and revoke refuse them. `whoami` reports them under
`implicit`.

| Capability | How it is satisfied |
|---|---|
| `file_backlog` | Every member of the tenant holds it on every live **delivery** board. It permits a request to any team: a **task** in the entry column of its delivery board, in the support lane. The server consults it only when the caller does not hold `manage_tasks` on the board. The work class of a request is `support`. The server refuses a request that sends the work class `planned`. The server refuses each create, a request too, that names a column that is not the entry column (KAIROS-T-0566). The repository is optional. It is not part of the condition. The capability permits the create only. The person who sends a request created it, so the [edit rule](#the-edit-rule) lets that person edit it and archive it. No rule lets that person move it. |

These implications are computed the same way — by the authorisation
check rather than by a stored row:

- **An organization admin passes every check**, on every board, with zero
  grants.
- **Team membership implies a set**, on that team's own board only:
  `manage_tasks`, `manage_documents`, `transition_items`. The shape of the set
  is what matters — a team member can do the daily work of their own delivery
  board without anyone granting it, and cannot configure that board or touch
  another team's. Configuration (`configure_*`) and membership
  (`administer_members`) are deliberately excluded.
- **On the ADR board of a team, the set also has `manage_adrs`.** A member of
  the team can create, edit and delete ADRs on that board with no grant. A
  member of a different team cannot. The ADR board of the organization has no
  team, so `manage_adrs` on it needs a grant. The MCP tools `whoami` and
  `my_boards` show the capabilities that each team board gives.
- **On a strategy or initiative board, the set also has `manage_strategies` or
  `manage_initiatives`.** Each such board has a team. A member of that team can
  edit and move the items of the board with no grant. A person with the
  `member` grant on the board can do the same. Archive and restore still need
  a grant.
- **On a strategy or initiative board, the `member` grant makes a person a
  member.** A person with `member` or `*` on the board is a member of it. A
  member holds these capabilities on the board with no further grant:

  | Board level | Capabilities of a member |
  |---|---|
  | `strategy` | `manage_strategies`, `transition_items` |
  | `initiative` | `manage_initiatives`, `transition_items` |

  Thus a member can edit and move each item on the board.

  No other grant makes a member. A person with only `pages`,
  `configure_boards` or `administer_members` cannot edit or move the items.
  A person with only `transition_items` can move the items and cannot edit
  them. Before KAIROS-T-0541, each grant on the board made a member.

  Archive and restore still need a grant of `manage_<type>`.
  `configure_boards` and `administer_members` still need their own grants.
  Delivery and ADR boards do not have this rule. When you revoke the `member`
  grant of a person, the person is not a member.

  The REST `whoami` shows the capabilities under `implied`, for each board in
  `capabilities`. The MCP tools `whoami` and `my_boards` show them too.

  A person with a grant on the board can claim an item of the board that has
  no [owner](../glossary/#owner). Use `claim_item`, or
  `POST /api/{entity_type}/{short_code}/owner/claim`. For the claim and the
  hand-off, each grant counts, not only `member`. The owner can hand the item
  off to a person with a grant on the board, or release it. An organization
  admin can set or clear the owner. The owner gets no capability.

## `manage_structure`

`manage_structure` is the structure of the organization: teams, boards and
grants. It is the work of an organization admin on that structure, and no
more. An organization admin grants it to a person or to a service account, at
one of two scopes.

| Scope | Authorises |
|---|---|
| The organization | Create, change and delete teams. Add and remove team members. Create boards, and set a code sequence. On each board: `configure_boards` and `administer_members`. Add, change and delete repositories. |
| One board | On that board only: `configure_boards` and `administer_members`. `configure_boards` includes the delete of the board when it is empty. No new team, no new board, no other board. |

It does not authorise these. They stay with the organization admin:

- accounts: organization members, local accounts and service accounts,
- API keys and SCIM tokens,
- the grants of `manage_structure`,
- the deployment admins and the tenant,
- templates, metadata definitions and delivery streams,
- the connections to a git forge and the read tokens of a repository.

The grants are not rows of `board_member_capabilities`. They are rows of
`structure_grants`, and the check reads them by name. Thus no glob covers
`manage_structure`: a holder of `*` or of `manage_*` on a board does not hold
it.

Grant it with `POST /api/structure-grants`, `kairos grants add`, or the MCP
tool `grant_capability`. Revoke it with `DELETE /api/structure-grants/{id}` or
`kairos grants remove`. `whoami` shows the grants of the caller under
`structure`.

A refusal names the capability and the scope. A refusal on the structure of
the organization has `details.scope` = `organization`. A refusal of
`configure_boards` or `administer_members` has `details.or_capability` =
`manage_structure`.

## The scope of an agent key

An agent key has a scope (KAIROS-T-0536, KAIROS-A-0063). The scope is a list of
capabilities and an optional list of boards. A request with the key can do an
action only when two conditions are true:

1. The person can do the action.
2. The scope has the capability of the action, on the board of the action.

Thus a scope never gives a right to the key. It only takes rights away.

### The names of a scope

A scope can name each capability of [the grantable vocabulary](#the-grantable-vocabulary),
`manage_structure` and `file_backlog`. Kairos refuses each other name, and a
glob such as `*`. The refusal is `VALIDATION`, with `details.field` =
`scope.capabilities` and `details.value` = the name.

| Scope | What the key can do |
|---|---|
| No `scope` in the request | The default scope: each capability but `purge` and `manage_structure`, on each board |
| `capabilities: []` | Nothing. The key is read-only |
| A list of capabilities | Only the actions of these capabilities |
| A list of boards | Only the actions on these boards. An action with no board is refused |

A scope does not limit a read. Each member of the organization can read each item.

### Which capability an action needs

| Action | The scope needs |
|---|---|
| An action of a board capability | That capability, for example `manage_tasks` or `purge` |
| An edit, an archive or a restore of an item | `manage_<type>` of the item. The creator and the claim of a task count only through it |
| A request to another team | `file_backlog` |
| A change of teams, boards and the grants of `manage_structure` | `manage_structure` |
| The work of the organization admin role: accounts, keys, SCIM tokens, templates, metadata definitions, delivery streams, forge connections | `manage_structure` |
| A claim, a hand-off or a release of the owner of an item | `member` |
| A write to a team page or an announcement | `manage_documents` |

The organization admin role passes each check only inside the scope. For
example, the default key of an admin can make a task on each board. It cannot
purge a task or make a team.

### The refusal

The refusal is `KEY_SCOPE`, with status 403. The refusal names the scope:

| Field of `details` | Value |
|---|---|
| `required_capability` | The capability of the action |
| `board_id` | The board of the action, or null |
| `scope_needs` | The capability that the scope must have. For the work of the admin role, this is `manage_structure` |
| `reason` | `capability`, `board` or `no_board` |
| `key_scope` | The `capabilities` and the `boards` of the key. `boards` null means each board |

When the scope has the capability and the person does not, the refusal is
`FORBIDDEN`, as for a login.

### Keys from before the scope

Each key that existed before KAIROS-T-0536 has no scope. Such a key has no
limit: it keeps each right of its person. Thus no agent stops at the upgrade.
Make a new key with a scope, and revoke the old key, to limit an agent.

A rotation keeps the scope of the key. The scope of a key does not change
after Kairos makes it. The key of a service account has no scope: a service
account has only its grants.

`whoami` shows the scope of the key in use: REST `agent_key.scope`, the MCP
section `Agent key scope` and the CLI line `key scope:`. See
[Give an agent your key](../../how-to/give-an-agent-your-key/#choose-the-scope-of-the-key).

## How a board is resolved

Authorisation needs a board, and not every item carries one directly.

| Item | Board |
|---|---|
| Strategy, initiative, task | Its own `board_id` |
| ADR on a board | Its own `board_id` |
| ADR not on a board | None |
| Document | Its owner board. Each document has an owner board. |

**When no board resolves, the org-admin-only policy applies.** That is the
fallback for an off-board ADR and for tenant-wide configuration. For an edit,
the creator of the item also passes: see [The edit rule](#the-edit-rule).

A `supports` edge gives a document no board. Up to and including 0.4.0, a
document could have no owner board. Then it took the board of its earliest
`supports` parent.

An `impacts` link is not part of this table. It gives no board and no right.
See [Who can write an `impacts` link](#who-can-write-an-impacts-link).

**An archived item resolves the same board, and therefore the same
capabilities, as it did while live.** Archiving is a visibility default and
not a permission boundary, so putting work away neither widens nor narrows who
may act on it — the write paths refuse archived items on their own, separately
from authorisation. See [Archiving](../../explanation/archiving/).

## The edit rule

A principal is a person or a service account. A principal can edit an item
when one of these conditions is true:

1. The principal created the item.
2. The principal holds the `manage_<type>` capability on the authorization
   board of the item.
3. The principal is an organization admin.
4. The item is a task, and the principal has its [claim](../glossary/#claim)
   (KAIROS-T-0532). A claim through an agent key counts. This condition does
   not apply to an archive or a restore.

The rule applies to each item type: strategy, initiative, task, document, ADR.
The person who works a task writes its notes, its state and its metadata.
That holds when a different team owns the board.

Creation is the primary mechanism of ownership. The server reads who created
the item, and does not read where the item is. If a team moves the item to a
different board, its creator can continue to edit it.

### What an edit is

| Write | REST | MCP tool |
|---|---|---|
| Title and content | `PATCH /api/{type}/{short_code}` | `update_item`, `edit_item` |
| Metadata | `PATCH /api/{type}/{short_code}/metadata` | `set_metadata` |
| Repository of a task | `PUT /api/tasks/{short_code}/repository` | `set_repository` |
| Lifecycle of a document | `PATCH /api/documents/{short_code}/lifecycle` | — |
| `impacts` link of a document or of an ADR | `POST /api/{type}/{short_code}/impacts`, `DELETE /api/{type}/{short_code}/impacts/{repository}` | `link_items`, `unlink_items` |
| Archive | `DELETE /api/{type}/{short_code}` | `delete_item` |
| Restore | `POST /api/{type}/{short_code}/restore` | `restore_item` |

For an archive and a restore, condition 2 needs a grant. Membership of a
strategy or initiative board is not sufficient. See
[Computed capabilities](#computed-capabilities).

### An archive applies the rule to each descendant

An archive cascades through `parent` edges. The server applies the edit rule
to the named item, and then to each descendant. The rule is for the caller of
the archive.

| Item | The caller can edit it | The caller cannot edit it |
|---|---|---|
| The named item | The server archives it. | The server refuses the archive with `FORBIDDEN`. Nothing changes. |
| A descendant | The server archives it, if the caller can edit each item between it and the named item. | The archive stops there. The descendant stays live, and each item below it stays live. |

A descendant that stays keeps its `parent` edge. The archive removes no edge.

The response has `not_reached`: one entry for each live descendant that stays.
An entry has one of two reasons:

| Field | Meaning |
|---|---|
| `required_capability`, `board_id` | The caller cannot edit this descendant. The caller did not create it, and does not hold this capability on this board. `board_id` is absent when the descendant has no authorization board. |
| `below` | This descendant is below the item with this short code, where the archive stopped. |

The response has no `not_reached` field when the archive reached each
descendant. `GET /api/{type}/{short_code}/cascade-preview` gives the same two
lists for the caller who asks, and changes nothing.

An organization admin can edit each item, so an archive by an admin takes each
descendant. A restore changes the named item only, so it applies the rule to
the named item only.

An archive takes no document. The cascade follows `parent` edges, and a
document has none. A document that supports an archived item stays live.

### What creation does not grant

Creation grants no movement. These writes keep the capability check, and the
creator of the item gets no right there:

| Write | REST | MCP tool | Capability |
|---|---|---|---|
| Move between columns | `POST /api/{type}/{short_code}/transition` | `transition_item` | `transition_items` on the board |
| Change the lane | `POST /api/tasks/{short_code}/work-class` | — | `transition_items` on the board |
| Move to a different board | `POST /api/tasks/{short_code}/move` | `move_item` | `manage_tasks` on the two boards |
| Change the owner board of a document | `PATCH /api/documents/{short_code}/board` | `move_item` | `manage_documents` on the two boards |
| Cancel a task | `POST /api/tasks/{short_code}/cancel` | `cancel_item` | `transition_items` on the board |
| Delete an archived task for good | `POST /api/tasks/{short_code}/purge` | `purge_task` | `purge` on the board |
| Archive the tasks of the done columns | `POST /api/boards/{id}/archive-completed` | `archive_completed` | `manage_tasks` on the board |

A team controls its own plan. A person who sends a request to a different
team can edit the request, link it and archive it. That person cannot move it
out of the entry column. That person cannot put it in the planned lane, and
cannot move it to a different board.

Creation grants nothing on a board, a team, a member, a capability, a
repository or the tenant configuration.

Creation does not change who can create. The create of an item needs
`manage_<type>` on the target board, or `file_backlog` for a request. The
create of a document needs `manage_documents` on its owner: see
[The create of a document](#the-create-of-a-document).

## Who can write relationships

This is the link rule. A principal can create or remove an edge when the
principal can edit the item at one end. The source is sufficient, and the
target is sufficient. [The edit rule](#the-edit-rule) decides each end. The
create of a `blocks` edge needs the target: see
[A `blocks` edge needs the blocked item](#a-blocks-edge-needs-the-blocked-item).

| Relationship | Who can write it |
|---|---|
| `parent`, `supports`, `informs`, `supersedes`, `amends` | A principal who can edit the source **or** the target: its creator, **or** a holder of `manage_<type>` on its authorization board, **or** an organization admin |
| Create `blocks` | A principal who can edit the target, the blocked item. With the source only, the call makes a proposal |
| Remove `blocks` | A principal who can edit the source **or** the target |

No relationship type needs the admin role. Releases up to and including 0.4.0
kept `supports`, `informs` and `supersedes` for organization admins. In those
releases the rule for `parent` and `blocks` did not look at the creator of the
target.

The link rule decides who can write an edge. It does not decide which edges
can exist. The type rules, the cycle check and the duplicate check do not
change. A caller who can edit the two ends of an impossible edge gets
`RELATIONSHIP_RULE`, not `FORBIDDEN`.

A refusal names two capabilities, one for each end. The caller needs one of
them.

### A `blocks` edge needs the blocked item

A `blocks` edge changes the work of its target. The card of the target shows
it blocked, and the claimant of the target gets the events. So the create of a
`blocks` edge needs the right to edit the target (KAIROS-T-0569).

| The caller can edit | `link_items` and `POST /api/relationships` give |
|---|---|
| The target | The edge. REST answers 201. |
| The source only | A proposal of the edge, and no edge. REST answers 202 with `message` and `proposal`. |
| Neither end | 403 `FORBIDDEN`. |

The answer of a proposal names the proposal and who can confirm it. A holder
of `approve_proposals` on the board of one end confirms, or an organization
admin. See [The decision of an edge proposal](#the-decision-of-an-edge-proposal).

The server records no proposal of an edge that cannot exist. The type rule
gives `RELATIONSHIP_RULE`, and an edge that exists gives `ALREADY_LINKED`. The
same proposal, pending, gives `CONFLICT`. The confirm does the cycle check.

The refusal of a `blocks` edge names the target first.
`details.required_capability` and `details.board_id` are those of the target.
Each entry of `details.any_of` has `gives`: `edge` for the target, and
`proposal` for the source.

The remove of a `blocks` edge keeps the link rule: a principal who can edit
one end removes it. To remove a blocker does not stop the work of a team.

Until KAIROS-T-0569, a principal who could edit the source wrote a `blocks`
edge. So a team could block the work of a different team, and that team did
not agree.

### The `supports` edge of a document

A `supports` edge does not change which board answers for a document: the
owner board answers for it. The link rule applies to the create of a
`supports` edge to a document. The remove of a `supports` edge of a document
has a narrower rule:

| Write | Who can do it |
|---|---|
| Create `supports` to a document | The link rule: a principal who can edit the source **or** the document |
| Remove a `supports` edge of a document | A principal who can edit the document: its creator, **or** a holder of `manage_documents` on its owner board, **or** an organization admin |

A principal who can edit only the source gets `FORBIDDEN` for the remove. The
refusal names `manage_documents`. So a principal who cannot edit the document
can add a parent to it, and cannot remove that parent.

Each `supports` edge of a document can go, the last one too. The document
keeps its owner board. In 0.4.0 and before, a document could have no owner
board. Then the server kept its last `supports` edge.

### The decision of an edge proposal

The grant `approve_proposals` decides who confirms or rejects an edge
proposal (KAIROS-T-0659). These callers can decide:

- a holder of `approve_proposals` on the board of the source;
- a holder of `approve_proposals` on the board of the target;
- an organization admin.

The board of a document is its owner board. An ADR off a board has no
board. For that end, only an organization admin decides.

The type of the caller does not change the rule. The agent key of a holder
can decide. A service account that holds the grant can decide: use it to
automate the approval. The proposer can decide their own proposal when they
hold the grant.

The rule is the same for `POST /api/proposals/{id}/confirm` and for
`POST /api/proposals/{id}/reject`. The link rule does not apply to a
confirm: the grant on the board of one end replaces it. A refusal is 403
`FORBIDDEN`. `details.required_capability` is `approve_proposals`, and
`details.any_of` gives the board of each end (`board_id` and `board`). A
refused decision leaves the proposal pending.

Each member can propose an edge with no capability, because a proposal
writes no edge. A proposal has one decision. A second decision gets
`CONFLICT`. Of two decisions at the same time, one gets `CONFLICT`.

No grant of `approve_proposals` exists after an upgrade. At first only the
organization admins decide. An admin grants `approve_proposals` to the
people and the service accounts that must decide. A holder of `*` on a
board holds it too.

Before KAIROS-T-0659, a person other than the proposer decided, and not
with an agent key (403 `PERSON_DECIDES`). The caller also had to be able to
edit one end.

## The owner of a document

A board owns a document. A repository does not own a document. A document has
two different links:

| Link | What it says |
|---|---|
| Document to board | The owner. The board gives the right to edit the document. |
| Document to repository, with the relationship `impacts` | What the document is about. The link gives no right. |

Each document has an owner board. The code of the document has the prefix of
that board: see [Short codes](../short-codes/).

A document is not a card of its owner board. It has no column
and no transition. The board view and `board_items` do not show it.

### Which boards can own a document

Each live board can own a document. The level of the board can be `strategy`,
`initiative`, `delivery` or `adr`.

An organization admin can grant `manage_documents` on a board of each level.
Team membership implies `manage_documents` only on the board of that team.
A board with no team has no such members, so it needs a grant.

The server refuses to delete a board that owns a live document. The refusal
is `BOARD_OWNS_DOCUMENTS`, with status 422. Name a different board for each
document, or archive it. The delete of a team has the same rule for each
delivery board of the team.

### The create of a document

The create of a document needs `board`. There is no default board, and a
parent does not give one.

| The create has | The owner | The caller needs |
|---|---|---|
| `board` | The board that the create names | `manage_documents` on that board |
| `board` and a parent | The board that the create names | `manage_documents` on that board |
| No `board` | — | The server refuses the create |

An organization admin passes each check. With `board` and a parent, the
caller creates the document. So the link rule lets the caller link it to the
parent. The caller needs no capability on the board of the parent.

A create with no `board`, or with a null or empty `board`, gets `VALIDATION`,
with status 422. The refusal names `board` in `details.field`, on REST and
on MCP.

### Who can change the owner

The change of the owner board is a move and not an edit. The rule is that of
the move of a task.

| Write | Who can do it |
|---|---|
| Change the owner board | A holder of `manage_documents` on the board that owns the document now **and** on the new board, **or** an organization admin |

The creator of the document gets no right to change its owner.

A call that names the board that the document has is a success. The server
writes nothing.

### A document always has an owner

Each document has an owner board, and nobody can remove it. The rule applies
to each principal, and an organization admin is not an exception.

| Write | What the server does |
|---|---|
| Create a document with no `board` | The refusal is `VALIDATION`, with status 422. The refusal names `board`. |
| Change the owner board with no board, or with a null or empty board | The refusal is `VALIDATION`, with status 422. The refusal names `board` (`to_board` for the MCP tool `move_item`). |
| Remove the last `supports` edge | The server removes the edge. The owner board does not change. |

To give a document to a different team, change its owner board.

### Who can write an `impacts` link

An `impacts` link goes from a document or an ADR to a repository. It is not an
edge between two items, so the link rule does not apply to it.

| Write | Who can do it |
|---|---|
| Create an `impacts` link | A principal who can edit the document or the ADR: [the edit rule](#the-edit-rule) |
| Remove an `impacts` link | The same principal |

The principal needs no right on the repository. The repository can be a live
repository of each team.

The link gives no right. A member of the owner team of a repository cannot
edit a document because the document impacts that repository. The link takes
no right from a principal who can edit the document.

A task, a strategy and an initiative cannot be the subject of the link. The
refusal is `RELATIONSHIP_RULE`. A task links to a repository with
`PUT /api/tasks/{short_code}/repository`.

A link stays when its repository is deleted, and when its document is
archived. A read shows the link with a mark. A principal who can edit the
document can remove a link to a deleted repository.

## Related guides

- [Set up a board](../../how-to/set-up-a-board/) — configuring columns,
  transitions and lanes, and which of these capabilities each step needs
- [Wind down a team](../../how-to/wind-down-a-team/)
- [Give an agent machine access](../../how-to/give-an-agent-machine-access/) —
  a service account is a principal that holds capabilities like any other

## Related reading

- [Capabilities and access](../../explanation/capabilities-and-access/) — why
  board-scoped rather than roles, and why reads are open
- [Errors](../errors/) — the `FORBIDDEN` envelope and what its `details` carry
- [Teams and boards](../../explanation/teams-and-boards/) — what owns a board
