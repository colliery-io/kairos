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
kairos tasks move DEMO-T-0041 --to-board platform-delivery
```

The task lands in the target board's **entry column** — its lowest-position
column — and takes on that board's team. It does not keep its column: the
column it was in belonged to the old board's workflow.

The task keeps its repository. The move does not look at the repository.

## When it is refused

Every one of these is a 422 (a tool error over MCP) carrying a stable code —
statuses and `details` shapes are in [Errors](../reference/errors.md):

| Code | What to do |
|---|---|
| `SAME_BOARD` | It is already there. |
| `NOT_DELIVERY_BOARD` | The target is a strategy or initiative board. Only delivery boards accept moves. |
| `NO_ENTRY_COLUMN` | The target board has no columns. Give it one — [Set up a board](set-up-a-board.md). |
| `ITEM_NOT_ON_BOARD` | The item has no board placement, so there is nothing to move. Documents never do. |

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

An agent that has no capability on the target board can read the task. Each
write gets `FORBIDDEN`.

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

   ```sh
   kairos tasks create --board platform-delivery \
     --title "Bulk invoice export endpoint"
   ```

   The GUI has no control for a request to a board that you do not manage.

3. If you know where the code is, add the repository. The repository is
   optional, and it does not choose the board.

   ```sh
   kairos tasks create --board platform-delivery --repo payments-api \
     --title "Bulk invoice export endpoint"
   ```

4. If the request blocks your work, link it to your task with a `blocks` edge.
   You created the request, so you can write `parent` and `blocks` edges from
   it.

The request arrives as support work:

- It goes to the entry column of the board.
- Its work class is `support`, for each task type.
- It has the team of the board.

You cannot move the request, edit it or delete it. The team that receives it
does the triage. A member of that team can change the work class to `planned`
with
[`POST /api/tasks/{short_code}/work-class`](../reference/rest/work-items.md).

Do not create a task to ask for the review of a pull request. The git provider
manages the pull request.

### When a request is refused

| Refusal | Cause | What to do |
|---|---|---|
| 403 `FORBIDDEN` | You sent the work class `planned`. | Remove the work class, or send `support`. |
| 403 `FORBIDDEN` | You named a column that is not the entry column. | Remove the column. |
| 403 `FORBIDDEN` | The board is not a delivery board. | Name the delivery board of the team. |
| 422 `VALIDATION` | You named a repository and no board or team. The message is `name a board or a team (board_id or team_id); a repository is a link and does not choose a board`. | Name a board or a team. |
| 422 `VALIDATION` | You named a team that is not the team of the board. | Name the team of the board, or no team. |

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
- [Teams and boards](../explanation/teams-and-boards.md#moving-work-between-boards)
- [Capabilities and access](../explanation/capabilities-and-access.md)
- [Repositories as execution scope](../explanation/repositories-as-execution-scope.md#requests-between-teams-are-support-work)
  — why a request is support work
- [Wind down a team](wind-down-a-team.md) — the usual reason for a batch of
  moves
