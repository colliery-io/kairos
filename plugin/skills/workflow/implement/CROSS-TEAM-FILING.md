# A request to a different team

Teams request work of each other. Any member can create a task on the delivery
board of any team (the computed `file_backlog` capability, COLLIERY-A-0023).
When you do not manage that board, the task is a **request**. A request goes
to the **entry column**, in the **support lane**.

1. Find the board. `my_boards` with `board_level: delivery` lists the delivery boards. For a request about code, `get_repository <slug>` gives the owning team and the owner's delivery board. Read its "How to work here" description.
2. `create_item` with `item_type: task`, `board: <the delivery board of that team>`, `parent: <YOUR initiative>`, a title and a body. Use the initiative that your work belongs to. You create the request, so the server permits the edge to any initiative. The body says what you need, why, and what "done" is for you.
3. Add `repository: <slug>` only when the request is about code in that repository. The repository is an optional link; it does not choose the board.
4. Omit `work_class`. The server sets `support`, and it refuses `planned` on a board that you do not manage.
5. Send `from_team: <your team>`. The key you work with may manage every board (an organization admin's key); `from_team` makes the task a request all the same, in the entry column and the support lane. Kairos does not keep `from_team` on the task, so the body names your team too.
6. `link_items` with `relationship: blocks`, `source: <the request>`, `target: <your task>`. You created the request, so you can edit it, and the server permits an edge when you can edit one end.
7. Report the short code. The answer of `create_item` names the lane: say that the request is in the entry column of that board, in the support lane.

That team moves the request. You created it, so you can edit it (`update_item`, `edit_item`, `set_metadata`, `set_repository`), link it, and archive it (`delete_item`). You cannot move it. `transition_item` needs `transition_items` on that board, and `move_item` needs `manage_tasks` on the two boards. A request is not a review. The git provider manages a pull request, so create no task to ask for a review.
