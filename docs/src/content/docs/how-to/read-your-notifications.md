---
title: "Read your notifications"
---

This page describes Kairos 0.18.1.

Kairos tells you about events of your work. The bell in the top bar of the
web app shows the number of notifications that you did not read.

You get a notification when one of these events occurs:

- A person gives you the claim of a task (a hand-off).
- An item that blocks your claimed task goes into a done column, or is
  archived.
- A person cancels a task that you created. The text has the reason.
- A person writes your user name in a note or in the content of an item,
  for example `@robin`.
- A bug with the priority P0 lands on the board of your team.
- Kairos turns off a webhook target that you own, after failed deliveries.
- An organization admin revokes an agent key of yours. The text names the key.
- An agent key of yours expires in 7 days or less. An organization admin
  also gets this notification for a key of a service account. The text
  names the key and the command `kairos keys rotate`. Kairos sends it one
  time for each key.
- A person confirms or rejects an edge proposal that you made. The text
  names the edge and the state.

You get no notification for a change that you made.

**Before you start:** you are a member of the organization.

## 1. Open the list

1. Select the bell in the top bar. The drawer opens.
2. Read the list. The newest notification is at the top. An unread
   notification has a mark.
3. Select the code of an item to open the item.

The count of the bell changes live. You do not have to load the page again.

## 2. Mark notifications read

- To mark one notification read, select **Mark read** on its row.
- To mark all of them read, select **Mark all read** at the top of the
  drawer. Kairos marks the notifications of the list. A notification that
  came after the list stays unread.

The count of the bell goes down. Your agent gets the same notifications.
Each agent key keeps its own read state. A notification that you mark read
stays unread for your agent. A notification that your agent marks read
stays unread in your bell. See
[Read notifications as an agent](../read-notifications-as-an-agent/).

## 3. Turn a kind off

1. In the drawer, select **Settings**. The **Notifications** page opens.
2. Turn off each kind that you do not want.
3. Select **Save**.

Kairos makes no notification of a kind that you turned off. The page also
lists your notifications, with **Mark all read**.

## 4. Read older notifications

The drawer and the **Notifications** page show up to 100 unread
notifications and the 20 newest read ones. To see more, open the
**Notifications** page:

1. Select **Show older** below the list. Kairos adds the next page.
2. Do this again until the button goes away. Then the page shows each
   notification.

The older unread notifications come first, then the older read ones. When
you mark a notification read, the page shows the first page again.

With REST, `GET /api/me/notifications?before=<id>` gives the unread
notifications older than `<id>`. `?read_before=<id>` gives the read
notifications that you read before `<id>`, 100 at most.

You can do the same with REST: `GET` and `PUT /api/me/notification-settings`.

## How long a notification stays

Kairos deletes an unread notification after
`KAIROS_NOTIFICATIONS_UNREAD_DAYS` days (180). It deletes a read
notification after `KAIROS_NOTIFICATIONS_READ_DAYS` days (30). See
[Configuration](../../reference/configuration/#retention).

## Related reading

- [Read notifications as an agent](../read-notifications-as-an-agent/).
- [MCP tools: Your notifications](../../reference/mcp-tools/#your-notifications):
  the kinds of notification.
- [Events](../../reference/events/#the-events-of-a-person): the live events of
  the bell.
