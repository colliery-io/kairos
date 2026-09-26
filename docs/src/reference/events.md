# `GET /ws/events` — the WebSocket event channel

Kairos 0.3.0. This is the deployment's only push channel; the rest of the HTTP
surface is specified by OpenAPI (`GET /api/openapi.json`), which does not model
WebSockets, so the channel is described here instead. Implementation:
`crates/kairos-server/src/ws.rs`.

## Purpose

The server pushes **thin change notifications** — never content — whenever
something in the tenant changes. Clients react by re-fetching the affected
resource through the REST API.

## Connecting

```
GET /ws/events            # WebSocket upgrade
```

- **Authentication** is the standard stack — bearer token, then tenant
  resolution — evaluated *before* the protocol upgrade: a missing or
  invalid token is the usual `401`, a non-member the usual `403`, an
  unknown tenant the usual `404`.
- Tokens travel in the `Authorization: Bearer <jwt>` header. Browser
  `WebSocket` clients cannot set request headers, so the ONE supported
  fallback is the `?access_token=<jwt>` query parameter (promoted into
  the `Authorization` header ahead of the auth layer). Browser clients
  resolve their tenant via the `Host` subdomain (A-0005 §2).
- The connection is **bound to the resolved tenant** at upgrade time.
  Reads are open tenant-wide (A-0006), so every org member may
  subscribe; only that tenant's events are ever delivered.

## Server → client: thin events

One JSON object per WebSocket text message
(`kairos_client::types_events::ThinEvent`):

```json
{
  "event": "item_transitioned",
  "entity_type": "task",
  "short_code": "KAIROS-T-0042",
  "board_id": "6f1a1f9e-...",
  "column_id": "b2c3d4e5-...",
  "actor": "0a1b2c3d-...",
  "occurred_at": "2026-07-10T09:15:00.123456Z"
}
```

| Field | Meaning |
|---|---|
| `event` | One of the nine values in the table below |
| `entity_type` | `strategy` \| `initiative` \| `task` \| `document` \| `adr` |
| `short_code` | The affected item — re-fetch it via the REST API |
| `board_id` | The item's board (UUID); `null` for off-board items (documents, unplaced ADRs) |
| `column_id` | The item's (new) column (UUID); omitted when not applicable |
| `actor` | The acting user's id (UUID) |
| `occurred_at` | RFC 3339 timestamp |

### The `event` vocabulary

Nine values. A client that does not recognise one should re-fetch the item and
otherwise ignore it.

| `event` | Emitted when |
|---|---|
| `item_created` | An item was created |
| `item_updated` | An item's content changed (an edit, or a rollback) |
| `item_transitioned` | An item moved to another column of its own board |
| `item_moved` | A task moved to another delivery board. Emitted **twice**: once for the board it left (`board_id` = the source, no `column_id`) and once for the board it joined, so both boards' subscribers re-fetch |
| `item_deleted` | An item was soft-deleted (put away) |
| `item_restored` | An archived item was put back. **Distinct from `item_created`**: the item and its history existed all along, so a client that treats this as a create shows a new card carrying an old version number |
| `relationship_changed` | A relationship edge touching the item was added or removed |
| `metadata_changed` | The item's metadata values changed |
| `item_links_changed` | The item's forge links (branches, pull or merge requests) changed |

Events carry **no payloads**: fetch the new state through the REST
endpoints in the OpenAPI spec.

## Client → server: board filter

Optionally filter the stream to one board
(`kairos_client::types_events::SubscribeRequest`):

```json
{"subscribe": {"board_id": "6f1a1f9e-..."}}   // only this board's events
{"subscribe": {}}                              // clear the filter
```

## Delivery semantics (best-effort, A-0005 §5)

- Events are UI-freshness **hints**, not a durable stream: no replay, no
  ordering guarantee beyond per-connection FIFO.
- A socket that falls behind the server's broadcast buffer silently
  skips the lagged events.
- Clients reconcile by re-fetching through the REST API after any
  (re)connect.
- Fan-out is PostgreSQL `LISTEN/NOTIFY` on the `kairos_events` channel,
  emitted inside the mutating transaction, so PostgreSQL delivers the
  notification on commit and drops it on rollback.

## Related reading

- [Errors](errors.md) — the `401`, `403` and `404` this endpoint returns before
  the upgrade
- [REST API](rest-api.md) — the endpoints a client re-fetches through
- [Glossary](glossary.md)
