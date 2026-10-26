---
name: implement
description: Implement one Kairos task from Todo to Completed. Use when the user names a task or short code to implement, asks to pick up the next todo item, or when groomed work is handed off for execution.
---

# Implement

Execute one Kairos task from Todo to Completed.

## Pick up

Your **queue** is the board of your team, filtered by the repository of this checkout. The SessionStart context gives both: `team_board` and `repository` (`.claude/kairos.local.md`, COLLIERY-A-0023). Given a short code, `get_item` it. Given nothing, `board_items` with `board=<team_board>`, `repository=<this repo>`, `column=Todo` and `details: true` → the highest-priority item with no unresolved `blocks` edge; state the pick and why before starting. With `details`, each card shows its priority, the date of its last change and its claim, so you need no `get_item` for each card. `list_metadata_definitions` gives the order of the priority values. (No `repository` wired? Fall back to `my_boards` → the delivery board → `board_items` unfiltered, and suggest `/kairos:bootstrap`.)

**Read each part of the board before you pick.** `board_items` gives 200 items at most in one result. When the result is a part of the board, its first lines say so and give the `offset` of the next part. Call `board_items` again with that `offset` until you have each part. Then make the decision. To read one column only, send `column` (`column=Todo`).

**Stay in your repository.** `get_item` prints the link of the task: `repository: <slug> (owner: <team>)`. The owner can be a different team; that is normal. You need a clone of that repository. It is this checkout or the clone of this worktree. In a board folder, the session context lists the clones. `get_repository <slug>` gives the full name to compare with the origin remote. If no clone has that origin, stop: say which repository holds the code, and pick a different task. A task with no repository that needs code in this checkout: link it with `set_repository` (`short_code`, `repository`), then continue.

**The work of other teams in this repository is not in your queue.** Find it with `search` (`filter.repository`), or in the commits. Read it; the team of its board does it.

**A task on the wrong board is moved, not recreated.** When a ticket is on this board but is really another team's work, `move_item` it to their delivery board (`to_board`) — recreating it there throws away its history, its edges and its author. You need `manage_tasks` on both boards; if the tool refuses, name the board it belongs on and ask the lead. The task keeps its repository: the move does not look at it.

Anything procedural you write while implementing — a how-to or reference page, an error message, CLI help text — follows Simplified Technical English: [The Simplified Technical English reference](../../../references/simplified-technical-english.md) (KAIROS-S-0009). It does **not** apply to your commit message, your code comments, or the reasoning you record on the item; those exist to explain why, and STE cannot. `angreal docs ste` checks the mechanical rules where the project has that task, and `angreal docs ste --code` checks the texts of errors in the code.

The task must be workable: acceptance criteria present and independently verifiable, blockers resolved. When it isn't, stop, note the gap on the item, and tell the lead it needs `/kairos:triage` — surface the gap rather than quietly filling it yourself.

`transition_item` to **Active** before the first change. The move gives the task your **claim**: the board and `get_item` show that you (or your agent) work on it. If `get_item` shows a claim of a different person, the task is theirs: ask before you start, and do not take it over. A change to a task with the claim of a different person succeeds. The answer then ends with a `Warning:` line that names the person. Stop, do "Abort", and tell the lead.

**Read your notifications.** While you have unread notifications, each answer of a Kairos tool ends with a block `## Your notifications (N unread)`: the 5 newest. With more than 5, the heading says `5+ unread`, and the block ends with `More: call get_events`. A notification tells you of a claim handed off to you, a blocker of your claimed task that is done or archived, a task that you created that someone cancelled, a mention of your user name, or a P0 bug on the board of your team. `get_events` lists them with their ids. Act on each one that changes your work (a cleared blocker can make a task workable; a P0 bug can come before your pick), tell the lead about the others, then `mark_events_read` (`ids`, or `all: true` with `up_to`: the newest id that you read, as `get_events` gives it). Your agent key keeps its own read state: your read does not clear the bell of the person. Do not mark a notification read before you have read it.

To give the task to a different person, `hand_off_item` (`to`: a user id, an email, a user name, or `me`). `list_members` gives the user names: send `board` for the people who can move the task. To mention a person in a note, write `@<user name>`. `whoami` gives your own user name. To stop work on a task and leave it in Active for anyone, `release_item`. To take a free task in Active, `claim_item` (KAIROS-T-0528): a task that a different person holds is refused with the name of the holder. The claim ends by itself when the task leaves Active.

## Open the work

Do the work in a worktree of the clone, not in the clone. Other sessions use the same clone. Do these steps after the claim:

1. Find the clone. In a checkout, it is this checkout, or the clone of this worktree (`git rev-parse --git-common-dir`). In a board folder, find it in the list of clones of the session context.
2. Find the default branch. `get_repository` gives it, else `git -C <clone> symbolic-ref --short refs/remotes/origin/HEAD`.
3. Get the latest commits: `git -C <clone> fetch origin <default>`.
4. Make the worktree next to the clone. The folder and the branch use the short code in lower case:
   `git -C <clone> worktree add ../wt-<code> -b <code> origin/<default>`
   For KAIROS-T-0522 that is the folder `../wt-kairos-t-0522` and the branch `kairos-t-0522`. When the worktree of the task is there already, use it.
5. Do the repository check on the worktree. `git -C <worktree> remote get-url origin` must give the repository of the task. If it does not, remove the worktree and stop.
6. Open the code index of the worktree: `fetch_index` with `root=<worktree>`.
7. Write the branch: `set_state` with `branch=<code>` and the next step.

The worktree has no `.claude/kairos.local.md`. The hooks read the wiring of its clone. Each later step runs in the worktree. Use `git -C <worktree>` and absolute paths. Do not use `cd` in commands that run in parallel, because they share one shell.

## Ground

Before the plan, read the record: [GROUND.md](../../meta/kairos/GROUND.md). The ADRs and the documents of this repository (`get_repository <slug>`: the items that impact it, and the ADRs of the owner team; `team_pages <team>`: the tree of the team), the documents of the parent initiative, and `related_work` on the task. Write the sentence it gives ("Read: ... Nothing read contradicts this plan." or "Step N contradicts ...") on the task and in your reply, before the first change.

## Check the names

List each symbol, file, crate and dependency that the task text names. Check each one with the code tools (`symbol`, `code_search`, `path`) before the plan relies on it. A name that is not in the code gets one note on the task, "Stale: `resolve_many` (KAIROS-T-0579) is not in the code", and the plan does not use it. A blocker that the task names: `get_item` it. Is it open, and does its scope block this task? When it does not, say so on the task.

An assertion that you make without evidence, in a note or in a reply, carries the marker "not checked". Repair it when you check it. The closing note of the task lists what stays "not checked".

## Work

The task item is your **working memory**. Every few significant steps, `append_note` findings, decisions, dead ends ruled out, and plan changes onto it (one call: no version, no anchor text; the note gets an id, and `get_item` shows it after the content under a heading with the date and your name) — the item must let any session, or another agent, resume cold after this conversation's context is gone. Reference commits, files, and other items by hash, path, or short code rather than restating them. End each note with a `Next step:` line: after a context compaction, the plugin gives you that line again for each Active task.

The task also has a **live state**, separate from the notes: one current line that `get_item`, `board_items` and the card show. Write it with `set_state` (`branch`, `commit`, `pushed`, `pr_url`, `next_step`) after each commit, after each push, after a PR opens, and before any stop or hand-off. The write is a patch (KAIROS-A-0029): a field that you do not send keeps its value, and `null` clears it. A note records what happened; the state says where the work is now.

To find code, use the code tools of the `kairos-code` MCP server before you read files: `module_map`, `code_search`, `callers` and `callees` (and `symbol`, `references` and `path`). Then read only the files that they name.

When you split the task, or add a task for work that you find, link the tasks with `blocks` when one must wait for the other. Give no edge to tasks that can go in parallel. Say the order in each task.

**What remains lives on the items.** The remaining work is the open tasks of the initiative and their `blocks` edges, not a list you rewrite from memory each turn. When asked what is left: `get_item` the initiative, then `search` with `traverse: {from: <initiative>, relationships: [parent, blocks], direction: both, depth: 2}`, and present the open tasks in the order of their edges. A new step goes on the record before you answer: a task with its `blocks` edge, or a note on the task when it is small. A deliverable of the active task is never "out of scope" in a reply: it is done, split into a task with a reason and an edge, or left on the task as open.

Use the `tdd` skill at the seams that the task names, or that the lead agrees. Run the repo's fast checks — formatter, typecheck, single test files — as you go; the full suite once at the end.

## The completion gate

The task transitions Active → Completed **only** when all of these hold, with output recorded on the item:

1. **Gates clean** where the repo has them: formatter check, linter with warnings denied, full test suite green (in a Rust/angreal repo: `cargo fmt --check`, `cargo clippy -- -D warnings`, `angreal test unit && angreal test integration`; otherwise the repo's equivalents).
2. **Every acceptance criterion demonstrated, not asserted**: for each criterion, `append_note` the command you ran and the observed output onto the item as evidence.
3. **New behavior carries new tests.**

The closing note lists what stays "not checked" (a manual check, a live run, a claim without evidence).

Before Completed, ask: did this work decide anything an ADR or the spec should hold? A behaviour change, a rule, a boundary. The test for an ADR: would a reader of the code get it wrong without it? That includes a choice against a library default, a rejected option that will come back, and a decision of the user. Skip routine bug fixes. Yes: record it with the "Record this decision" path of the `domain-modeling` skill (an ADR with `parent` this task, `impacts` the repository), and write its short code in the closing note. Then cite the ADR in the code that you changed: a comment or a test names the short code and does not copy the reasoning. A change of a part of an existing ADR `amends` it; a replacement of all of it `supersedes` it. No: say so in the closing note ("No decision to record."). A decision in a note or in local memory only is lost to the next session.

Then review the diff (the `kairos:code-review` skill; the built-in `/code-review` posts nothing to the task). Commit in the worktree, on the branch of the task (`git -C <worktree> commit`). Do "Push and open the PR", then "Close the work", then `transition_item` to **Completed**. When the work needs a change from a DIFFERENT team (an API to add, a client to update), send a request to that team. The recipe is [CROSS-TEAM-FILING.md](CROSS-TEAM-FILING.md): `create_item` (`board: <the delivery board of that team>`, `parent: <this task's initiative>`), then `link_items blocks` from the request to this task.

## Push and open the PR

This is the push policy (KAIROS-A-0046). You push the branch of the task and open a draft PR. Then you stop. The user merges.

1. Push the branch of the task: `git -C <worktree> push -u origin <code>`. The git guard lets this push through.
2. Open a draft PR: `gh pr create --draft --base <default> --head <code>`. Run it in the worktree. The body links the task.
3. Write the PR on the task: `set_state` with each field. Send `branch`, `commit`, `pushed: true`, `pr_url` and `next_step`.
4. Stop at the PR. Do not merge, release, tag or deploy.

Do not push the default branch. Do not use `--force`, `--delete` or a `+` refspec. The guard asks the user for each other push. When the guard asks, tell the user why you push.

## Close the work

Do this step at Completed. When the branch is pushed and the PR is open, remove the worktree:

`git -C <clone> worktree remove ../wt-<code>`

Keep the branch, because the PR uses it. Do not use `--force`. When the command refuses, the worktree has changes that are not committed: keep it. Also keep the worktree when the branch is not pushed or no PR is open. When this session runs in the worktree, keep it and tell the user to remove it after the session. Write the path of a kept worktree and the reason in the closing note.

## Abort

Do these steps when you stop a task before Completed. Examples: the user cancels the work, `/kairos:cancel-ralph` stops the loop, or a different person has the claim. Do not leave a task with no record. A task that waits on a decision or another team goes to Blocked first (the next section).

1. Look for changes that are not committed: `git -C <worktree> status --porcelain`.
2. When the output is empty, remove the worktree: `git -C <clone> worktree remove ../wt-<code>`. Keep the branch.
3. When the output is not empty, keep the worktree. Do not use `--force`.
4. `append_note` on the task. Write what is done and what is not done. Write the path of a kept worktree and the reason. End with a `Next step:` line.
5. `set_state` with each field: `branch`, the last `commit`, `pushed` (true or false), `pr_url`, and `next_step`.
6. When the task is in Active and you hold the claim, `release_item` the task. The task stays in Active, free for the next person. Do not release the claim of a different person.

Then tell the lead the short code, the state and the worktree.

## Blocked

When the task stalls on something outside your reach — a decision, access, another team — `link_items` (`blocks`) from the blocking thing to the task, `transition_item` to Blocked, and `append_note` a note naming exactly what's needed and from whom, on the task **and** on its parent initiative so the blocker surfaces at the Initiative Board Review. Then stop rather than guess.
