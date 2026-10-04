# From the user's words to Kairos

Kairos has five item types: `strategy`, `initiative`, `task`, `document` and `adr`. A task has a `task_type`: `task`, `bug`, `tech_debt` or `support`. An initiative can be a bucket with a `bucket_type`: `tech_debt`, `bug` or `ad_hoc`. Nothing else is an item type. When the user uses a word from a different tracker, translate it. Use the Kairos word in the reply ("an epic is an initiative in Kairos: `COLLIERY-I-0012`").

## Item types

| The user says | Kairos | `create_item` |
|---|---|---|
| epic, project, workstream, feature (large) | initiative | `item_type: initiative`, on an initiative board |
| ticket, story, issue, card, work item | task | `item_type: task`, `board: <delivery board>` |
| bug, defect, regression | task of type bug | `task_type: bug` |
| tech debt, refactor, cleanup | task of type tech debt | `task_type: tech_debt` |
| support request, ops work, "can you help with" | task of type support, in the support lane | `task_type: support` (the lane is `support` by default) |
| a request to a different team | a task on the delivery board of that team, in the support lane | see `workflow/implement/CROSS-TEAM-FILING.md` |
| spike, investigation | task | title it "Spike: ..." |
| research, findings | document that supports the task | `item_type: document, board: <team board>, parent: <task>`; see `engineering/research` |
| spec, design doc, PRD, RFC | document that supports an item, from a template | `item_type: document, board: <team board>, parent: <initiative>, template: <name>` |
| vision, architecture overview of a repository | document that a board owns, and that impacts the repository | `item_type: document, board: <team board>, template: <name>`, then `link_items impacts` |
| decision record, "why we did it" | ADR | `item_type: adr`; see `engineering/domain-modeling/ADR-FORMAT.md` |
| roadmap, OKR, north star, theme | strategy | `item_type: strategy`, with a `hypothesis` |
| a bucket, "a place for the small bugs", standing work | initiative that is a bucket | `item_type: initiative, bucket_type: tech_debt \| bug \| ad_hoc` |
| sub-task, checklist item, step | an acceptance criterion of a task | not an item |
| sprint, milestone, release, phase 2, batch 1 | no item | an order between tasks: a `blocks` edge |
| status, state, "in progress", "done" | the column of the item | `get_item`; move it with `transition_item` |
| assignee, owner (of a task) | not a field | the team of the board owns the task |

The names of the templates are different in each organization. Do not guess a template name: if `create_item` refuses it, the error names the problem.

A task with code to change links to one repository (`repository` at create, or `set_repository`). The repository does not choose the board: the team chooses the board (`COLLIERY-A-0023`).

## Relationships

`link_items` takes a source, a target and one relationship. The server refuses an edge that the rule does not allow between the two item types. It also refuses an edge that makes a cycle.

| Relationship | From → to | Use it when |
|---|---|---|
| `parent` | strategy → initiative, initiative → task | The target is a part of the source. This is the group. A delete cascades along it. `create_item` with `parent` writes it. |
| `supports` | strategy, initiative or task → document or ADR | The document or the ADR belongs to that item (a PRD of an initiative, the findings of a task). `create_item` of a document or an ADR with `parent` writes it. |
| `informs` | document or ADR → strategy, initiative or task | The document or the ADR gives context to the item but does not belong to it (an ADR that an initiative must follow). |
| `supersedes` | ADR → ADR | A new ADR replaces an old one. |
| `blocks` | strategy, initiative or task → strategy, initiative or task | The target cannot start until the source is done. This is the order of the work. `/kairos:ralph-initiative` follows it. |
| `impacts` | document or ADR → repository | The document or the ADR is about that repository. The target is a repository slug, not an item. A task does not impact a repository: `set_repository` links a task. |

No relationship says "this task filed that task" or "these tasks are related" with no order. Write such a link as text on the item, with the short code.
