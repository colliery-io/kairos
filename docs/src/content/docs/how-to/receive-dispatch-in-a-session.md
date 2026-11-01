---
title: "Receive dispatch in a Claude Code session"
---

This page describes Kairos 0.19.0 and the `kairos` plugin.

Send the tasks of a board to a Claude Code session that runs now. A runner
is not necessary. The `kairos` plugin has a channel, `kairos-dispatch`. The
channel is a local listener. It checks each signed delivery of a
[dispatch rule](../dispatch-tasks-to-an-agent/) and pushes it into the
session. The session then works the task with the Kairos MCP tools.

**Before you start:**

- You installed the `kairos` plugin, and the session can reach Kairos over MCP.
- You have `configure_boards` on the board.
- The deployment has `KAIROS_SECRETS_KEY`, and
  `KAIROS_WEBHOOK_ALLOW_PRIVATE=true`: the listener is on your own machine.
- Channels are in research preview. A channel that is not on the allowlist
  needs `--dangerously-load-development-channels`, and that flag works only in
  an interactive session: `claude -p` ignores it.

## 1. Make the target

The listener is on `127.0.0.1` of the machine of the session. A Kairos server
in Docker Desktop on the same machine reaches it at `host.docker.internal`.
Nothing off the machine can reach it.

```sh
kairos boards dispatch add-target platform-delivery \
  --name "Claude session" --hook-url http://host.docker.internal:8790/
# → secret: kairos_whsec_3b9e…
```

For a Kairos server that runs on the machine itself, use
`http://127.0.0.1:8790/`.

## 2. Give the secret to the channel

The channel reads the config of a board from
`~/.claude/channels/kairos/<board>.json`, mode 600. `<board>` is the name of
the folder of the session (set `KAIROS_CHANNEL_BOARD` to choose another
name). Write it with the secret on standard input, so the secret is not in
your shell history:

```sh
pbpaste | ~/.claude/plugins/marketplaces/kairos/plugin/bin/kairos-channel \
  setup platform --port 8790
# → Wrote ~/.claude/channels/kairos/platform.json (mode 600). …
```

The script is in the plugin: `~/.claude/plugins/marketplaces/kairos` is the
clone of the marketplace, and `~/.claude/plugins/cache/kairos/kairos/<version>`
has the same file.

The channel never prints or logs the secret. It refuses a config that other
users can read.

## 3. Make a rule

```sh
kairos boards dispatch add-rule platform-delivery --column Todo --target 6f1a…
kairos boards dispatch add-rule platform-delivery --trigger unblocked --target 6f1a…
```

## 4. Start the session with the channel

```sh
cd ~/Boards/platform
claude --dangerously-load-development-channels plugin:kairos@kairos
```

Claude Code shows a warning for the development channel. The line under the
banner names `plugin:kairos@kairos`. Run `/mcp`: `kairos-dispatch` is
connected. Its instructions say where it listens, and that this session owns
the port.

With no config, the channel does not listen. A folder with Kairos wiring is
a folder that `kairos config show` binds: a `[[project]]` of `config.toml`.
There, the instructions say that
dispatch is off, and give the three steps of this page. In a folder with no wiring, the channel gives no instructions.

The channel is a server of the plugin, so it starts in each session of the
folder. A session without `--dangerously-load-development-channels` also
listens, but Claude Code drops the events of that session. The channel cannot
see the difference: the `initialize` request of Claude Code is the same with
and without the flag. Start each session of the folder with the flag.

## 5. Test it

Open **Admin**, then **Boards**, select the board, and select **Send test**
on the target. The session gets:

```text
<channel source="kairos-dispatch" event="ping" delivery="…">
A test delivery (ping) from the Kairos board settings arrived.
</channel>
```

A task that enters Todo arrives as:

```text
<channel source="kairos-dispatch" event="task.entered_column" code="PLAT-T-0042"
  board="platform-delivery" column="Todo" task_type="bug" work_class="support" agents_decide="true" rule_id="…">
Kairos dispatched PLAT-T-0042: it entered Todo on the board platform-delivery (lane support). Read it with get_item.
</channel>
```

The event has facts only: no title and no content. The session reads the task
with `get_item`.

A task in the support lane in the entry column is intake. For
intake, the session reads the task, calls `related_work`, and writes a proposal
with `append_note`. It applies the outcome only when the board lets an agent
decide (`agents_decide`). The instructions of the channel give these steps, so
the session does not need `/kairos:triage`. Another task runs
`kairos:implement`.

At the end of a task, the session pushes the branch of the task with
`git push -u origin <branch>`. It opens a draft pull request and writes it on
the task. Then it stops. It does not merge, release or tag. The git guard of
the plugin lets that push through, and asks for each other push.

## What the channel refuses

| Delivery | Answer | Pushed |
|---|---|---|
| Any delivery before the session ends the MCP handshake | 503 | no: Kairos tries again |
| A wrong `X-Kairos-Signature` | 401 | no |
| An event older than 24 hours, or more than 5 minutes in the future | 401 | no |
| `X-Kairos-Delivery` different from the `id` of the body | 401 | no |
| A delivery id that already arrived (a try again) | 200 | no |
| An event other than `ping`, `task.entered_column` and `task.unblocked` | 200 | no |

Events that arrive while the session is busy wait, and arrive together on its
next turn. The channel keeps the delivery ids in `<board>.seen.json` beside
the config, so a restart does not push a delivery again.

One session listens on a port. It writes `<board>.owner.json` with its process
id and its folder. A second session of the same board finds the port taken.
Its channel is off, and its instructions name the folder and the process id
of the session that listens.

## Related reading

- [Dispatch tasks to an agent runner](../dispatch-tasks-to-an-agent/)
- [Webhook deliveries](../../reference/webhooks/)
