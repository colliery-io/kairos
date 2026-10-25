---
title: "GET /ws/events — the WebSocket event channel"
---

This page describes Kairos 0.17.0.

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
  [Errors](../errors/#a-query-parameter).
- The connection is **bound to the resolved tenant** at upgrade time.
  Each organization member can read tenant-wide (A-0006), so each
  organization member may subscribe. The server delivers only the events
  of that tenant.
- The server checks the credential of the connection again every 5
  minutes. A session can end, an admin can revoke a key, and a token can
  expire. Then the server closes the connection with the close code
  `1008`. Connect again with a valid credential.

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
  "agent_key_id": "5e6f7a8b-...",
  "occurred_at": "2026-07-10T09:15:00.123456Z"
}
```

| Field | Meaning |
|---|---|
| `event` | One of the values in the tables below |
| `entity_type` | `strategy` \| `initiative` \| `task` \| `document` \| `adr` \| `repository` \| `board` \| `notification` |
| `short_code` | The affected item — re-fetch it via the REST API. For `repository` and `board`, the slug |
| `board_id` | The item's board (UUID); `null` for off-board items (documents, unplaced ADRs) |
| `column_id` | The item's (new) column (UUID); omitted when not applicable |
| `change` | What changed, for `claim_changed` and `board_configured`; the id of the note for `note_appended` and `note_promoted`; omitted for each other kind |
| `actor` | The acting user's id (UUID). For a change that an agent made with the agent key of a person, the person |
| `agent_key_id` | The agent key (UUID) of the change, when an agent made it with the agent key of a person; omitted otherwise |
| `occurred_at` | RFC 3339 timestamp |

### The `event` vocabulary

Fourteen values are for the items, the boards and the repositories. Two values
are for the notifications of a person (see [The events of a person](#the-events-of-a-person)).
A client that does not recognise a value should re-fetch the item and otherwise
ignore it.

| `event` | Emitted when |
|---|---|
| `item_created` | An item was created |
| `item_updated` | An item's content changed (an edit, or a rollback). Also a write of the live state of a task (`set_state`), a claim, a hand-off and a release |
| `item_transitioned` | An item moved to another column of its own board |
| `item_moved` | A task moved to another delivery board. Emitted **twice**: once for the board it left (`board_id` = the source, no `column_id`) and once for the board it joined, so both boards' subscribers re-fetch |
| `item_deleted` | An item was soft-deleted (put away). An archive cascade sends one event for each item that it puts away, each with the board of that item |
| `item_restored` | An archived item was put back. **Distinct from `item_created`**: the item and its history existed all along, so a client that treats this as a create shows a new card carrying an old version number. A restore puts back only the named item, so it sends one event |
| `relationship_changed` | A relationship edge touching the item was added or removed. An `impacts` link of the item was added or removed. The owner board of a document changed |
| `metadata_changed` | The item's metadata values changed |
| `item_links_changed` | The item's forge links (branches, pull or merge requests) changed |
| `code_index_build_changed` | A run of the code index builder started or ended for a repository. `entity_type` is `repository`, `short_code` is its slug, and `board_id` is `null`. Read its runs again: `GET /api/repositories/{slug}/code-indexes/builds` |
| `task_state_changed` | The live state of a task changed. A write of the state sends it. A hand-off or the end of a claim that clears the next step also sends it |
| `task_cancelled` | A person cancelled a task. `column_id` is the done column of the board |
| `claim_changed` | The claim of a task changed. `change` tells what changed (see below) |
| `board_configured` | The configuration of a board changed: a column, a transition, a flag or a setting. `entity_type` is `board`, `short_code` is the slug of the board, `board_id` is the board. `change` names the setting (see below). Read the board again |
| `note_appended` | A person or an agent added a note to the item. `change` is the id of the note (`n-3`). A note writes no version of the content: read the item again for its `notes` |
| `note_promoted` | A note of the item became a draft ADR or document. `change` is the id of the note |

### The precise kinds

The kinds `task_state_changed`, `task_cancelled`, `claim_changed`,
`note_appended` and `note_promoted` come **in addition** to the kind that
Kairos sent before:

| Write | Events |
|---|---|
| A write of the state | `item_updated`, `task_state_changed` |
| A claim | `item_updated`, `claim_changed` (`taken`) |
| A hand-off | `item_updated`, `claim_changed` (`handed_off`), and `task_state_changed` when the next step is cleared |
| A release | `item_updated`, `claim_changed` (`released`), and `task_state_changed` when the next step is cleared |
| A cancel | `item_transitioned`, `task_cancelled`, and `claim_changed` (`ended`) when the task had a claim |
| A move into Active by a person | `item_transitioned`, `claim_changed` (`taken`) |
| A move out of Active | `item_transitioned`, `claim_changed` (`ended`) |
| A note | `item_updated`, `note_appended` (the id of the note) |
| A promote of a note | `item_updated`, `note_promoted` (the id of the note) for the item of the note, and `item_created` for the new item |

Thus a client that knows only the older kinds keeps working. A client that
must know what changed reads the precise kind. The events of one write come
in one transaction. Their order is not part of the contract.

The values of `change` for `claim_changed`:

| `change` | Meaning |
|---|---|
| `taken` | A person took the claim: a claim, or a move into Active |
| `handed_off` | A hand-off gave the claim to a person |
| `released` | A release ended the claim. The task stays in its column, free |
| `ended` | The claim ended for a different reason: a move out of Active, a cancel, an archive, a change of a column flag, or a person who left the organization |

The values of `change` for `board_configured` name the setting:

| `change` | The setting |
|---|---|
| `column_add`, `column_remove`, `column_rename`, `columns_reorder` | The columns |
| `column_done`, `column_claims`, `column_keeps_claims`, `column_wip_limit` | A flag or the WIP limit of a column |
| `transition_add`, `transition_remove` | The transitions |
| `board_settings` | The name or the slug of the board |
| `agents_decide_triage` | The triage setting of the board |

A client that does not know a value reads the board again.

Events carry **no payloads**: fetch the new state through the REST
endpoints in the OpenAPI spec.

### The events of a person

A notification is for one person (KAIROS-T-0381). Its 2 events go only to the
connections of that person, in the tenant of the notification. A connection of
a different person never gets them. One write can tell many persons, for
example the team of a board. Then the server sends one event, and each person
of the write gets it.

| `event` | Emitted when |
|---|---|
| `notification_created` | Kairos made a notification for you. `short_code` is the code of the item of the notification |
| `notifications_read` | You marked one or more of your notifications as read with your login. A read with an agent key sends no event, because your bell does not change. `short_code` is empty |

For the two events, `entity_type` is `notification` and `board_id` is `null`.
Each filter lets them through: a board filter too (see
[Client → server: filters](#client--server-filters)). The text of the
notification is not in the event: read your notifications through the REST
API.

The `kind` of a notification says what happened. The blocker kinds:

| `kind` | Kairos makes it when |
|---|---|
| `blocker_cleared` | An open blocker of your item stops blocking it. The blocker is done, archived or purged, or a person removes the `blocks` link |
| `blocked_again` | A done blocker of your item leaves the done column (KAIROS-T-0568) |
| `blocker_cancelled` | A person cancels a blocker of your item. The blocker still blocks (KAIROS-T-0567) |

"Your item" is a task that you have the claim on, or a strategy or initiative
that you own. For `blocked_again`, the person who created the item gets it
when nobody has the claim or owns it. For `blocker_cancelled`, the person who
created the item gets it too. A cancel does not resolve a blocker, so it sends no
`blocker_cleared`.

The claim kinds (KAIROS-T-0531):

| `kind` | Kairos makes it when |
|---|---|
| `claim_dropped` | A person leaves the organization, and their claim on a task of the board of your team ends. The task is free in its column |
| `claim_stale` | A claim has no write and no state change for `KAIROS_STALE_CLAIM_DAYS` days (default 14). The holder gets it, and the owner of the initiative of the task |

Kairos does not end a stale claim. It sends `claim_stale` once for each quiet
period. A write to the task or to its state starts a new period.

The key kind (KAIROS-T-0540):

| `kind` | Kairos makes it when |
|---|---|
| `key_expiring` | A key expires in 7 days or less. The person of an agent key gets it. For a key of a service account, each organization admin gets it |

Each hour, a periodic pass finds these keys. Kairos sends `key_expiring` one
time for each key. It sends none for a revoked key or a key with no expiry.
It sends none for the old key of a rotation.

The text names the key and `kairos keys rotate`. The notification has no item, so `short_code` is empty.

The proposal kind (KAIROS-T-0558):

| `kind` | Kairos makes it when |
|---|---|
| `proposal_decided` | A person confirms or rejects an edge proposal. The proposer gets it. The text names the edge, the state and the decider. `short_code` is the code of the source of the edge |

The [MCP tools reference](../mcp-tools/#your-notifications) lists each kind.

## Client → server: filters

Optionally filter the stream
(`kairos_client::types_events::SubscribeRequest`):

```json
{"subscribe": {"board_id": "6f1a1f9e-..."}}   // this board, and your events
{"subscribe": {"person_only": true}}           // only your events
{"subscribe": {}}                              // clear the filter
```

- `board_id` lets through the events of one board. It also lets through
  [the events of a person](#the-events-of-a-person). Thus one connection
  serves the bell and the board view of a browser tab.
- `person_only` lets through only the events of the person. The bell of a
  page with no live view uses it. With `person_only`, the server ignores
  `board_id`.

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
event of the tenant, and each member can read each item. The one rule that is
an authorization is the rule of [the events of a person](#the-events-of-a-person). Thus the filter
shows no event that the member cannot get without it.

## Delivery semantics (best-effort, A-0005 §5)

- Events are UI-freshness **hints**, not a durable stream: no replay, no
  ordering guarantee beyond per-connection FIFO.
- A socket that falls behind the server's broadcast buffer skips the
  lagged events. The server then sends one frame `{"event":"lagged"}`.
  That frame is not a thin event: it names no item. Read again what you
  show.
- Clients reconcile by re-fetching through the REST API after any
  (re)connect, and after the frame `lagged`.
- Reconnect with a backoff and a random jitter. The Kairos GUI waits 1
  second to 15 seconds, and adds or removes up to 50 % of that time. Thus
  the clients do not all connect again in the same second after a restart
  of the server.
- An event comes at most about 100 ms after the commit of its write. See
  [How the server sends the events](#how-the-server-sends-the-events).

## How the server sends the events

A write puts its events into the table `public.event_outbox`. It does this
in its own transaction. Thus a write that rolls back sends no event.

Each server process has one emitter. The emitter is part of the owner of
the `LISTEN` connection. Every 100 ms, the emitter does these steps in one
transaction:

1. It takes the unsent rows in order, with `FOR UPDATE SKIP LOCKED`.
2. It finds the boards across the `blocks` edges, with one query for each
   tenant.
3. It sends the events with PostgreSQL `NOTIFY` on the channel
   `kairos_events`. One `NOTIFY` holds a JSON array of events, under 8000
   bytes.
4. It marks the rows as sent.

Every 10 seconds, the emitter deletes the rows that it sent more than one
minute ago.

Each server listens on the channel. It sends each event to the
connections of the tenant of the event.

The design has these results:

- A write does not send `NOTIFY`. Thus the writes of different tenants do
  not wait for one database lock at commit.
- With two server processes, `SKIP LOCKED` gives each row to one emitter.
  The `NOTIFY` of that emitter reaches the listeners of the two processes.
  Thus each connection gets each event one time.
- A process can stop after the commit of a write and before the
  `NOTIFY`. The row stays unsent. The next pass of an emitter sends it.
  Thus the event comes late, but it does not go.
- An emitter can stop before its commit. Then PostgreSQL rolls back its
  marks and its `NOTIFY`. The next pass sends the rows again.
- An event of a person goes to many persons in one event. The event holds
  a list of at most 100 persons. A longer list makes more events.

## Related reading

- [Errors](../errors/) — the `401`, `403` and `404` this endpoint returns before
  the upgrade
- [REST API](../rest-api/) — the endpoints a client re-fetches through
- [Glossary](../glossary/)
