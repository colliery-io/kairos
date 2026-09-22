---
name: decompose
description: Break an initiative, plan, or PRD into independently-grabbable Kairos tasks using tracer-bullet vertical slices.
argument-hint: "Initiative or document short code (optional if it's already in the conversation)"
disable-model-invocation: true
---

# Decompose

Break a plan into independently-grabbable Kairos tasks using vertical slices (tracer bullets).

## Process

### 1. Gather context

Work from whatever is already in the conversation. If the user passes a short code as an argument, `get_item` it — and if it's an initiative whose relationships list an attached PRD, `get_item` that too — and read the full content. If neither an argument nor a plan in the conversation exists, ask the user what to decompose.

### 2. Explore the codebase (if not already done)

If you have not already explored the codebase, do so to understand the current state of the code. Task titles and descriptions should use the project's domain glossary vocabulary, and respect ADRs in the area you're touching.

Look for opportunities to prefactor the code to make the implementation easier. "Make the change easy, then make the easy change."

### 3. Draft the tasks

Break the plan into **tracer bullet** tasks, following the **Vertical slice rules**. A **wide refactor** is the exception to that rule — slice it by **expand–contract** instead (see **Wide refactors**). Prefactoring becomes its own leading task(s) that the slices it enables block on. Every user story in the source material maps to a slice or goes on an explicit uncovered list.

### 4. Quiz the user

Present the proposed breakdown as a numbered list. For each slice, show:

- **Title**: short descriptive name
- **Blocked by**: which other slices (if any) must complete first
- **User stories covered**: which user stories this addresses (if the source material has them) — and show the uncovered list, so nothing vanishes silently

Ask the user:

- Does the granularity feel right? (too coarse / too fine)
- Are the dependency relationships correct?
- Should any slices be merged or split further?

Iterate until the user approves the breakdown.

### 5. Publish the tasks to Kairos

Every task is issued against **one repository** (KAIROS-A-0019): the repo an agent will implement it in. Default to the session's `repository` (SessionStart context). When the initiative spans repositories, decide per slice — ask the user which repo each slice lands in (a slice that "touches both" is two slices, one per repo, joined by a `blocks` edge). Use `list_repositories` to see the directory when another team's repo is involved; a task filed against another team's repository lands in THEIR Backlog for triage (any member may do this).

A repository routes the task to its owning team's delivery board, so `board` is not needed; only when the session has no repository, find the board with `my_boards` (if more than one could hold the work, ask the user which) and pass `board` instead.

Publish in dependency order — blockers first — so every blocking edge names a real short code. For each approved slice:

- `create_item(item_type: task, repository: <slug>, parent: <initiative short code>, title: ..., content: ...)` using the **Task body template** (`board: <delivery board>` instead of `repository` only when nothing is wired). The `parent` edge links the task to its initiative — the body carries no parent section.
- For each of its blockers: `link_items(source: <blocker short code>, target: <this task's short code>, relationship: blocks)`.

Columns and blocking edges are native to Kairos — new tasks land in the board's entry column, and that's where they belong; leave them for the team to pull. The initiative itself stays untouched: decompose only adds children.

Finish by listing the created short codes in dependency order, each with its repository (and, for another team's repo, a note that it awaits that team's triage).

## Reference

### Vertical slice rules

Each task is a thin vertical slice that cuts through ALL integration layers end-to-end, NOT a horizontal slice of one layer.

- Each slice delivers a narrow but COMPLETE path through every layer (schema, API, UI, tests)
- A completed slice is demoable or verifiable on its own
- Any prefactoring should be done first

### Wide refactors

A **wide refactor** is one mechanical change — rename a column, retype a shared symbol — whose **blast radius** fans across the whole codebase, so a single edit breaks thousands of call sites at once and no vertical slice can land green. Don't force it into a tracer bullet; sequence it as **expand–contract**. First expand: add the new form beside the old so nothing breaks. Then migrate the call sites over in batches sized by blast radius (per package, per directory), each batch its own task blocked by the expand, keeping CI green batch to batch because the old form still exists. Finally contract: delete the old form once no caller remains, in a task blocked by every migrate batch. When even the batches can't stay green alone, keep the sequence but let them share an integration branch that all block a final integrate-and-verify task — green is promised only there.

### Task body template

<task-template>
## What to build

A concise description of this vertical slice. Describe the end-to-end behavior, not layer-by-layer implementation.

Avoid specific file paths or code snippets — they go stale fast. Exception: if a prototype produced code that encodes a decision more precisely than prose can (state machine, reducer, schema, type shape), add a context pointer to where that prototype code lives rather than inlining it.

## Acceptance criteria

- [ ] Criterion 1
- [ ] Criterion 2
- [ ] Criterion 3
</task-template>
