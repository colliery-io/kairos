# Errors

Every error code Kairos returns, its HTTP status, and what its `details`
carries.

## The envelope

Every `/api` error has the same shape:

```json
{
  "error": {
    "code": "RESTORE_BLOCKED",
    "message": "The server cannot restore PLATFORM-T-0012. The item needs its board column (removed). …",
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
| `FORBIDDEN` | 403 | Authenticated, but not allowed | one of five shapes — see below |
| `MEMBERSHIP_REQUIRED` | 403 | Authenticated against the issuer, but not a member of this tenant | `organization` — the slug that was resolved |
| `NOT_FOUND` | 404 | The thing the call is about does not exist | — |
| `TENANT_NOT_FOUND` | 404 | The request host resolves to no provisioned tenant | — |
| `TENANT_NOT_READY` | 503 | The schema of the organization did not migrate when the server started. The other organizations serve. An operator repairs the schema and runs `kairos-server migrate-tenants`; the organization then serves again within 30 seconds, with no restart | — |
| `CONFLICT` | 409 | A state conflict. Usually optimistic concurrency: the submitted `version` is stale. Also emitted where a create collides with an existing row, or a delete is blocked by what still points at the entity | `current` — the full current entity — on a version conflict **only**; see below |
| `VALIDATION` | 422, or **400** on `POST /api/search`. 400 or 415 for [an input that the server cannot read](#an-input-that-a-route-does-not-accept) | A body, a query or a reference is malformed, or names something that does not exist | `field`/`fields` where a specific field is at fault, plus any typed extras (e.g. `cap`, `limit`, `offset`, `depth`). `field` and `allowed` for a field that the route does not know. `parameter` and `allowed` for a query parameter that the route does not know |
| `INTERNAL` | 500 | Server fault; the message is logged, not returned in detail | — |

**`VALIDATION` is 422 everywhere except `POST /api/search`**, which answers
**400** for every rejection `kairos_core::search::validate` and the DTO
conversion produce — a blank `q`, an inverted date range, a bad `limit` or
`offset`, a missing or over-cap `traverse.depth`, a malformed UUID or
timestamp, an out-of-vocabulary enum value, an unknown field. The code is the
same; the status is not. Branch on the code.

### An input that a route does not accept

**The server refuses each input that a route does not know.** The server does
not ignore the input: an input that does nothing tells the client that it did
something. The refusal is `VALIDATION`, the message names the input, and the
server writes nothing. The rule has three parts:

| The input | The rule | Status |
|---|---|---|
| A field of the body | A route refuses a field that it does not know | 422 |
| A query parameter | A route refuses a parameter that it does not know. A route with no query parameters refuses each parameter | 400 |
| The body | A route with no body refuses a body that is not empty | 400 |

The rule applies to each route below `/api`, the `GET` routes too, and to
`/ws/events`.

#### A field of the body

Each write route of the API has a body with a known set of fields. The table of
the body in [Schemas](rest/schemas.md) gives the set. In the OpenAPI spec the
schema of the body has `additionalProperties: false`.

```json
{
  "error": {
    "code": "VALIDATION",
    "message": "The body has the field \"board_level\". This route does not accept that field. The fields of the body are: name, slug, team_id.",
    "details": { "field": "board_level", "allowed": ["name", "slug", "team_id"] }
  }
}
```

`details.field` is the first unknown field that the server finds. `details.allowed`
is the list of the fields of that body. For a field in an object of the body, the
two are about that object.

The body of an update has the fields of the update only. Do not send back the
full object that a `GET` gave. Fields such as `id`, `created_at` and
`board_level` are not fields of an update.

A body that the route cannot read for a different reason has the same envelope
and the same code. The status tells the reasons apart:

| The body | Status | `details` |
|---|---|---|
| Has a field that the route does not know | 422 | `field`, `allowed` |
| Has a different fault of shape: a required field is absent, or a value has the wrong JSON type | 422 | — |
| Is not JSON | 400 | — |
| Has no `Content-Type: application/json` header | 415 | — |

On `POST /api/search` the first two are 400, as each refusal of that route is.

#### A query parameter

The page of each route in the [REST API](rest-api.md) gives its query
parameters. `GET /api/boards?limit=5&page=2` has this refusal:

```json
{
  "error": {
    "code": "VALIDATION",
    "message": "The request has the query parameter \"page\". This route does not accept that parameter. The query parameters of this route are: limit, offset.",
    "details": { "parameter": "page", "allowed": ["limit", "offset"] }
  }
}
```

`details.parameter` is the first unknown parameter that the server finds.
`details.allowed` is the list of the parameters of the route. For a route with
no query parameters the list is empty, and the message says `This route has no
query parameters.`

A parameter with a value that the route cannot read has the same status and the
same code. `GET /api/boards?limit=many` has the message `The value of the query
parameter "limit" is not correct: invalid digit found in string.` Its `details`
has `parameter` only.

`access_token` is the one query parameter of `/ws/events`.

#### A body on a route that has none

These routes accept no body:

- each `GET` and each `DELETE`
- a restore: `POST /api/{family}/{short_code}/restore`
- the confirm and the reject of a proposal
- the rotate of a forge connection
- `POST /api/logout`

A request to one of them with a body has this refusal:

```json
{
  "error": {
    "code": "VALIDATION",
    "message": "The request has a body. This route does not accept a body. Send the request with no body.",
    "details": {}
  }
}
```

A body of zero length is no body. `{}` and `null` are bodies, and the server
refuses them.

#### The routes that the rule does not apply to

A third party defines the inputs of these routes, so the rule does not apply:

| Route | The inputs are those of |
|---|---|
| `/scim/v2` | RFC 7644. It tells a SCIM server to ignore an attribute that it does not know |
| `/webhooks` | The git forge |
| `POST /api/auth/token` | The OAuth standard |
| `/mcp`, `/.well-known/oauth-protected-resource` | The MCP transport and RFC 9728. The arguments of a tool have the rule: see below |
| `/healthz`, `/readyz`, `/metrics` | The probe of the platform that runs the server |
| Each path of the GUI, and its files | The browser. The GUI reads the query of its own pages |

Each MCP tool has the same rule for its arguments. See
[MCP tools](mcp-tools.md#refusal-codes).

**`FORBIDDEN`'s `details` has five shapes**, and which one arrives depends on
what kind of gate refused:

| Shape | Emitted by |
|---|---|
| `required_capability`, `board_id` (null = the item or the write has no board, and the caller needs the admin role) | a board capability check, or the [edit rule](capabilities.md#the-edit-rule). For the edit rule, the caller also did not create the item |
| `required_capability`, `board_id`, `held: "file_backlog"` | an MCP move (`transition_item`, `move_item`) refused to the person who sent a request, while the request is in the entry column of its board. The message says that the item is a request, that the team of the board moves it, and that the caller can edit, link and archive it |
| `required_role: "admin"` | tenant-wide configuration: templates and metadata definitions |
| `relationship`, `required_capability`, `board_id`, `any_of` | an edge of any relationship type where the caller can edit neither end (the [link rule](capabilities.md#who-can-write-relationships)). `required_capability` and `board_id` are those of the source. `any_of` has one entry for each end: `end`, `required_capability`, `board_id`. For [the `supports` edge of a document](capabilities.md#the-supports-edge-of-a-document), the caller cannot edit the document: `any_of` has one entry, the target, and `required_capability` is `manage_documents` |
| `required: "deployment_admin"` | the cross-tenant provisioning routes, which are not scoped to any organization |

**`CONFLICT` is not only optimistic concurrency.** `details.current` is present
only on a version conflict; a client that reads it unconditionally will find
nothing on the others. The other 409s and what they carry instead:

| Condition | `details` |
|---|---|
| Deleting a team that still owns repositories | `repositories` — the slugs to re-home |
| Registering a repository whose slug is taken | — |
| The create or the update of a board with the slug of a live board. The create of a team whose delivery board gets such a slug | `slug`, and `board` (`id`, `name`): the board that has the slug |
| Registering a repository already registered for that forge | — |
| `DEFINITION_IN_USE` (its own code; see below) | its own fields |

**The distinction to internalise**, because it decides how a client branches:
**a reference that does not resolve is `VALIDATION`; the call's own subject not
existing is `NOT_FOUND`.** Creating a task that names a repository slug that does
not exist is `VALIDATION` — the repository was a reference in the body.
Fetching `/api/tasks/ACME-T-9999` is `NOT_FOUND` — the task was the subject.

There is **one exception**: `search` refuses an unresolvable `traverse.from`
with `NOT_FOUND`, not `VALIDATION`, even though it is a reference in the body.
A client applying the general rule would branch wrongly here.

### A retired short code

When an item gets a new short code, its old code becomes a retired code. A
read, for example `GET /api/tasks/{short_code}` or the history, finds the item
from a retired code. The response has the item with its current `short_code`.

Kairos refuses a write with a retired code with `NOT_FOUND`. The message names
the current code, and `details` has these fields:

| Field | Value |
|---|---|
| `retired_code` | The retired code of the request. |
| `current_code` | The current code of the item. Send the write again with this code. |

### A `limit` or an `offset` that is out of the range

The server does not refuse a `limit` or an `offset` that is out of the range.
It uses the nearest value of the range. A `limit` above the maximum becomes
the maximum. A `limit` below 1 becomes 1. An `offset` below 0 becomes 0. The
response gives the `limit` and the `offset` that the server used.

This rule applies to each list route and to `POST /api/search`. It also
applies to the MCP tools and the CLI commands that have these arguments. The
server refuses a value that is not an integer, with `VALIDATION`.

### Board and column configuration

| Code | Status | Meaning | `details` |
|---|---|---|---|
| `INVALID_TRANSITION` | 422 | The move is not an edge in the board's transition graph | `from` and `to` (`id`, `name` each) and `allowed_targets` — the columns reachable from the current one, as `id`/`name` pairs |
| `ITEM_NOT_ON_BOARD` | 422 | The item has no board placement, so it cannot transition or be moved: an off-board ADR, and over MCP a document, which never has one | — |
| `NOT_CLAIMABLE` | 422 | A hand-off or a release of the claim of a task that is not in a column that holds claims (Active) (KAIROS-T-0359) | — |
| `NO_CLAIM` | 422 | A release of the claim of a task that has no claim | — |
| `COLUMN_NOT_EMPTY` | 422 | The column still holds live items | `item_count` — live items only; archived ones do not count — and `column` (`id`, `name`) |
| `BOARD_NOT_EMPTY` | 422 | The board still holds live items | `item_count` and `items` — the blocking short codes, **capped at 20** even when `item_count` is higher. Deleting a team adds `board_id` and `board` (`id`, `name`, `slug`): the board that is not empty. A team of old data can have 2 or more delivery boards, and the delete of the team examines each |
| `DUPLICATE_COLUMN_NAME` | 422 | A live column of that board already has the name | — |
| `DUPLICATE_COLUMN_POSITION` | 422 | A live column of that board already holds the position | — |
| `DUPLICATE_TRANSITION` | 422 | That edge already exists | — |
| `NO_ENTRY_COLUMN` | 422 | The target board has no column to admit an arriving item | — |
| `TEAM_HAS_DELIVERY_BOARD` | 422 | The team has a live delivery board. A team has only one delivery board | `team_id` and `board` (`id`, `name`, `slug`), the board that the team has |
| `TEAM_HAS_ADR_BOARD` | 422 | The team has a live ADR board. A team has only one ADR board | `team_id` and `board` (`id`, `name`, `slug`), the board that the team has |
| `BOARD_OWNS_DOCUMENTS` | 422 | The board is the owner of live documents. Name a different board for each document, or archive it. The delete of a team gives this code for a delivery board of the team | `board_id`, `item_count` and `items`: the short codes of the documents, **capped at 20** |
| `LAST_DELIVERY_BOARD` | 422 | The board is the only delivery board of a team. Delete the team to remove the team and its board together | `board_id` and `team` (`id`, `name`) |
| `BOARD_TEAM_IS_FIXED` | 422 | The update of a board has a `team_id` that is not the team of the board. The team of a board does not change. Move the task to give work to a different team | `board_id` and `team_id`, the team that the board has |
| `CODE_PREFIX_IS_FIXED` | 422 | The body of a route other than a create has the field `code_prefix`, for example the update of a board. The short-code prefix of a board does not change. To give an item a code with a different prefix, move it to a different board with a rename | `field`: `code_prefix`, and `allowed`: the fields of the body |
| `CODE_IN_USE` | 409 | `PUT /api/boards/{board}/code-sequences/{item_type}`: an item (live or archived) has the code of the number. See [Short codes](short-codes.md#keep-the-numbers-of-an-import) | `code` |
| `CODE_RETIRED` | 409 | The same route: the code of the number is retired | `code`, and `current_code`: the current code of its item, or null |
| `SEQUENCE_IS_PAST` | 409 | The same route: the sequence is at the number or above it. A sequence does not go back | `last_number` and `next_code`: the code that the next create gets |

### Moving and restoring work

| Code | Status | Meaning | `details` |
|---|---|---|---|
| `SAME_BOARD` | 422 | The move's target is the board the item is already on | — |
| `NOT_DELIVERY_BOARD` | 422 | Cross-board moves are between delivery boards only | — |
| `RENAME_NOT_NEEDED` | 422 | A move with a rename to a board whose prefix the code has already, or a rename of a document whose owner board does not change. Nothing changes. Do the move with no rename | `argument`: `rename`. For a prefix, also `short_code` and `code_prefix` |
| `RESTORE_BLOCKED` | 422 | The item's board, column, owning team or repository has been removed, so it has nowhere to return to. For a document, the board is its owner board | `missing` — a list naming each thing that is gone |

Each document has an owner board. The server refuses a document create with
no board. It also refuses a change of the owner board with no board. The
refusal is `VALIDATION` with `details.field` = `board` (MCP:
`details.argument` = `board`, or `to_board` for `move_item`). See
[A document always has an owner](capabilities.md#a-document-always-has-an-owner).

### Relationships

| Code | Status | Meaning | `details` |
|---|---|---|---|
| `CYCLE_DETECTED` | 422 | The edge would create a cycle | — |
| `RELATIONSHIP_RULE` | 422 | The edge is not legal between those two entity types. For an `impacts` link: the subject is not a document and not an ADR | `relationship`. For an `impacts` link, also `source_type` and `allowed_source_types` |
| `ALREADY_LINKED` | 422 | That edge already exists. For an `impacts` link: the item impacts that repository already | — |

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
| `SECRETS_NOT_CONFIGURED` | 501 | The deployment has no `KAIROS_SECRETS_KEY`, so the server cannot keep the read token of a repository. `details.setting` names the setting |
| `CREDENTIAL_UNREADABLE` | 409 | The stored read token of a repository does not decrypt: the key changed, the key is not set, or the row was changed. Set the token again |
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
- `SAME_BOARD`, `NOT_DELIVERY_BOARD` →
  [Move work between boards](../how-to/move-work-between-boards.md)
- `FORBIDDEN` or `VALIDATION` on a task that you create for a different team →
  [Send a request to a different team](../how-to/move-work-between-boards.md#when-a-request-is-refused)
- `VALIDATION` with `details.field` = `board` on a document →
  [A document always has an owner](capabilities.md#a-document-always-has-an-owner)
- `BOARD_OWNS_DOCUMENTS` →
  [The owner of a document](capabilities.md#the-owner-of-a-document)
- `FORGE_NOT_CONFIGURED`, `WEBHOOK_REJECTED` →
  [Connect a git forge](../how-to/connect-a-git-forge.md)
- `SECRETS_NOT_CONFIGURED`, `CREDENTIAL_UNREADABLE` →
  [Read a private repository](../how-to/read-a-private-repository.md)

## Related reading

- [Capabilities](capabilities.md) — what `FORBIDDEN`'s
  `required_capability` refers to
- [MCP tools](mcp-tools.md) — which of these codes each tool can return
- [REST API](rest-api.md) — the per-endpoint response tables
