# A request to a different team

Teams request work of each other. Any member can create a task on the delivery
board of any team (the computed `file_backlog` capability, COLLIERY-A-0023).
When you do not manage that board, the task is a **request**. A request goes
to the **entry column**, in the **support lane**.

1. Find the board. `my_boards` with `level: delivery` lists the delivery boards. For a request about code, `get_repository <slug>` gives the owning team and the owner's delivery board. Read its "How to work here" description.
2. `create_item` with `item_type: task`, `board: <the delivery board of that team>`, `parent: <YOUR initiative>`, a title and a body. Use an initiative that you manage or created. The body says what you need, why, and what "done" is for you.
3. Add `repository: <slug>` only when the request is about code in that repository. The repository is an optional link; it does not choose the board.
4. Omit `work_class`. The server sets `support`, and it refuses `planned` on a board that you do not manage.
5. `link_items` with `relationship: blocks`, `source: <the request>`, `target: <your task>`. You created the request, so the server permits this edge.
6. Report the short code. Say that the request is in the entry column of that board, in the support lane.

That team moves the request. You cannot move, edit or delete it: those need `manage_tasks` on that board. A request is not a review. The git provider manages a pull request, so create no task to ask for a review.
