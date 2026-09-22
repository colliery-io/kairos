---
name: implement
description: Implement one Kairos task from Todo to Completed. Use when the user names a task or short code to implement, asks to pick up the next todo item, or when groomed work is handed off for execution.
---

# Implement

Execute one Kairos task from Todo to Completed.

## Pick up

This session is scoped to one **repository** — the `repository` in the SessionStart context (`.claude/kairos.local.md`, KAIROS-A-0019). Given a short code, `get_item` it. Given nothing, `board_items` on the team board with `repository=<this repo>` → Todo → the highest-priority item with no unresolved `blocks` edge; state the pick and why before starting. (No `repository` wired? Fall back to `my_boards` → the delivery board → `board_items` unfiltered, and suggest `/kairos:bootstrap`.)

**Stay in your repository.** `get_item` prints the task's `repository: <slug> (owner: <team>)`; if it differs from this checkout's, stop: say which repository it belongs to and that it must be implemented from that checkout (or by that team), and pick something else. Never implement another codebase's ticket from here. An item with no `repository` on a multi-repo team: bind it first — there is no MCP tool for binding, so ask the user to run `kairos repos bind <code> <slug>` (or do it if the CLI is authenticated in this session) — then proceed.

The task must be workable: acceptance criteria present and independently verifiable, blockers resolved. When it isn't, stop, note the gap on the item, and tell the lead it needs `/kairos:triage` — surface the gap rather than quietly filling it yourself.

`transition_item` to **Active** before the first change.

## Work

The task item is your **working memory**. Every few significant steps, `edit_item` findings, decisions, dead ends ruled out, and plan changes into it — the item must let any session, or another agent, resume cold after this conversation's context is gone. Reference commits, files, and other items by hash, path, or short code rather than restating them.

Use the `tdd` skill at pre-agreed seams where possible. Run the repo's fast checks — formatter, typecheck, single test files — as you go; the full suite once at the end.

## The completion gate

The task transitions Active → Completed **only** when all of these hold, with output recorded on the item:

1. **Gates clean** where the repo has them: formatter check, linter with warnings denied, full test suite green (in a Rust/angreal repo: `cargo fmt --check`, `cargo clippy -- -D warnings`, `angreal test unit && angreal test integration`; otherwise the repo's equivalents).
2. **Every acceptance criterion demonstrated, not asserted**: for each criterion, `edit_item` the command you ran and the observed output onto the item as evidence.
3. **New behavior carries new tests.**

Then review the diff (the `code-review` skill), commit to the current branch, and `transition_item` to **Completed**. When the work needs a change in ANOTHER repository (an API the other team must add, a client to update), do not stall: file it against that repository — the recipe is [CROSS-TEAM-FILING.md](CROSS-TEAM-FILING.md): `create_item` (`repository: <their slug>`, `parent: <this task's initiative>`) then `link_items blocks` from the new task to this one; it lands in their Backlog for triage.

## Blocked

When the task stalls on something outside your reach — a decision, access, another team — `link_items` (`blocks`) from the blocking thing to the task, `transition_item` to Blocked, and append a note naming exactly what's needed and from whom, on the task **and** on its parent initiative so the blocker surfaces at the Initiative Board Review. Then stop rather than guess.
