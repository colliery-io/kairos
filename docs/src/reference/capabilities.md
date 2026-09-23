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
| `configure_templates` | **Nothing — see below.** Intended: create and edit document templates |
| `configure_metadata` | **Nothing — see below.** Intended: create and edit metadata definitions |
| `manage_members` | Add and remove board members, and grant and revoke their capabilities |

### Two of these do not work

`configure_templates` and `configure_metadata` can be granted, are shown as
granted in the admin interface, and **authorise nothing**. Template and
metadata-definition writes are org-admin only, and no handler consults either
capability. Tracked as
[KAIROS-T-0182](https://github.com/colliery-io/kairos/blob/main/.metis/backlog/bugs/KAIROS-T-0182.md).

Do not rely on them to delegate that work: grant it and the grantee still gets
403. `configure_boards` and `manage_members`, which look identical in the
interface, are enforced normally.

A grant is a `(board, user, capability)` triple. The model is a **whitelist**:
a user with no grants on a board has no write access to it. Reads are open
tenant-wide.

## Globs

A stored grant may be a glob. These four are the whole glob vocabulary: they
are the only patterns a grant may carry.

| Grant | Satisfies |
|---|---|
| `*` | Every capability |
| `manage_*` | `manage_strategies`, `manage_initiatives`, `manage_tasks`, `manage_documents`, `manage_adrs`, **and `manage_members`** |
| `configure_*` | `configure_boards`, `configure_templates`, `configure_metadata` |
| `transition_*` | `transition_items` |

`manage_*` covers `manage_members` because the match is textual, not
family-aware: `manage_members` begins with `manage_`. A grant of `manage_*` is
therefore a grant of board administration, including granting and revoking
other members' capabilities. To give someone every work-item capability
*without* that, grant the five `manage_<type>` names individually.

### Matching rules

`*` matches any sequence of characters, including an empty one. Every other
character matches itself. Matching is **case-sensitive**.

The API accepts only the fourteen values in the two tables above as a stored
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
| `file_backlog` | Any member of the tenant, on any live **delivery** board. Permits creating a **task** that is issued against a repository that board's team owns and lands in that board's entry column. Nothing past the entry column is opened by it, and it is never consulted for a create that names no repository. |

Two further implications are computed the same way — by the authorisation
check rather than by a stored row:

- **An organization admin passes every check**, on every board, with zero
  grants.
- **Team membership implies a set**, on that team's own board only:
  `manage_tasks`, `manage_documents`, `transition_items`. The shape of the set
  is what matters — a team member can do the daily work of their own delivery
  board without anyone granting it, and cannot configure that board or touch
  another team's. Configuration (`configure_*`) and membership
  (`manage_members`) are deliberately excluded.

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
tenant-wide configuration.

**An archived item resolves the same board, and therefore the same
capabilities, as it did while live.** Archiving is a visibility default and
not a permission boundary, so putting work away neither widens nor narrows who
may act on it — the write paths refuse archived items on their own, separately
from authorisation. See [Archiving](../explanation/archiving.md).

## Relationship edges

Writing a relationship is org-admin by default, with one exception.

| Relationship | Who may write it |
|---|---|
| `parent`, `blocks` | An org admin, **or** a member who manages the source's board, **or** a member who manages the target's board, **or** the user who created the source item |
| `supports`, `informs`, `supersedes` | Org admin only |

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
