# Errors

Every error code Kairos returns, its HTTP status, and what its `details`
carries.

## The envelope

Every `/api` error has the same shape:

```json
{
  "error": {
    "code": "RESTORE_BLOCKED",
    "message": "DEMO-T-0012 cannot be restored because its board column (removed) is gone; …",
    "details": { "missing": ["its board column (removed)"] }
  }
}
```

`code` is stable and safe to branch on. `message` is for a human and may
change. `details` is present only where a code has something structured to
say; the table below states which do.

`/scim/v2` uses the RFC 7644 envelope instead, not this one — except for a
path under `/scim/v2` that is not routed at all, which answers a `NOT_FOUND`
in *this* envelope. See [SCIM](scim.md#errors).

## Codes

### Generic

| Code | Status | Meaning | `details` |
|---|---|---|---|
| `UNAUTHORIZED` | 401 | No token, or a token that does not verify | — |
| `FORBIDDEN` | 403 | Authenticated, but not allowed | one of four shapes — see below |
| `MEMBERSHIP_REQUIRED` | 403 | Authenticated against the issuer, but not a member of this tenant | `organization` — the slug that was resolved |
| `NOT_FOUND` | 404 | The thing the call is about does not exist | — |
| `TENANT_NOT_FOUND` | 404 | The request host resolves to no provisioned tenant | — |
| `CONFLICT` | 409 | A state conflict. Usually optimistic concurrency: the submitted `version` is stale. Also emitted where a create collides with an existing row, or a delete is blocked by what still points at the entity | `current` — the full current entity — on a version conflict **only**; see below |
| `VALIDATION` | 422, or **400** on `POST /api/search` | A body or a reference is malformed, or names something that does not exist | `field`/`fields` where a specific field is at fault, plus any typed extras (e.g. `cap`, `limit`, `offset`, `depth`) |
| `INTERNAL` | 500 | Server fault; the message is logged, not returned in detail | — |

**`VALIDATION` is 422 everywhere except `POST /api/search`**, which answers
**400** for every rejection `kairos_core::search::validate` and the DTO
conversion produce — a blank `q`, an inverted date range, a bad `limit` or
`offset`, a missing or over-cap `traverse.depth`, a malformed UUID or
timestamp, an out-of-vocabulary enum value, an unknown field. The code is the
same; the status is not. Branch on the code.

**`FORBIDDEN`'s `details` has four shapes**, and which one arrives depends on
what kind of gate refused:

| Shape | Emitted by |
|---|---|
| `required_capability`, `board_id` (null = the org-admin-only fallback applied) | a board capability check |
| `required_capability`, `board_id`, `held: "file_backlog"` | an MCP write refused to a cross-team filer, whose message states the Backlog-only rule |
| `required_role: "admin"`, and `relationship` when an edge was at fault | tenant-wide configuration: templates, metadata definitions, and non-collaborative relationship types |
| `relationship` | a `parent`/`blocks` edge where the caller manages neither board and did not create the source |
| `required: "deployment_admin"` | the cross-tenant provisioning routes, which are not scoped to any organization |

**`CONFLICT` is not only optimistic concurrency.** `details.current` is present
only on a version conflict; a client that reads it unconditionally will find
nothing on the others. The other 409s and what they carry instead:

| Condition | `details` |
|---|---|
| Deleting a team that still owns repositories | `repositories` — the slugs to re-home |
| Registering a repository whose slug is taken | — |
| Registering a repository already registered for that forge | — |
| `DEFINITION_IN_USE` (its own code; see below) | its own fields |

**The distinction to internalise**, because it decides how a client branches:
**a reference that does not resolve is `VALIDATION`; the call's own subject not
existing is `NOT_FOUND`.** Creating a task against a repository slug that does
not exist is `VALIDATION` — the repository was a reference in the body.
Fetching `/api/tasks/ACME-T-9999` is `NOT_FOUND` — the task was the subject.

There is **one exception**: `search` refuses an unresolvable `traverse.from`
with `NOT_FOUND`, not `VALIDATION`, even though it is a reference in the body.
A client applying the general rule would branch wrongly here.

### Board and column configuration

| Code | Status | Meaning | `details` |
|---|---|---|---|
| `INVALID_TRANSITION` | 422 | The move is not an edge in the board's transition graph | `from` and `to` (`id`, `name` each) and `allowed_targets` — the columns reachable from the current one, as `id`/`name` pairs |
| `ITEM_NOT_ON_BOARD` | 422 | The item has no board placement, so it cannot transition or be moved: an off-board ADR, and over MCP a document, which never has one | — |
| `COLUMN_NOT_EMPTY` | 422 | The column still holds live items | `item_count` — live items only; archived ones do not count — and `column` (`id`, `name`) |
| `BOARD_NOT_EMPTY` | 422 | The board still holds live items | `item_count` and `items` — the blocking short codes, **capped at 20** even when `item_count` is higher. Deleting a team adds `board_id` |
| `DUPLICATE_COLUMN_NAME` | 422 | A live column of that board already has the name | — |
| `DUPLICATE_COLUMN_POSITION` | 422 | A live column of that board already holds the position | — |
| `DUPLICATE_TRANSITION` | 422 | That edge already exists | — |
| `NO_ENTRY_COLUMN` | 422 | The target board has no column to admit an arriving item | — |

### Moving and restoring work

| Code | Status | Meaning | `details` |
|---|---|---|---|
| `SAME_BOARD` | 422 | The move's target is the board the item is already on | — |
| `NOT_DELIVERY_BOARD` | 422 | Cross-board moves are between delivery boards only | — |
| `REPOSITORY_OWNER_MISMATCH` | 422 | The task's repository is owned by a team other than the target board's | `repository` and `owner_board_id` — null when the owner has no single delivery board, in which case the binding must be cleared rather than followed |
| `RESTORE_BLOCKED` | 422 | The item's board, column, owning team or repository has been removed, so it has nowhere to return to | `missing` — a list naming each thing that is gone |

### Relationships

| Code | Status | Meaning | `details` |
|---|---|---|---|
| `CYCLE_DETECTED` | 422 | The edge would create a cycle | — |
| `RELATIONSHIP_RULE` | 422 | The edge is not legal between those two entity types | `relationship` |
| `ALREADY_LINKED` | 422 | That edge already exists | — |

### Tenant configuration

| Code | Status | Meaning | `details` |
|---|---|---|---|
| `DEFINITION_IN_USE` | **409** | The metadata definition is still referenced | `item_values`, `template_fields` (the true counts) and `items`, `templates` (which ones). Each `items` entry is `{short_code, archived}` — an archived carrier is marked, and still blocks. `items` is **capped at 20**, so a definition stamped on thousands of items names twenty of them and counts the rest |
| `SLUG_CONFLICT` | 422 | The slug is taken | — |
| `PROTECTED_PAGE` | 422 | The team page is protected and cannot be removed | — |
| `FOLDER_NOT_EMPTY` | 422 | The team-page folder still has children | — |
| `LAST_ADMIN` | 422 | The change would leave the organization with no admin | — |
| `CONFIRMATION_REQUIRED` | 422 | A destructive call needs an explicit confirmation flag | — |

Note `DEFINITION_IN_USE` is **409**, not 422, unlike its neighbours — it
reports a state conflict rather than a malformed request.

### Deployment and integration

| Code | Status | Meaning |
|---|---|---|
| `WEBHOOK_REJECTED` | 401 | A forge webhook failed signature verification, or named an unknown tenant or connection — one indistinguishable refusal for all of them |
| `FORGE_NOT_CONFIGURED` | 501 | The operation needs a forge connection the tenant has not set up |
| `PUBLIC_URL_NOT_CONFIGURED` | 501 | The operation needs a publicly reachable URL the deployment has not been given |
| `IDP_UNREACHABLE` | 502 | The OIDC issuer could not be reached |
| `WEB_DIST_MISSING` | 503 | The GUI bundle is absent from the running binary |

`WEBHOOK_REJECTED` is served from `/webhooks/{forge}/{tenant}/{connection_id}`
rather than `/api`, and its envelope carries `code` and `message` only — no
`details` key at all.

## What archived work refuses

Archived items are readable and frozen, so a write to one is refused as though
the item were not there: `NOT_FOUND` with a message naming the short code. The
reason it is not a distinct code is that nothing a client can do differs — it
has to restore the item first either way. See
[Archiving](../explanation/archiving.md).

## Related guides

The refusals a reader most often arrives here from:

- `RESTORE_BLOCKED` → [Find archived work](../how-to/find-archived-work.md)
- `BOARD_NOT_EMPTY`, and a team that will not delete →
  [Wind down a team](../how-to/wind-down-a-team.md)
- `COLUMN_NOT_EMPTY`, `INVALID_TRANSITION`, `DUPLICATE_COLUMN_NAME` →
  [Set up a board](../how-to/set-up-a-board.md)
- `SAME_BOARD`, `NOT_DELIVERY_BOARD`, `REPOSITORY_OWNER_MISMATCH` →
  [Move work between boards](../how-to/move-work-between-boards.md)
- `FORGE_NOT_CONFIGURED`, `WEBHOOK_REJECTED` →
  [Connect a git forge](../how-to/connect-a-git-forge.md)

## Related reading

- [Capabilities](capabilities.md) — what `FORBIDDEN`'s
  `required_capability` refers to
- [MCP tools](mcp-tools.md) — which of these codes each tool can return
- [REST API](rest-api.md) — the per-endpoint response tables
