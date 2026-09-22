# Filing work against another team's repository

Any member may do this; the task lands in that team's **Backlog** for their
triage (the computed `file_backlog` capability, KAIROS-A-0019 §4). The
`parent` you name must be one you manage or authored; the `blocks` edge
you add is allowed because you created the task.

1. `list_repositories` — find the repo and its owning team; `get_repository <slug>` — read its "How to work here" description so the request fits how they work.
2. `create_item` with `item_type: task`, `repository: <their slug>`, `parent: <YOUR initiative>` (one you manage or authored — their initiative would be refused), a title, and a body that says what you need, why, and what "done" looks like for you.
3. `link_items` with `relationship: blocks`, `source: <the new task>`, `target: <your item>`, so your board shows the dependency and theirs shows who is waiting.
4. Report the short code and that it sits in that team's Backlog awaiting their triage. Do not transition, edit or implement it — that is theirs. A PR you later open in their repository naming the short code links itself to the ticket through their forge webhook.

What you cannot do from here: implement a ticket bound to another repository (switch checkouts), move it out of their Backlog, parent it under their initiative, or bind one ticket to two repositories — split it instead.
