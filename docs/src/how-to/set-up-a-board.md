# Set up a board

Shape a board around how a team actually works: add the columns they use, make
them reachable, and give them the fields they fill in.

**Before you start:**

- The board exists. A team is created with its delivery board, so there is
  normally nothing to create — see [Teams and boards](../explanation/teams-and-boards.md).
- You hold `configure_boards` on that board, or you are an org admin. Steps 1
  to 3 return 403 without it.
- **Step 4 needs org admin specifically.** Metadata definitions are
  tenant-wide, and creating or retiring one is org-admin only — a board
  capability does not reach them. If you are not one, get the fields created by
  someone who is and do steps 1 to 3 yourself.
- **Board configuration is GUI and API only.** The `kairos` CLI reads boards
  (`kairos boards list`, `kairos boards show`) and does not configure them, so
  work from *Admin → Boards → your board* in the GUI, or call the endpoints
  directly.

## 1. Add the columns

*Admin → Boards → your board → Columns*, or:

```sh
curl -X POST https://<host>/api/boards/<board-id>/columns \
  -H "Authorization: Bearer <token>" -H 'Content-Type: application/json' \
  -d '{"name":"Review","position":3}'
```

Names and positions are both unique on a board: a clash comes back as 422
`DUPLICATE_COLUMN_NAME` or `DUPLICATE_COLUMN_POSITION` rather than being
silently resolved. `PATCH` on a column renames it or moves it, reordering the
others around the new position.

**The lowest-position column is the entry column.** Newly created work lands
there, tasks moved in from another board land there, and cross-team filings are
confined to it. Reordering columns therefore reorders that too — if you put
`Review` at position 0, new work starts in Review.

## 2. Make the new column reachable

A column with no transition edge into it cannot be reached. Adding `Review`
between `Active` and `Completed` means two edges, not one:

```sh
curl -X POST https://<host>/api/boards/<board-id>/transitions \
  -H "Authorization: Bearer <token>" -H 'Content-Type: application/json' \
  -d '{"from_column_id":"<active>","to_column_id":"<review>"}'
```

…then `Review → Completed`. In the GUI the Transitions panel has **From** and
**To** selects and an **Add transition** button; do it twice.

Duplicate edges are refused with 422 `DUPLICATE_TRANSITION`, as are edges whose
endpoints are not both on this board.

## 3. Check it from the board

Move a real card through the new edge — drag it in the GUI, or:

```sh
curl https://<host>/api/boards/<board-id>/columns -H "Authorization: Bearer <token>"
kairos tasks transition <CODE> --to <review-column-id>
```

(`--to` takes the column's UUID, not its name.)

A 422 `INVALID_TRANSITION` lists the allowed target columns by name and id from
where the card is, which is the fastest way to find the edge you forgot. If the
card will go in but not out, you added one edge and needed two.

## 4. Add the fields the team will fill in — as an org admin

Board columns say where work is; metadata definitions say what the team records
about it. *Admin → Metadata*, or
[`POST /api/metadata-definitions`](../reference/rest/tenant-configuration.md)
(org admin only, as is the `DELETE` below).
Once defined, a field is stamped on an item from the item page, over MCP with
`set_metadata`, or with
[`PATCH /api/{entity_type}/{short_code}/metadata`](../reference/rest/across-any-work-item.md)
— not from the CLI, which stamps no metadata. It is searchable from anywhere:

```sh
kairos search --metadata risk=high
```

**Retiring a definition is where people get stuck.** `DELETE` is refused with
409 `DEFINITION_IN_USE` while any item still carries a value, and neither
`?include_deleted=true` nor `?force=true` overrides it. The trap: once a card is
archived its metadata row is unreachable — there is no route that clears it —
but it is still counted. **Clear the stamp before you archive or delete the
card**, or that definition can never be retired.

There is nothing to configure for the Planned and Support lanes: work class is
a property of the task, not of the board
([`work_class` in the CLI](../reference/cli.md#kairos-search)).

## Removing a column

```sh
curl -X DELETE https://<host>/api/boards/<board-id>/columns/<col-id> \
  -H "Authorization: Bearer <token>"
```

Two outcomes worth knowing before you try:

- **A live card in the column refuses the removal** — 422 `COLUMN_NOT_EMPTY`,
  with the count in `details.item_count`. Move the cards first.
- **A column holding only archived cards can be removed.** It is a soft delete:
  live boards stop showing the column, its transition edges are kept but no
  longer apply, and archived cards keep reporting the column they were put away
  in. That is deliberate — see
  [Archiving](../explanation/archiving.md#restore-refuses-rather-than-re-homing) —
  and it has one consequence to plan for: an archived card whose column is gone
  can no longer be restored. See
  [Find archived work](find-archived-work.md#when-a-restore-is-refused).

## Who may do this

`configure_boards` is a board-scoped capability, granted per board through
[`POST /api/boards/{id}/members`](../reference/rest/boards-and-teams.md) or the
GUI's board members panel — not through the CLI, and not through a role. Org
admins bypass it. The full vocabulary is in
[Capabilities](../reference/capabilities.md); why it works this way is
[Capabilities and access](../explanation/capabilities-and-access.md).

## Related

- [Boards and teams](../reference/rest/boards-and-teams.md) — every column and
  transition endpoint
- [Errors](../reference/errors.md) — the `DUPLICATE_*`, `COLUMN_NOT_EMPTY` and
  `DEFINITION_IN_USE` codes with their statuses and `details`
- [Flight levels](../explanation/flight-levels.md#why-the-boards-are-configurable)
- [Move work between boards](move-work-between-boards.md)
- [Wind down a team](wind-down-a-team.md)
