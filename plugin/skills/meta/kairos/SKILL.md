---
name: kairos
description: Router for the kairos plugin's user-invoked skills — which to reach for, and when.
disable-model-invocation: true
---

The kairos plugin's user-invoked surface. Reach for:

- `/kairos:bootstrap` — first, in any repo not yet wired to a Kairos deployment (and after remote/team/board changes): configure the MCP endpoint, confirm auth, detect this repository from the git remote, discover boards, write `.claude/kairos.local.md`.
- `/kairos:grill-me` — before building: a relentless interview that stress-tests a plan or design until shared understanding is reached.
- `/kairos:grill-with-docs` — before building, when the design should leave a paper trail: the same relentless interview as grill-me, additionally capturing glossary terms (CONTEXT.md) and Kairos ADRs as decisions crystallise.
- `/kairos:to-initiative` — when a discussed plan is ready to become work: synthesize the conversation into a Kairos initiative with an attached PRD (no interview — grill first if it needs one).
- `/kairos:decompose` — when an initiative or plan needs breaking into tasks: tracer-bullet vertical slices on the delivery board, quizzed with the user, blocking edges wired, published in dependency order.
- `/kairos:triage` — grooming the delivery board: refine acceptance criteria, prioritize bugs, weigh tech debt against its ~20% allocation, archive stale items, restock Todo, escalate cross-team blockers.
- `/kairos:architecture-review` — reviewing a codebase's structure: scan, classify by altitude, an HTML report of deepening candidates, then a grilling loop on the one you pick.
- `/kairos:diataxis-review` — reviewing a documentation tree: classify every page against the Diataxis spec, run the structural pass, optionally file findings as tech debt.
- `/kairos:writing-great-skills` — when writing or editing a skill: the normative authoring reference (invocation split, information hierarchy, leading words, pruning).
- `/kairos:handoff` — at the end of a session: bring the active Kairos items current, then compact the leftover conversation context into a note for the next session.
- `/kairos:kairos` — this router.

Model-invoked disciplines (the agent reaches these on its own; naming them works too): `grilling` (the interview loop behind grill-me), `implement` (pick up a Kairos task, work it with the task as working memory, complete through the verification gate), `code-review` (two-axis: repo standards + spec fidelity to the originating item), `tdd` (red → green test-first; slice progress lands on the task), `diagnosing-bugs` (reproduce → minimise → hypothesise → instrument → fix → regression-test), `prototype` (throwaway build to answer a design question), `research` (background investigation saved as a Kairos document), `domain-modeling` (glossary + ADR upkeep), `codebase-design` (deep-module design vocabulary and discipline).

**Sync rule**: this router maps the plugin's entire user-invoked surface. Adding, renaming, removing, or changing the behavior of a user-invoked skill without updating this router is a defect — a router that lies is a bug.

## Repositories: where tickets are issued and executed

Kairos plans work on boards and streams, but a ticket is issued against ONE repository and executed inside it (KAIROS-A-0019). Every repository has exactly one owning team; a task filed against it lands on that team's delivery board. The session's `repository` (SessionStart context, from `/kairos:bootstrap`) scopes everything: `board_items` with `repository=<slug>` is your queue, tasks you create take `repository=<slug>`, and `get_repository <slug>` gives a repo's owner, board, "how to work here" description and in-flight PRs.

**Filing work against another team's repository** (any member may; it lands in their Backlog for their triage):

1. `list_repositories` — find the repo and its owning team; `get_repository <slug>` — read its description so the request fits how they work.
2. `create_item` with `item_type: task`, `repository: <their slug>`, `parent: <your initiative or theirs>`, a title and a body that says what you need, why, and what "done" looks like for you.
3. `link_items` with `relationship: blocks` from the new task to your item, so your board shows the dependency and theirs shows who is waiting.
4. Report the short code and that it sits in that team's Backlog awaiting their triage; do not transition, edit or implement it — that is theirs. A PR you later open in their repository naming the short code links itself to the ticket through their forge webhook.

What you cannot do from here: implement a ticket bound to another repository (switch checkouts), move it out of their Backlog, or bind one ticket to two repositories — split it instead.
