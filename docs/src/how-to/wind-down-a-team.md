# Wind down a team

Disband a team whose work is finished or absorbed elsewhere, and get its delete
to go through.

**Before you start:** you are an org admin. Deleting a team is org-admin only.

Kairos refuses the delete while anything still points at the team, and the
refusal names what. There are exactly **two** guards, they fire in this order,
and clearing them is the whole job:

1. The team must own **no repositories** — 409.
2. Its delivery board must hold **no live cards** — 422 `BOARD_NOT_EMPTY`.

Work through them in that order; the second refusal will not appear until the
first is clear.

## 1. Try the delete and read the refusal

```sh
kairos teams delete <team-id> --confirm
```

```text
409 — team "Mobile" still owns 1 repository: [payments-api];
re-home them before removing the team
```

The message lists the repositories by slug, and `details.repositories` carries
the same list.

## 2. Re-home or retire the repositories

Re-home it to the surviving team:

```sh
kairos repos update payments-api --team platform
```

The repository's tasks are untouched and re-checked on their next write. Or
retire the repository entirely, which is itself refused with 409 while any task
or webhook connection still references it:

```sh
kairos repos delete payments-api --confirm
```

**Re-homing retargets everything bound to that repository.** A task on the old
board bound to it can now only move to the new owner's board, and an agent
scoped to it keeps finding its queue while failing every write. Both are
covered in [Move work between boards](move-work-between-boards.md#a-task-bound-to-a-repository-follows-its-repository).

## 3. Clear the board

```sh
kairos teams delete <team-id> --confirm
```

```text
422 BOARD_NOT_EMPTY — team "Mobile"'s delivery board still holds 2 live card(s):
[DEMO-T-0041, DEMO-T-0043]; move them to another board
(POST /api/tasks/{code}/move) or delete them, then retry
```

The refusal names the cards — up to twenty of them — so this is a worklist, not
a puzzle. Two ways to clear each one, and the choice matters:

- **The work continues somewhere else** → move it:

  ```sh
  kairos tasks move DEMO-T-0041 --to-board platform-delivery
  ```

  See [Move work between boards](move-work-between-boards.md), which is also
  where the `manage_tasks`-on-both-boards requirement and the
  repository-binding guard live.

- **The work is finished** → archive it:

  ```sh
  kairos tasks delete DEMO-T-0043 --confirm
  ```

  Despite the verb this is a soft delete, and **archived cards do not hold the
  guard open**, so this is the normal way to empty a board at the end of a
  quarter ([Archiving](../explanation/archiving.md)).

  **It cascades to the card's children.** A parent named in the refusal takes its
  whole subtree with it, so check what a card holds before archiving it rather
  than after — restore does not un-cascade, and putting the subtree back is one
  item at a time.

### Decide before you archive

Archiving is easy to undo *while the board exists*. Once the team goes, its
delivery board is soft-deleted with it and answers 404, so an archived card that
was on that board can no longer be restored — a restore is refused with
`RESTORE_BLOCKED` naming the missing board. The activity log still records that
the work existed and who did what to it, either way.

So: anything somebody might plausibly pick back up should be **moved**, not
archived. See [Find archived work](find-archived-work.md#when-a-restore-is-refused).

## 4. Delete the team

```sh
kairos teams delete <team-id> --confirm
kairos teams list --limit 100      # it is gone
```

The team and its delivery board are soft-deleted together, so there is no orphan
board left behind. Move the people and any service accounts to their new team
first or after, as you prefer:

```sh
kairos teams members add <surviving-team-id> --user <user-id>
```

Deleting the team does not delete its members' accounts, and does not touch work
that moved to another board.

## Related

- [Teams and boards](../explanation/teams-and-boards.md#why-a-team-cannot-be-deleted-while-its-board-still-has-work)
  — why the guards exist
- [Archiving](../explanation/archiving.md) — what putting work away does and
  does not mean
- [Errors](../reference/errors.md) — `BOARD_NOT_EMPTY` and `RESTORE_BLOCKED`
  with their `details`
- [CLI → `kairos teams delete`](../reference/cli.md#kairos-teams-delete)
- [`DELETE /api/teams/{id}`](../reference/rest/boards-and-teams.md)
