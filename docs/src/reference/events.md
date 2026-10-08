# `GET /ws/events` — the WebSocket event channel

This page describes Kairos 0.8.0.

This is the deployment's only push channel. OpenAPI
(`GET /api/openapi.json`) specifies the rest of the HTTP surface. OpenAPI does
not model WebSockets, so this page describes the channel instead.
Implementation: `crates/kairos-server/src/ws.rs`.

## Purpose

The server pushes **thin change notifications** — never content — whenever
something in the tenant changes. Clients react by re-fetching the affected
resource through the REST API.

## Connecting

```
GET /ws/events            # WebSocket upgrade
```

- **Authentication** is the standard stack — bearer token, then tenant
  resolution. The server evaluates it *before* the protocol upgrade. A
  missing or invalid token gets the usual `401`. A non-member gets the
  usual `403`. An unknown tenant gets the usual `404`.
- Tokens travel in the `Authorization: Bearer <jwt>` header. Browser
  `WebSocket` clients cannot set request headers. Thus the ONE supported
  fallback is the `?access_token=<jwt>` query parameter. The server
  promotes that parameter into the `Authorization` header ahead of the
  auth layer. Browser clients resolve their tenant via the `Host`
  subdomain (A-0005 §2).
- `access_token` is the one query parameter of the route. The server
  refuses each other parameter with `400 VALIDATION`. See
  [Errors](errors.md#a-query-parameter).
- The connection is **bound to the resolved tenant** at upgrade time.
  Each organization member can read tenant-wide (A-0006), so each
  organization member may subscribe. The server delivers only the events
  of that tenant.

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
| `event` | One of the ten values in the table below |
| `entity_type` | `strategy` \| `initiative` \| `task` \| `document` \| `adr` \| `repository` |
| `short_code` | The affected item — re-fetch it via the REST API. For `repository`, the slug of the repository |
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
| `relationship_changed` | A relationship edge touching the item was added or removed. An `impacts` link of the item was added or removed. The owner board of a document changed |
| `metadata_changed` | The item's metadata values changed |
| `item_links_changed` | The item's forge links (branches, pull or merge requests) changed |
| `code_index_build_changed` | A run of the code index builder started or ended for a repository. `entity_type` is `repository`, `short_code` is its slug, and `board_id` is `null`. Read its runs again: `GET /api/repositories/{slug}/code-indexes/builds` |

Events carry **no payloads**: fetch the new state through the REST
endpoints in the OpenAPI spec.

## Client → server: board filter

Optionally filter the stream to one board
(`kairos_client::types_events::SubscribeRequest`):

```json
{"subscribe": {"board_id": "6f1a1f9e-..."}}   // only this board's events
{"subscribe": {}}                              // clear the filter
```

### Events from a different board

The board filter also lets through some events about an item on a different
board. The server sends such an event when these three conditions are true:

- The `event` is `item_transitioned`, `item_moved`, `item_deleted` or
  `item_restored`.
- The item has a `blocks` edge to or from an item on the board of the filter.
- The item on the board of the filter is not put away.

These events change the "blocked by" and "blocks" counts on the cards of the
board. An example is a blocker on a different board that moves to a done
column. The `board_id` of the event is the board of the item that changed. It
is not the board of the filter. A client fetches its board again, as for each
other event.

The filter is not an authorization. A connection with no filter gets each
event of the tenant, and each member can read each item. Thus the filter
shows no event that the member cannot get without it.

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
