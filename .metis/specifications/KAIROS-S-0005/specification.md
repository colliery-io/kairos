---
id: api-design
level: specification
title: "API Design"
short_code: "KAIROS-S-0005"
created_at: 2026-03-05T02:50:26.525695+00:00
updated_at: 2026-03-05T02:50:26.525695+00:00
parent: KAIROS-I-0001
blocked_by: []
archived: false

tags:
  - "#specification"
  - "#phase/discovery"


exit_criteria_met: false
initiative_id: NULL
---

# Kairos API Design

Consolidated from KAIROS-A-0005 (API structure), A-0004 (versioning), A-0006 (authorization), A-0007 (unified search).

Tenant context: subdomain (`acme.kairos.io/api/...`). Local dev: `X-Tenant` header fallback or single-tenant mode.

**Authorization model (KAIROS-A-0006):**
- All reads are open tenant-wide.
- Write on board items: board membership + matching capability.
- Write on documents: inherited from parent entity's board (via `supports` relationship).
- Templates, metadata definitions, relationships: org admin only.
- DELETE is soft delete (sets `deleted_at`). Cascades to children via `parent` relationships.

## Point Access (GET)

Simple accessors. No filter params. Optional `?limit=&offset=` for pagination on list endpoints.

### Strategies

```
GET    /api/strategies                              List all
GET    /api/strategies/{short_code}                  Get one
POST   /api/strategies                              Create
PATCH  /api/strategies/{short_code}                  Update content (version required)
DELETE /api/strategies/{short_code}                  Soft delete (cascades to children)
POST   /api/strategies/{short_code}/transition       Move to column
```

### Initiatives

```
GET    /api/initiatives                              List all
GET    /api/initiatives/{short_code}                  Get one
POST   /api/initiatives                              Create
PATCH  /api/initiatives/{short_code}                  Update content (version required)
DELETE /api/initiatives/{short_code}                  Soft delete (cascades to children)
POST   /api/initiatives/{short_code}/transition       Move to column
```

### Tasks

```
GET    /api/tasks                                    List all
GET    /api/tasks/{short_code}                        Get one
POST   /api/tasks                                    Create
PATCH  /api/tasks/{short_code}                        Update content (version required)
DELETE /api/tasks/{short_code}                        Soft delete
POST   /api/tasks/{short_code}/transition             Move to column
```

### Documents

```
GET    /api/documents                                List all
GET    /api/documents/{short_code}                    Get one
POST   /api/documents                                Create (optional: template_id to stamp content + metadata)
PATCH  /api/documents/{short_code}                    Update content (version required)
DELETE /api/documents/{short_code}                    Soft delete
```

### ADRs

```
GET    /api/adrs                                     List all
GET    /api/adrs/{short_code}                         Get one
POST   /api/adrs                                     Create
PATCH  /api/adrs/{short_code}                         Update content (version required)
DELETE /api/adrs/{short_code}                         Soft delete
POST   /api/adrs/{short_code}/transition              Move to column (on ADR board)
```

### Boards

```
GET    /api/boards                                   List all boards
GET    /api/boards/{id}                               Board detail (columns, transitions)
POST   /api/boards                                   Create board
PATCH  /api/boards/{id}                               Update board settings
DELETE /api/boards/{id}                               Soft delete board (must be empty)
GET    /api/boards/{id}/items                          All items on board, grouped by column
GET    /api/boards/{id}/columns                        List columns
POST   /api/boards/{id}/columns                        Add column
PATCH  /api/boards/{id}/columns/{col_id}               Update column (name, position)
DELETE /api/boards/{id}/columns/{col_id}               Remove column (must be empty)
GET    /api/boards/{id}/transitions                     List allowed transitions
POST   /api/boards/{id}/transitions                     Add transition
DELETE /api/boards/{id}/transitions/{transition_id}      Remove transition
```

### Content History

```
GET    /api/{entity_type}/{short_code}/history        Content version history
```

### Cascade Preview

*(Added 2026-07-15 — KAIROS-T-0051. Found by T-0041: DELETE cascades soft-deletion
to ALL transitive `parent`-edge descendants (see the authorization note above), but
that full set was only known AFTER the fact from the DELETE response. Clients
warning a user before a delete could show only the item's direct children. This
endpoint returns the AUTHORITATIVE set the delete WOULD cascade to, without
deleting.)*

```
GET    /api/{entity_type}/{short_code}/cascade-preview  Descendants a soft-delete would cascade to
```

- Side-effect-free read (open tenant-wide, KAIROS-A-0006), shape chosen over a
  `DELETE ?dry_run=true` so the mutating verb keeps a single meaning; matches the
  generic `{entity_type}` per-item read pattern (`history`, `relationships`,
  `metadata`).
- Computed by the SAME `kairos_core::items::cascade_descendants` BFS the soft-delete
  uses (single source of truth — no second traversal), reading the LIVE short codes
  of exactly those descendants. `cascaded_short_codes` is therefore identical to the
  `DeleteResponse` a subsequent DELETE returns (barring concurrent edits).
- 404 for an unknown family or short code (same as the other per-item reads);
  previewing an already soft-deleted item is a 404 (no live root).

Response:
```json
{
  "short_code": "S-0001",
  "cascade_count": 2,
  "cascaded_short_codes": ["I-0002", "T-0003"]
}
```

### Relationships

```
GET    /api/{entity_type}/{short_code}/relationships  All relationships for an item
POST   /api/relationships                             Create relationship (org admin)
DELETE /api/relationships/{id}                         Remove relationship (org admin)
```

### Metadata

```
GET    /api/{entity_type}/{short_code}/metadata       Get item's metadata values
PATCH  /api/{entity_type}/{short_code}/metadata       Set/update metadata values
GET    /api/metadata-definitions                       List available definitions (org admin)
POST   /api/metadata-definitions                       Create definition (org admin)
GET    /api/metadata-definitions/{id}                   Get definition (with enum options)
PATCH  /api/metadata-definitions/{id}                   Update definition (org admin)
DELETE /api/metadata-definitions/{id}                   Delete definition (org admin, fails if in use)
```

### Templates

```
GET    /api/templates                                List templates
GET    /api/templates/{id}                            Get template (content + associated metadata defs)
POST   /api/templates                                Create template (org admin)
PATCH  /api/templates/{id}                            Update template (org admin)
DELETE /api/templates/{id}                            Delete template (org admin)
```

### Teams

```
GET    /api/teams                                    List teams
GET    /api/teams/{id}                                Get team
POST   /api/teams                                    Create team
PATCH  /api/teams/{id}                                Update team
DELETE /api/teams/{id}                                Soft delete team
GET    /api/teams/{id}/members                         List team members
POST   /api/teams/{id}/members                         Add member to team
DELETE /api/teams/{id}/members/{user_id}                Remove member from team
```

### Delivery Streams

```
GET    /api/delivery-streams                         List delivery streams
GET    /api/delivery-streams/{id}                     Get delivery stream
POST   /api/delivery-streams                         Create delivery stream
PATCH  /api/delivery-streams/{id}                     Update delivery stream
DELETE /api/delivery-streams/{id}                     Soft delete delivery stream
GET    /api/delivery-streams/{id}/teams                Teams in this stream
POST   /api/delivery-streams/{id}/teams                Add team to stream
DELETE /api/delivery-streams/{id}/teams/{team_id}       Remove team from stream
```

### Board Authorization

```
GET    /api/boards/{id}/members                       List members + capabilities
POST   /api/boards/{id}/members                       Add member with capabilities
PATCH  /api/boards/{id}/members/{user_id}              Update capabilities (replace all)
DELETE /api/boards/{id}/members/{user_id}               Remove member (revoke all capabilities)
```

### Activity Log

```
GET    /api/activity?entity_id={id}                  Activity for a specific entity
GET    /api/activity?actor_id={user_id}               Activity by a specific user
GET    /api/activity?action={action}                   Activity by action type
GET    /api/activity?since={timestamp}                 Activity since timestamp
```

Params combinable. Supports `?limit=&offset=` pagination.

### Tenant Provisioning

```
POST   /api/admin/tenants                             Create tenant (creates org + schema + defaults;
                                                      optional initial_admin_external_id, defaults to caller,
                                                      inserted as organization_members role=admin)
GET    /api/admin/tenants                             List tenants
DELETE /api/admin/tenants/{slug}                       Remove tenant
```

Deployment-admin authority (added 2026-07-10, A-0010 addendum candidate): these routes bypass tenant
middleware but require authentication; authorized only for users whose OIDC `sub` is listed in
`KAIROS_DEPLOYMENT_ADMINS` (comma-separated). Unset/empty → always 403.

### Organization Membership

*(Added 2026-07-10 — day-zero bootstrap gap: org admins had no way to add members. Org-admin gated.)*

```
GET    /api/members                                   List members (user info + role), paginated
POST   /api/members                                   Add member {email, role} — resolves public.users by
                                                      email; users JIT-register at first login, so the
                                                      contract is "log in once first" (unknown email -> 404)
PATCH  /api/members/{user_id}                          Change role (demoting the last admin -> 422 LAST_ADMIN)
DELETE /api/members/{user_id}                          Remove membership (same last-admin guard)
```

### API Specification

```
GET    /api/openapi.json                              OpenAPI spec, generated from handlers/DTOs via utoipa
                                                      (Swagger UI mounted in dev builds; CI artifact per release for SDK generation)
```

---

## Event Push (WebSocket)

Added 2026-07-08 per KAIROS-A-0005 §5 — the UI never polls.

```
GET /ws/events                                        WebSocket upgrade; bearer token on connect; tenant-scoped
```

Thin change notifications (clients re-fetch details via REST):

```json
{
  "event": "item_transitioned",   // item_created | item_updated | item_transitioned |
                                  // item_deleted | relationship_changed | metadata_changed
  "entity_type": "task",
  "short_code": "T-0123",
  "board_id": "uuid",
  "column_id": "uuid",
  "actor": "user-uuid",
  "occurred_at": "2026-07-08T12:00:00Z"
}
```

- Optional subscription filter by `board_id` (client message after connect)
- Fan-out via PostgreSQL LISTEN/NOTIFY (`kairos_events`, tenant-tagged), emitted post-commit
- Best-effort delivery: no replay, no cross-connection ordering; clients reconcile by re-fetching on reconnect

---

## Unified Search (POST)

All complex querying goes through one endpoint. See KAIROS-A-0007.

```
POST /api/search
```

### Request

```json
{
  "q": "authentication",
  "filter": {
    "entity_type": ["task", "initiative"],
    "board_id": "uuid",
    "column_id": "uuid",
    "team_id": "uuid",
    "task_type": ["bug", "tech_debt"],
    "is_bucket": false,
    "metadata": {
      "priority": "critical",
      "component": "auth*"
    },
    "created_after": "2026-01-01T00:00:00Z",
    "created_before": "2026-03-01T00:00:00Z",
    "include_deleted": false
  },
  "traverse": {
    "from": {"short_code": "S-0001"},
    "relationships": ["parent"],
    "direction": "outbound",
    "depth": 3
  },
  "sort": {"field": "created_at", "order": "desc"},
  "limit": 25,
  "offset": 0
}
```

All fields optional. At least one of `q`, `filter`, or `traverse` required.

### Composition Examples

```json
// Full-text search
{"q": "authentication"}

// Structured filter: all critical bugs
{"filter": {"entity_type": ["task"], "task_type": ["bug"], "metadata": {"priority": "critical"}}}

// Graph: all tasks under strategy S-0001
{"traverse": {"from": {"short_code": "S-0001"}, "relationships": ["parent"], "direction": "outbound", "depth": 5}, "filter": {"entity_type": ["task"]}}

// Combined: search "auth" in tasks descended from S-0001
{"q": "auth", "traverse": {"from": {"short_code": "S-0001"}, "relationships": ["parent"], "direction": "outbound", "depth": 5}, "filter": {"entity_type": ["task"]}}

// Everything blocking initiative I-0003
{"traverse": {"from": {"short_code": "I-0003"}, "relationships": ["blocks"], "direction": "inbound", "depth": 1}}
```

### Response

```json
{
  "results": {
    "strategies": [...],
    "initiatives": [...],
    "tasks": [...],
    "documents": [...],
    "adrs": [...]
  },
  "total": 42,
  "limit": 25,
  "offset": 0
}
```

Empty type groups omitted. Each entity fully typed with all fields.

---

## Common Patterns

### Content Update (PATCH)

Request:
```json
{
  "title": "Updated title",
  "content": "Updated markdown content",
  "version": 3
}
```

Success: 200 with updated entity (new version number).
Conflict: 409 with current entity state.

### Transition (POST /transition)

Request:
```json
{
  "to_column_id": "uuid"
}
```

Success: 200 with updated entity (new column).
Invalid transition: 422.
No permission: 403.

### List Responses

```json
{
  "items": [...],
  "total": 42,
  "limit": 25,
  "offset": 0
}
```

### Error Responses

```json
{
  "error": {
    "code": "CONFLICT",
    "message": "Version mismatch: expected 3, current is 4",
    "details": {}
  }
}
```