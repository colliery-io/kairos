---
title: "Read notifications as an agent"
---

This page describes Kairos 0.17.0.

An agent that works with your agent key gets your notifications. For
example, the agent learns that a person gave it the claim of a task. Kairos
does not start a session. The agent reads the notifications when it calls a
tool.

**Before you start:**

- The agent connects over MCP with your agent key. See
  [Give an agent your key](../give-an-agent-your-key/).
- A service account gets no notifications. Use an agent key.

## 1. Read the block at the end of each answer

While you have unread notifications, each answer of a Kairos tool ends with
a block. The block has the 5 newest unread notifications. With more than
5, the heading says `5+ unread`:

```text
## Your notifications (5+ unread)
- 2026-10-12 09:15Z · hand_off · ACME-T-0012: Robin gave you the claim on ACME-T-0012: Fix the login
- …
More: call get_events
get_events gives each with its id; mark them read with mark_events_read when you have acted on them.
```

Each line has the time, the kind, the item and the text. `get_events` gives
the ids. The block
stays on each answer until the agent marks the notifications read. A
refusal has no block.

## 2. List all of them

Call the tool `get_events`. It gives each unread notification first, newest
first, then the 20 newest read notifications. It shows 100 unread
notifications at most. The answer then gives a `before` value. Send it to
`get_events` to get the next page.

The answer ends with the `up_to` value: the id of the newest unread
notification.

With REST, use `GET /api/me/notifications`. Add `?before=<id>` for the next
page. Add `?read_before=<id>` for the read notifications that the key read
before `<id>`.

## 3. Act, then mark them read

Act on each notification that changes the work. For example, start the task
of a cleared blocker, or put a P0 bug first. Then mark the notifications
read with `mark_events_read`:

```json
{ "ids": ["6f1a1f9e-…", "2b7c…"] }
```

Or mark each notification that you read. Send `up_to`: the id of the newest
notification that you read.

```json
{ "all": true, "up_to": "6f1a1f9e-…" }
```

Kairos marks that notification and each older one. A notification that came
after your list stays unread. Kairos refuses `all` without `up_to`.

The answer is one line:

```text
Marked 2 notification(s) read. You have 5 unread.
```

Kairos refuses an id that is not one of your notifications, and changes
nothing.

With REST, use `POST /api/me/notifications/read` with the same body.

Do not mark a notification read before the agent acts on it.

You and the agent get the same notifications. Each agent key keeps its own
read state. When the agent marks a notification read, the bell of the web
app still shows it unread. You mark it read yourself.

## The skills of the plugin

The `implement` and `triage` skills of the Kairos plugin read the block.
They call `get_events`, and mark the notifications read after they act.
They send `all: true` with `up_to`, or the ids.

## Related reading

- [Read your notifications](../read-your-notifications/): the bell and the
  settings.
- [MCP tools: Your notifications](../../reference/mcp-tools/#your-notifications):
  `get_events` and `mark_events_read`.
- [Connect over MCP](../connect-over-mcp/).
