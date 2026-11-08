---
name: tdd
description: Test-driven development. Use when the user wants to build features or fix bugs test-first, mentions "red-green", or wants integration tests.
---

# Test-Driven Development

TDD is the red → green loop. This skill is the reference that makes that loop produce tests worth keeping: what a good test is, where tests go, the anti-patterns, and the rules of the loop. Every section applies on every cycle — consult them before and during the loop, not after.

Before the first cycle, pull the active Kairos task (`get_item` — short code from session context, or `my_boards` to find it) so test names and interface vocabulary match the domain language on the item, and respect ADR documents linked in the area you're touching.

Code index first. Find code with the tools of the `kairos-code` MCP server before you read a file. `code_search` finds what the code does; `symbol` finds one name. `callers` and `references` find its uses; `path` finds how one symbol reaches another. Then read only the files that they name. grep is for literal text only: a log line, a config key, an error message. After you change code, run `kairos index update`.

## What a good test is

Tests verify behavior through public interfaces, not implementation details. Code can change entirely; tests shouldn't. A good test reads like a specification — "user can checkout with valid cart" tells you exactly what capability exists — and survives refactors because it doesn't care about internal structure.

See [tests.md](tests.md) for examples and [mocking.md](mocking.md) for mocking guidelines.

## Seams — where tests go

A **seam** is the public boundary you test at: the interface where you observe behavior without reaching inside. Tests live at seams, never against internals.

**Test only at agreed seams.** Use a seam that the task names, or a seam that the user confirms. You cannot test everything. Agreed seams put the tests on the critical paths and the complex logic, not on each edge case.

Read the seams from the task:

1. Read the task with `get_item`. Read its acceptance criteria, its content and its notes.
2. Find each public interface that the task names. Examples: a function, a command, an endpoint, an MCP tool, a hook.
3. Each interface that the task names is a seam. For these seams, do not ask the user to confirm.
4. Before the first test, write the seams on the task with `append_note`. For each seam, give the criterion or the note that names it.

If the task names no seam, or you need a seam that the task does not name, ask the user. Ask: "What is the public interface, and which seams do we test?"

In a Ralph loop or a dispatch run, no user can answer. Then do not guess a seam:

- Write the question on the task with `append_note`.
- End the reply with `<promise>NEED INPUT</promise>`. A Ralph loop then stops.

## Anti-patterns

- **Implementation-coupled** — mocks internal collaborators, tests private methods, or verifies through a side channel (querying the database instead of using the interface). The tell: the test breaks when you refactor but behavior hasn't changed.
- **Tautological** — the assertion recomputes the expected value the way the code does (`expect(add(a, b)).toBe(a + b)`, a snapshot derived by hand the same way, a constant asserted equal to itself), so it passes by construction and can never disagree with the code. Expected values must come from an independent source of truth — a known-good literal, a worked example, the spec.
- **Horizontal slicing** — writing all tests first, then all implementation. Bulk tests verify _imagined_ behavior: you test the _shape_ of things rather than user-facing behavior, the tests go insensitive to real changes, and you commit to test structure before understanding the implementation. Work in **vertical slices** instead — one test → one implementation → repeat, each test a **tracer bullet** that responds to what the last cycle taught you.

## Rules of the loop

- **Red before green.** Write the failing test first, then only enough code to pass it. Don't anticipate future tests or add speculative features.
- **One slice at a time.** One seam, one test, one minimal implementation per cycle.
- **Refactoring is not part of the loop.** It belongs to the review stage (see the `kairos:code-review` skill), not the red → green implementation cycle.
- **Progress lives on the task.** As each slice lands, append a progress note to the active Kairos task with `append_note` — the seam, the test, what the cycle proved. The task is the persistent memory; the conversation is not.
