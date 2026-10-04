# Move work between boards

Re-file a task that is on the wrong delivery board — a mis-triaged task, work
absorbed by another team, a reorg.

**Before you start:** you need `manage_tasks` on **both** the task's current
board and the target board, or org admin. This is the requirement that stops most
attempts ([why](../explanation/teams-and-boards.md#moving-work-between-boards)).
If you do not hold it on both sides, go straight to
[What to do instead](#what-to-do-instead).

Only tasks move, and only between **delivery** boards.

## Move it

| Surface | How |
|---|---|
| CLI | `kairos tasks move <CODE> --to-board <slug\|uuid>` |
| API | `POST /api/tasks/{short_code}/move` with `{"board": "<slug\|uuid>"}` |
| MCP | `move_item {short_code, to_board}` |
| GUI | the **Board** select on the item page's Board panel |

```sh
kairos tasks move MOBILE-T-0001 --to-board platform-delivery
```

The task lands in the target board's **entry column** — its lowest-position
column — and takes on that board's team. It does not keep its column: the
column it was in belonged to the old board's workflow.

The task keeps its repository. The move does not look at the repository.

## Give it a code of the new board

A move keeps the short code of the task. To give the task the next code of
the target board, add the rename option:

| Surface | How |
|---|---|
| CLI | `kairos tasks move <CODE> --to-board <slug\|uuid> --rename` |
| API | `POST /api/tasks/{short_code}/move` with `{"board": "<slug\|uuid>", "rename": true}` |
| MCP | `move_item {short_code, to_board, rename: true}` |
| GUI | the switch **Give it a code of the new board**, then **Move board** |

```sh
kairos tasks move COLLIERY-T-0100 --to-board skadi --rename
```

A rename does these steps in one transaction:

1. The task gets the next code of the target board, for example
   `SKADI-T-0001`.
2. Kairos retires the old code and does not issue it again. A read with the
   old code finds the task and names the new code
   ([Errors](../reference/errors.md)).
3. Each reference to the old code in the title and the content of each item
   changes to the new code. Each item that changes gets a new version by you.
   This occurs one time: a later edit that writes the old code stays as it is.
4. The activity log records the rename: the old code, the new code, and who
   did it. `get_history` shows it under **Renames**.

These references do not change:

- A code in a URL, for example `https://example.com/COLLIERY-T-0100.md`.
- A code in a path, for example `docs/COLLIERY-T-0100/notes`.
- A file name, for example `COLLIERY-T-0100.md`.
- The footer of an item from the Metis importer. The footer records the
  Metis code.

The Kairos link `/items/COLLIERY-T-0100` changes, because it names the item.

Kairos refuses a rename to a board whose prefix the code has already
(`RENAME_NOT_NEEDED`). Then the task does not move. Do the move with no
rename.

A document can get a code of its new owner board too: `move_item` with
`to_board` and `rename: true`, or `kairos documents move <CODE> --to-board
<BOARD> --rename`. In the GUI, use the **Owner board** panel of the document.
Select the new owner board. Set the switch **Give it a code of the new
board**. Then select **Set owner board**. A rename of a document needs a new
board.

The Metis importer and a rename do not agree about a path. A rename keeps a
code in a path. The importer changes a code in a path, for example
`/FIDIUS-S-0001/`. See [Short codes](../reference/short-codes.md).

## When it is refused

Every one of these is a 422 (a tool error over MCP) carrying a stable code —
statuses and `details` shapes are in [Errors](../reference/errors.md):

| Code | What to do |
|---|---|
| `SAME_BOARD` | It is already there. |
| `NOT_DELIVERY_BOARD` | The target is a strategy or initiative board. Only delivery boards accept moves. |
| `NO_ENTRY_COLUMN` | The target board has no columns. Give it one — [Set up a board](set-up-a-board.md). |
| `ITEM_NOT_ON_BOARD` | The item has no board placement, so there is nothing to move. Documents never do. |
| `RENAME_NOT_NEEDED` | The code has the prefix of the target board already. Do the move with no rename. |

A 403 means the capability, and the message names which side is missing it.

### Agents that work the old board

The queue of an agent is the board of its team, with a filter for the
repository of the checkout. After a move, the task is not on that board, so the
agent does not find the task in its queue.

- If the agent must continue with the task, add its service account to the team
  of the target board:
  [Give an agent machine access](give-an-agent-machine-access.md).
- If the agent only needs to find the task, use `search` with the repository
  filter. The filter finds the tasks that link to a repository on all boards.

An agent that has no capability on the target board can read the task. If the
agent created the task, it can edit the task, link it and archive it. It
cannot move the task. If the agent did not create the task, each write gets
`FORBIDDEN`.

## Send a request to a different team

Use a request when you want work from a team and you do not manage its board.
Teams request work of each other. No team pushes work to a different team.

Every member of the organization can send a request. The capability is
`file_backlog`, and `kairos whoami --json` lists it under `implicit`
([Capabilities](../reference/capabilities.md)). No grant is necessary.

1. Find the delivery board of the team. The default slug is
   `<team-slug>-delivery`, and `kairos boards list` shows each board.

2. Create a task on that board. Name the board, or name the team.

   | Surface | How |
   |---|---|
   | CLI | `kairos tasks create --board <slug\|uuid> --title <title>`, or `--team <uuid>` in place of `--board` |
   | API | `POST /api/tasks` with `{"board_id": "<slug\|uuid>", "title": "…"}`, or `team_id` in place of `board_id` |
   | MCP | `create_item {item_type: "task", board, title}` |
   | GUI | **New request** on the delivery board of the team |

   ```sh
   kairos tasks create --board platform-delivery \
     --title "Bulk invoice export endpoint"
   ```

   In the GUI:

   1. Open the delivery board of the team.
   2. Click **New request**. A board that you manage shows **New task**.
   3. Type a title.
   4. Type the content. Say what you need.
   5. Select the task type.
   6. Click **Send request**.

   The dialog names the team, the entry column and the lane. It has no
   control for the lane or the column. A notice on the board gives the short
   code of the request.

3. If you know where the code is, add the repository. The repository is
   optional, and it does not choose the board.

   ```sh
   kairos tasks create --board platform-delivery --repo payments-api \
     --title "Bulk invoice export endpoint"
   ```

   In the GUI, select the repository in the dialog before you send the
   request. The repositories of the team of the board are first in the list.

4. If the request blocks your work, link it to your task with a `blocks` edge.
   You created the request, so you can write an edge from it or to it.

The request arrives as support work:

- It goes to the entry column of the board.
- Its work class is `support`, for each task type.
- It has the team of the board.

You created the request, so you can edit it and archive it. You cannot move
it to a different column, lane or board. The team that receives it does the
triage. A member of that team can change the work class to `planned`
with
[`POST /api/tasks/{short_code}/work-class`](../reference/rest/work-items.md).

Do not create a task to ask for the review of a pull request. The git provider
manages the pull request.

### When a request is refused

The GUI shows the message of the server in the dialog. The dialog stays open.

| Refusal | Cause | What to do |
|---|---|---|
| 403 `FORBIDDEN` | You sent the work class `planned`. | Remove the work class, or send `support`. |
| 403 `FORBIDDEN` | You named a column that is not the entry column. | Remove the column. |
| 403 `FORBIDDEN` | The board is not a delivery board. | Name the delivery board of the team. |
| 422 `VALIDATION` | You named a repository and no board or team. The message starts with `The request has no board_id and no team_id.` | Name a board or a team. |
| 422 `VALIDATION` | You named a team that is not the team of the board. | Name the team of the board, or no team. |

## Give a document to a different owner board

A document is on no board. It has an owner board, which gives the right to
edit it. To change the owner board, you need `manage_documents` on two boards:

- the board that owns the document now
- the new owner board

An organization admin can change the owner of each document. The creator of a
document gets no right to change its owner.

1. Change the owner board with one of these commands:

   ```sh
   kairos documents move ACME-D-0004 --to-board web-delivery
   ```

   An agent calls `move_item` with the short code and `to_board`.

2. Make sure that the document shows the new board:

   ```sh
   kairos documents get ACME-D-0004
   ```

The document gets no column, and no board view shows it. Its `supports` edges
and its `impacts` links do not change.

Each document has an owner board, so you cannot remove it. You can only
change it to a different board.

## What to do instead

If you do not hold `manage_tasks` on both boards, do not go looking for a
workaround; there is a supported route for each case.

- **You want to ask another team for something.**
  [Send a request to a different team](#send-a-request-to-a-different-team).

- **You want the capability.** It is granted per board, through
  [`POST /api/boards/{id}/members`](../reference/rest/boards-and-teams.md) or
  the GUI, by someone who administers that board. See
  [Capabilities](../reference/capabilities.md).

- **The work is finished, not misfiled.** Archive it instead:
  [Find archived work](find-archived-work.md).

## Related

- [`POST /api/tasks/{short_code}/move`](../reference/rest/work-items.md)
- [`move_item`](../reference/mcp-tools.md#move_item)
- [The owner of a document](../reference/capabilities.md#the-owner-of-a-document)
- [Teams and boards](../explanation/teams-and-boards.md#moving-work-between-boards)
- [Capabilities and access](../explanation/capabilities-and-access.md)
- [Repositories as execution scope](../explanation/repositories-as-execution-scope.md#requests-between-teams-are-support-work)
  — why a request is support work
- [Wind down a team](wind-down-a-team.md) — the usual reason for a batch of
  moves
