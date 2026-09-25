---
id: 001-abac-authorization-board-scoped
level: adr
title: "ABAC Authorization - Board-Scoped Capabilities with Whitelist"
number: 1
short_code: "KAIROS-A-0006"
created_at: 2026-03-04T04:04:57.411382+00:00
updated_at: 2026-07-08T15:00:24.034095+00:00
decision_date: 
decision_maker: 
parent: 
archived: false

tags:
  - "#adr"
  - "#phase/decided"


exit_criteria_met: false
initiative_id: NULL
---

# ADR-6: ABAC Authorization - Board-Scoped Capabilities with Whitelist

## Context

Kairos needs an authorization model that controls who can do what across three Flight Levels (Strategy, Initiative, Delivery). The system uses Keycloak for OIDC authentication (identity), but authorization (permissions) is Kairos's responsibility.

Key requirements:
- Access control must be granular enough to give different users different permissions on different boards
- The system should be transparent — read access is open across the tenant, write access is controlled
- Flight Levels have different ownership models (leadership owns strategy boards, coordinators own initiative boards, team leads own delivery boards) but this should emerge from configuration, not be hardcoded
- Custom access patterns must be supported — not every organization maps neatly to the default Flight Levels ownership model
- Simple to reason about: a user should be able to understand what they can and can't do

Three decisions: what attributes drive access, how capabilities are scoped, and whitelist vs blacklist.

## Decision

### Board-Scoped Capabilities with Whitelist and Glob Matching

Authorization is determined by two things: **board membership** and **capabilities granted on that board**. There are no global capabilities — all write permissions are scoped to a specific board.

**Read access is open tenant-wide.** Any member of the organization can view any board, item, or document within their tenant. This reflects the Flight Levels principle that transparency enables coordination.

**Write access is whitelist-only.** A user can only perform actions on a board if they have an explicit capability grant for that board. No grant = no access.

### Data Model

A single table handles both board membership and capability grants:

```
board_member_capabilities
  - board_id    (uuid, FK boards)
  - user_id     (uuid, FK users)
  - capability  (text)
  - granted_at  (timestamptz)
  - granted_by  (uuid, FK users)
  UNIQUE (board_id, user_id, capability)
```

Each row grants one capability on one board to one user. Multiple rows per user per board.

### Capability Vocabulary

The system defines a fixed set of capabilities:

**Manage capabilities** (CRUD on entity types):
- `manage_strategies` — create, update, delete strategies
- `manage_initiatives` — create, update, delete initiatives
- `manage_tasks` — create, update, delete tasks
- `manage_documents` — create, update, delete documents
- `manage_adrs` — create, update, delete ADRs

**Workflow capabilities**:
- `transition_items` — move items between board columns

**Configuration capabilities** — amended by KAIROS-T-0182 (2026-09-25):
- `configure_boards` — modify board columns, transitions, settings

> **Amendment (KAIROS-T-0182).** `configure_templates` and `configure_metadata`
> are **removed from the vocabulary**. They were listed here from the start, were
> consulted by no handler for their whole life, and could not have been: a grant
> is keyed `(board_id, user_id, capability)`, while `templates` and
> `metadata_definitions` carry no `board_id` — they are tenant-wide, scoped at
> most by `entity_type`. "Configure metadata on board X" could therefore only
> ever have authorised edits affecting every board, which is precisely the
> org-admin authority it was meant to delegate away from.
>
> Template and metadata-definition writes stay org-admin. Making them delegable
> means giving those resources a board scope first, which is a schema change, not
> a permissions fix.
>
> `configure_*` survives as a glob and still covers `configure_boards`. The
> decision this amendment reverses is the vocabulary entry, not the family.
>
> The failure mode is worth recording, because it is the argument for the test
> that now guards it: two of the four non-`manage_*` capabilities were enforced
> and two were not, and the admin interface rendered all four identically. A
> capability nobody checks is indistinguishable from one nobody has used yet.

**Board administration**:
- `manage_members` — add/remove users from the board, grant/revoke capabilities

### Glob Matching

Capabilities support glob patterns for convenience:

- `*` — all capabilities on this board (full access)
- `manage_*` — all manage capabilities
- `configure_*` — all configuration capabilities
- `transition_*` — all transition capabilities (currently just `transition_items`, but future-proof)

Glob matching translates `*` to SQL `LIKE` with `%`. The access check query:

```sql
SELECT EXISTS (
  SELECT 1 FROM board_member_capabilities
  WHERE board_id = $1
    AND user_id = $2
    AND ($3 LIKE replace(capability, '*', '%')
         OR capability = '*')
)
```

Where `$3` is the required capability for the action being performed.

### Access Check Flow

For any write operation:

1. Identify which board the target item belongs to (via `board_id` on the entity)
2. Query `board_member_capabilities` for `(board_id, user_id)` where capability matches the required action
3. If a matching row exists → allow. Otherwise → 403.

For items not on boards:

- **Documents**: Documents are children of workflow items (via `supports` relationship). Authorization is inherited from the parent entity's board. To edit a document that supports an initiative, the user needs write capability on that initiative's board.
- **Templates, metadata definitions, relationships**: These are tenant-wide configuration. Only org admins (`organization_members.role = 'admin'`) can create, modify, or delete them.

### Org Admin Bypass

Users with `role = 'admin'` in `organization_members` have implicit full access to all boards. They don't need explicit `board_member_capabilities` rows. This is the only role that exists outside the board-scoped model.

### Team-Implied Capabilities *(amendment 2026-08-09, KAIROS-T-0072, approved by Dylan)*

Membership of a board's **owning team** (`boards.team_id` → `team_members`) is a second implicit capability source, limited to the day-to-day delivery set:

- `manage_tasks`
- `manage_documents`
- `transition_items`

Nothing else is implied — `configure_*`, `manage_members`, and the strategy/initiative/ADR `manage_*` families remain explicit grants (or org admin). Nothing is stored and nothing needs syncing: the implication is evaluated inside the same single-query check (an `OR EXISTS` arm over the team-membership join, gated by `kairos_core::abac::team_implies`), and leaving the team is the revocation.

Motivation: UAT showed a team member could not work their own team's delivery board without an org admin hand-granting capabilities per member per board — "join the team ⇒ work the team's board" is the expected behavior. Rejected alternatives: auto-granting rows on team join (sync/revocation ambiguity once admins customize grants) and keeping the pure whitelist with seeded defaults (leaves the onboarding chore in place). A team-owned board at a non-delivery level grants its team the same narrow set — acceptable: the set contains no configuration or membership powers, and the level's own `manage_<family>` (e.g. `manage_strategies`) is not in it. Explicit grants and their audit story are unchanged; the implied source is derivable (team roster + board ownership) rather than logged per grant.

## Alternatives Analysis

### Capability Scoping

| Option | Pros | Cons | Risk Level | Cost |
|--------|------|------|------------|------|
| **Global capabilities + board membership** | Simpler model (two tables), fewer rows | Can't restrict a user on a specific board if they have a global capability, all-or-nothing per capability | Low | Low |
| **Board-scoped capabilities** (chosen) | Full granularity — different permissions per board, user can be admin on one board and read-only on another | More rows, slightly more complex queries | Low | Medium |
| **Role-per-level** | Maps to Flight Levels ownership model directly | Hardcodes the assumption that all strategy boards have the same access model, inflexible | Medium | Low |

### Whitelist vs Blacklist

| Option | Pros | Cons | Risk Level | Cost |
|--------|------|------|------------|------|
| **Whitelist** (chosen) | Secure by default — no access unless explicitly granted, easy to audit (list all grants), no "forgot to deny" bugs | More setup required for new users/boards, need defaults to avoid tedious manual grants | Low | Low |
| **Blacklist** | Less setup — users can do everything unless restricted | Insecure by default, "forgot to deny" is a real failure mode, harder to audit ("what can't they do?") | High | Low |
| **Hybrid (whitelist + deny rules)** | Maximum flexibility | Complex evaluation order (allow then deny? deny then allow?), harder to reason about | Medium | Medium |

## Rationale

1. **Board-scoped capabilities map to how Flight Levels actually work.** Each board has its own owner and participants. A coordinator on Initiative Board A shouldn't automatically have write access to Initiative Board B. The board is the natural authorization boundary.

2. **Whitelist is secure by default.** In a system where strategic decisions flow through boards, accidental access is worse than denied access. A user who can't transition a strategy will ask for access. A user who accidentally transitions one won't.

3. **Glob matching provides convenience without complexity.** Granting `*` on a board is a single row. Granting `manage_*` covers all entity management. The pattern set is small and predictable — no regex, no complex matching rules.

4. **Read-open, write-controlled reflects Flight Levels transparency.** The whole point of the board system is making work visible across the organization. Restricting read access would undermine that. Write access is where control matters.

5. **No global capabilities simplifies reasoning.** "What can User X do?" is always answered by looking at their board memberships. There's no separate global layer to check.

## Consequences

### Positive
- Authorization is fully auditable — list `board_member_capabilities` for a user or board to see exactly what's granted
- Secure by default — new users and new boards start with no write access
- Flexible — supports any organizational structure, not just the default Flight Levels ownership model
- Simple to reason about — check board membership, check capability, done
- Glob patterns make common grants concise (`*` for board admins, `manage_*` for content managers)

### Negative
- New user onboarding requires capability grants (mitigated by default policies at implementation time)
- More rows in `board_member_capabilities` than a global capability model
- Items not on boards (standalone documents, templates) need special handling or a synthetic board context
- Org admin bypass is an escape hatch that could be overused

### Neutral
- Default capability sets (what a "coordinator" or "team lead" gets) are a convenience layer on top, not part of the authorization model itself — deferred to implementation
- Keycloak handles authentication only; all authorization logic lives in Kairos application code
- Capability vocabulary is extensible — new capabilities can be added without schema changes (they're text values)