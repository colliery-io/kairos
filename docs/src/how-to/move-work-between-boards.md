# Move work between boards

Re-file a task that is on the wrong delivery board — a mis-triaged ticket, work
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

## When it is refused

Every one of these is a 422 (a tool error over MCP) carrying a stable code —
statuses and `details` shapes are in [Errors](../reference/errors.md):

| Code | What to do |
|---|---|
| `SAME_BOARD` | It is already there. |
| `NOT_DELIVERY_BOARD` | The target is a strategy or initiative board. Only delivery boards accept moves. |
| `NO_ENTRY_COLUMN` | The target board has no columns. Give it one — [Set up a board](set-up-a-board.md). |
| `REPOSITORY_OWNER_MISMATCH` | See below. |
| `ITEM_NOT_ON_BOARD` | The item has no board placement, so there is nothing to move. Documents never do. |

A 403 means the capability, and the message names which side is missing it.

### A task bound to a repository follows its repository

A task bound to a repository may **only** move to that repository's owning
team's board. Nowhere else, whoever you are. The refusal is
`REPOSITORY_OWNER_MISMATCH` and it names the repository and the board the task
may go to.

This bites hardest after a re-home, because re-homing a repository retargets
everything bound to it, invisibly, until someone tries to move a card. Either
way out works:

- **Send the task after its repository:** move it to the repository's new
  owning team's board, which is the only target the guard allows.
- **Unbind it first**, and then it moves like any other task:

  ```sh
  kairos repos unbind DEMO-T-0041
  kairos tasks move DEMO-T-0041 --to-board platform-delivery
  ```

- **Or re-home the repository** so that the board you want is its owner's:
  `kairos repos update payments-api --team platform`.

Why the binding is this strict is [Repositories as execution
scope](../explanation/repositories-as-execution-scope.md#one-owning-team-at-most-one-repository-per-task).

### Agents scoped to the repository

After a move, an agent whose queue is a repository on the old team's board will
still **find** the work — the queue query follows the repository — and will fail
every write on it, because its capabilities came from a team that no longer owns
the repository. Move the service account's team membership across too:
[Give an agent machine access](give-an-agent-machine-access.md).

## What to do instead

If you do not hold `manage_tasks` on both boards, do not go looking for a
workaround; there is a supported route for each case.

- **You want to ask another team for something.** File it against their
  repository. It lands in their **Backlog** and nothing else, and their triage
  decides. Every member can do this already — `kairos whoami --json` lists
  `file_backlog` under `implicit`
  ([Capabilities](../reference/capabilities.md)), so there is nothing to
  request.

  ```sh
  kairos tasks create --repo payments-api --title "Bulk invoice export endpoint"
  ```

  You cannot then move it out of Backlog, edit it or delete it. You *may* link
  it to your own work with a `parent` or `blocks` edge.

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
- [Wind down a team](wind-down-a-team.md) — the usual reason for a batch of
  moves
