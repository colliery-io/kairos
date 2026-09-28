# Capabilities

The names Kairos authorises against, how a stored grant is matched against a
required capability, and which capabilities are computed rather than granted.

Capabilities are scoped to a **board**, not to a user globally. For why the
model is shaped that way — and why it is not a role system — see
[Capabilities and access](../explanation/capabilities-and-access.md). Granting
and revoking are tasks, not facts: see
[the admin endpoints](rest/boards-and-teams.md).

## The grantable vocabulary

These are the values `board_member_capabilities.capability` may hold. Anything
else is refused at grant time.

| Capability | Authorises |
|---|---|
| `manage_strategies` | Create, edit and delete strategies on the board |
| `manage_initiatives` | Create, edit and delete initiatives on the board |
| `manage_tasks` | Create, edit and delete tasks on the board |
| `manage_documents` | Create, edit and delete documents whose parent resolves to the board |
| `manage_adrs` | Create, edit and delete ADRs on the board |
| `transition_items` | Move items between the board's columns, and between the Planned and Support lanes |
| `configure_boards` | Add, rename, reorder and remove columns and transitions |
| `administer_members` | Add and remove board members, and grant and revoke their capabilities |

A `manage_<type>` capability gates the create of an item on the board. For an
edit or a delete, the capability is one of three ways in: see
[The edit rule](#the-edit-rule).

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

The API accepts only the twelve values in the two tables above as a stored
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
| `file_backlog` | Every member of the tenant holds it on every live **delivery** board. It permits a request to any team: a **task** in the entry column of its delivery board, in the support lane. The server consults it only when the caller does not hold `manage_tasks` on the board. The work class of a request is `support`. The server refuses a request that sends the work class `planned`. The server refuses a request that names a column that is not the entry column. The repository is optional. It is not part of the condition. The capability permits the create only. The person who sends a request created it, so the [edit rule](#the-edit-rule) lets that person edit it and archive it. No rule lets that person move it. |

Two further implications are computed the same way — by the authorisation
check rather than by a stored row:

- **An organization admin passes every check**, on every board, with zero
  grants.
- **Team membership implies a set**, on that team's own board only:
  `manage_tasks`, `manage_documents`, `transition_items`. The shape of the set
  is what matters — a team member can do the daily work of their own delivery
  board without anyone granting it, and cannot configure that board or touch
  another team's. Configuration (`configure_*`) and membership
  (`administer_members`) are deliberately excluded.

## How a board is resolved

Authorisation needs a board, and not every item carries one directly.

| Item | Board |
|---|---|
| Strategy, initiative, task | Its own `board_id` |
| ADR on a board | Its own `board_id` |
| ADR not on a board | None |
| Document | Its parent's board, via the `supports` edge where the document is the target. Where several parents exist, the earliest-created edge that resolves to a board wins. |

**When no board resolves, the org-admin-only policy applies.** That is the
fallback for an off-board ADR, a document with no resolvable parent, and
tenant-wide configuration. For an edit, the creator of the item also passes:
see [The edit rule](#the-edit-rule).

An archived parent is a parent. It gives the document the board that it gave
while it was live.

**An archived item resolves the same board, and therefore the same
capabilities, as it did while live.** Archiving is a visibility default and
not a permission boundary, so putting work away neither widens nor narrows who
may act on it — the write paths refuse archived items on their own, separately
from authorisation. See [Archiving](../explanation/archiving.md).

## The edit rule

A principal is a person or a service account. A principal can edit an item
when one of these conditions is true:

1. The principal created the item.
2. The principal holds the `manage_<type>` capability on the authorization
   board of the item.
3. The principal is an organization admin.

The rule applies to each item type: strategy, initiative, task, document, ADR.

Creation is the primary mechanism of ownership. The server reads who created
the item, and does not read where the item is. If a team moves the item to a
different board, its creator can continue to edit it.

### A document with no parent

A document has no board. Its authorization board is the board of its earliest
`supports` parent. See [the board of each item](#how-a-board-is-resolved).

A document with no parent has no authorization board. Condition 2 cannot be
true for it. Only its creator and an organization admin can edit it.

The server does not make a document with no parent: see
[A document always has a parent](#a-document-always-has-a-parent). A tenant
can have one from a release up to and including 0.4.0.

### What an edit is

| Write | REST | MCP tool |
|---|---|---|
| Title and content | `PATCH /api/{type}/{short_code}` | `update_item`, `edit_item` |
| Metadata | `PATCH /api/{type}/{short_code}/metadata` | `set_metadata` |
| Repository of a task | `PUT /api/tasks/{short_code}/repository` | `set_repository` |
| Lifecycle of a document | `PATCH /api/documents/{short_code}/lifecycle` | — |
| Archive | `DELETE /api/{type}/{short_code}` | `delete_item` |
| Restore | `POST /api/{type}/{short_code}/restore` | `restore_item` |

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

### What creation does not grant

Creation grants no movement. These writes keep the capability check, and the
creator of the item gets no right there:

| Write | REST | MCP tool | Capability |
|---|---|---|---|
| Move between columns | `POST /api/{type}/{short_code}/transition` | `transition_item` | `transition_items` on the board |
| Change the lane | `POST /api/tasks/{short_code}/work-class` | — | `transition_items` on the board |
| Move to a different board | `POST /api/tasks/{short_code}/move` | `move_item` | `manage_tasks` on the two boards |

A team controls its own plan. A person who sends a request to a different
team can edit the request, link it and archive it. That person cannot move it
out of the entry column. That person cannot put it in the planned lane, and
cannot move it to a different board.

Creation grants nothing on a board, a team, a member, a capability, a
repository or the tenant configuration.

Creation does not change who can create. The create of an item needs
`manage_<type>` on the target board, or `file_backlog` for a request. The
create of a document needs `manage_documents` on the authorization board of
its parent.

## Who can write relationships

This is the link rule. A principal can create or remove an edge when the
principal can edit the item at one end. The source is sufficient, and the
target is sufficient. [The edit rule](#the-edit-rule) decides each end.

| Relationship | Who can write it |
|---|---|
| `parent`, `blocks`, `supports`, `informs`, `supersedes` | A principal who can edit the source **or** the target: its creator, **or** a holder of `manage_<type>` on its authorization board, **or** an organization admin |

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

### The `supports` edge of a document

The first `supports` edge of a document decides which board answers for the
document. So the link rule is narrower for this edge. It is not narrower for a
`supports` edge to an ADR, or for a different relationship type.

| Write | Who can do it |
|---|---|
| Create `supports` to a document that has a parent | The link rule: a principal who can edit the source **or** the document |
| Create `supports` to a document with no parent | A principal who can edit the document: its creator, **or** an organization admin |
| Remove a `supports` edge of a document | A principal who can edit the document: its creator, **or** a holder of `manage_documents` on its authorization board, **or** an organization admin |

A principal who can edit only the source gets `FORBIDDEN` for the second and
the third write. The refusal names `manage_documents`.

A new parent does not change the authorization board of a document that has a
parent. The earliest edge continues to give the board.

A principal who cannot edit the document can add a parent to it, and cannot
remove that parent.

### A document always has a parent

The server refuses to remove the last `supports` edge of a document. The
refusal is `LAST_PARENT`, with status 422. The rule applies to each principal,
and an organization admin is not an exception.

To move a document to a different item, do these steps:

1. Link the document to the new item.
2. Remove the old edge.

If the document has no more use, archive it.

An edge to an archived parent counts as a parent. The archive of the only
parent of a document does not change who can edit the document.

### The confirm of an edge proposal

The confirm of an edge proposal writes an edge, so the link rule applies to
`POST /api/proposals/{id}/confirm`. The caller must be a person. The caller
must be able to edit the item at one end of the proposed edge. A refused
confirm leaves the proposal pending. To propose an edge, and to reject a proposal,
the link rule is not necessary: they write no edge.

## Related guides

- [Set up a board](../how-to/set-up-a-board.md) — configuring columns,
  transitions and lanes, and which of these capabilities each step needs
- [Wind down a team](../how-to/wind-down-a-team.md)
- [Give an agent machine access](../how-to/give-an-agent-machine-access.md) —
  a service account is a principal that holds capabilities like any other

## Related reading

- [Capabilities and access](../explanation/capabilities-and-access.md) — why
  board-scoped rather than roles, and why reads are open
- [Errors](errors.md) — the `FORBIDDEN` envelope and what its `details` carry
- [Teams and boards](../explanation/teams-and-boards.md) — what owns a board
