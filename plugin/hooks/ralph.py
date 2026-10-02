#!/usr/bin/env python3
"""The Ralph loop of the kairos plugin (COLLIERY-T-1861), ported from the
Metis plugin 2.3.0 (`setup-metis-ralph-tasks.sh`, `stop-hook.sh`).

One stdlib module, four subcommands:

    ralph.py start [--session ID] [--max-iterations N] [--missing CODE]... CODE...
    ralph.py start-initiative [--session ID] [--max-iterations N] CODE < plan.json
    ralph.py cancel [--session ID]
    ralph.py stop                      (the Stop hook; hook input on stdin)

The skills `/kairos:ralph`, `/kairos:ralph-initiative` and
`/kairos:cancel-ralph` call it. The skill reads each item with the kairos
MCP tools first (a hook and a script hold no credentials, KAIROS-A-0014),
then gives the script what it found: `--missing` for a task code that
`get_item` did not find, or the plan of an initiative as JSON on stdin.

The state is one JSON file per Claude Code session, OUTSIDE the
repository: `$KAIROS_RALPH_STATE_DIR/<session_id>.json`, default
`~/.claude/kairos-ralph/`. The Stop hook only reads the file of its own
session, so two sessions in one checkout do not interfere.

The Stop hook blocks the stop and gives the next instruction until the last
assistant message ends with `<promise>ALL TASKS COMPLETE</promise>` or
`<promise>LOOP STOPPED</promise>`, or the iteration limit is reached. Each
error in the hook lets the session stop: a broken loop must never trap a
session.
"""

import json
import os
import re
import sys
from datetime import datetime, timezone

COMPLETION_PROMISE = "ALL TASKS COMPLETE"
STOP_PROMISE = "LOOP STOPPED"
TASK_CODE = re.compile(r"^[A-Z][A-Z0-9]*-T-[0-9]+$")
INITIATIVE_CODE = re.compile(r"^[A-Z][A-Z0-9]*-I-[0-9]+$")
ANY_CODE = re.compile(r"^[A-Z][A-Z0-9]*-[A-Z]-[0-9]+$")
SESSION_ID = re.compile(r"^[A-Za-z0-9_.-]+$")
DONE_COLUMNS = ("completed", "done")
PLAN_TASK_FIELDS = {"code", "column", "blockers"}
PLAN_BLOCKER_FIELDS = {"code", "done"}


class Refusal(Exception):
    """A start or cancel that must not happen. The text names the cause."""


# --- state -------------------------------------------------------------------


def state_dir():
    configured = os.environ.get("KAIROS_RALPH_STATE_DIR")
    if configured:
        return configured
    return os.path.join(os.path.expanduser("~"), ".claude", "kairos-ralph")


def state_path(session):
    return os.path.join(state_dir(), f"{session}.json")


def read_state(session):
    """The state of the session, None when there is no loop. Raises
    ValueError when the file is not a valid state."""
    path = state_path(session)
    if not os.path.isfile(path):
        return None
    with open(path, encoding="utf-8") as f:
        state = json.load(f)
    if not isinstance(state, dict):
        raise ValueError("the state is not a JSON object")
    for key, kind in (("iteration", int), ("max_iterations", int), ("tasks", list)):
        if not isinstance(state.get(key), kind):
            raise ValueError(f"the state has no valid `{key}`")
    if state.get("mode") not in ("tasks", "initiative"):
        raise ValueError(f"the state has an unknown mode: {state.get('mode')!r}")
    return state


def write_state(session, state):
    os.makedirs(state_dir(), exist_ok=True)
    path = state_path(session)
    temporary = f"{path}.tmp.{os.getpid()}"
    with open(temporary, "w", encoding="utf-8") as f:
        json.dump(state, f, indent=2)
        f.write("\n")
    os.replace(temporary, path)


def remove_state(session):
    try:
        os.remove(state_path(session))
    except FileNotFoundError:
        pass


# --- the order of the tasks of an initiative ----------------------------------


def order_initiative_tasks(tasks):
    """Order the open tasks of an initiative by their `blocks` edges.

    `tasks` is a list of {"code", "column", "blockers": [{"code", "done"}]},
    in the order to use when no edge decides (the skill gives them by short
    code). A blocker is resolved when it is done, or when it is a task of
    this list that runs earlier. A task is skipped when it is Blocked, when
    a blocker outside the list is not done, when a blocker is skipped, or
    when it is in a cycle. Returns (order, skipped), where skipped is a list
    of {"code", "reason"}. Pure function."""
    by_code = {t["code"]: t for t in tasks}
    skipped = {}
    for t in tasks:
        if t.get("column", "").strip().lower() == "blocked":
            skipped[t["code"]] = f"{t['code']} is in the Blocked column"
    for t in tasks:
        if t["code"] in skipped:
            continue
        for blocker in t.get("blockers", []):
            if not blocker.get("done") and blocker["code"] not in by_code:
                skipped[t["code"]] = (
                    f"the blocker {blocker['code']} of {t['code']} is not done"
                )
                break

    order = []
    placed = set()
    progress = True
    while progress:
        progress = False
        for t in tasks:
            code = t["code"]
            if code in placed or code in skipped:
                continue
            waiting = [
                b["code"]
                for b in t.get("blockers", [])
                if not b.get("done") and b["code"] not in placed
            ]
            skipped_blocker = next((c for c in waiting if c in skipped), None)
            if skipped_blocker:
                skipped[code] = (
                    f"the loop skips the blocker {skipped_blocker} of {code}"
                )
                progress = True
            elif not waiting:
                order.append(code)
                placed.add(code)
                progress = True
                break
    for t in tasks:
        if t["code"] not in placed and t["code"] not in skipped:
            skipped[t["code"]] = f"{t['code']} is in a cycle of `blocks` edges"
    return order, [
        {"code": t["code"], "reason": skipped[t["code"]]}
        for t in tasks
        if t["code"] in skipped
    ]


def parse_plan(text):
    """The plan of an initiative from stdin: {"tasks": [...]}, each task as
    `order_initiative_tasks` takes it. Refuses each unknown field."""
    try:
        plan = json.loads(text)
    except ValueError as error:
        raise Refusal(f"The plan on stdin is not valid JSON: {error}.")
    if not isinstance(plan, dict) or not isinstance(plan.get("tasks"), list):
        raise Refusal('The plan on stdin must be a JSON object: {"tasks": [...]}.')
    unknown = sorted(set(plan) - {"tasks"})
    if unknown:
        raise Refusal(f"The plan has an unknown field: {', '.join(unknown)}.")
    tasks = []
    for item in plan["tasks"]:
        if not isinstance(item, dict) or not isinstance(item.get("code"), str):
            raise Refusal(f"Each task in the plan must have a `code`: {item!r}.")
        unknown = sorted(set(item) - PLAN_TASK_FIELDS)
        if unknown:
            raise Refusal(
                f"The task {item['code']} in the plan has an unknown field: "
                f"{', '.join(unknown)}."
            )
        if not TASK_CODE.match(item["code"]):
            raise Refusal(f"{item['code']} in the plan is not a task short code.")
        blockers = item.get("blockers", [])
        if not isinstance(blockers, list):
            raise Refusal(f"The `blockers` of {item['code']} must be a list.")
        for blocker in blockers:
            if not isinstance(blocker, dict) or not isinstance(blocker.get("code"), str):
                raise Refusal(f"Each blocker of {item['code']} must have a `code`.")
            unknown = sorted(set(blocker) - PLAN_BLOCKER_FIELDS)
            if unknown:
                raise Refusal(
                    f"A blocker of {item['code']} has an unknown field: "
                    f"{', '.join(unknown)}."
                )
        column = item.get("column", "")
        if column.strip().lower() in DONE_COLUMNS:
            continue
        tasks.append(
            {
                "code": item["code"],
                "column": column,
                "blockers": [
                    {"code": b["code"], "done": bool(b.get("done"))} for b in blockers
                ],
            }
        )
    return tasks


# --- arguments ----------------------------------------------------------------


def parse_args(args, allowed):
    """A small parser that refuses each unknown option and names it.
    `allowed` maps an option to "value" or "list". Returns (options,
    positional)."""
    options = {name: [] for name, kind in allowed.items() if kind == "list"}
    positional = []
    index = 0
    while index < len(args):
        arg = args[index]
        if arg.startswith("-"):
            if arg not in allowed:
                raise Refusal(f"Unknown option: {arg}.")
            if index + 1 >= len(args):
                raise Refusal(f"The option {arg} needs a value.")
            value = args[index + 1]
            if allowed[arg] == "list":
                options[arg].append(value)
            else:
                options[arg] = value
            index += 2
        else:
            positional.append(arg)
            index += 1
    return options, positional


def session_from(options):
    session = options.get("--session") or os.environ.get("CLAUDE_SESSION_ID", "")
    if not session:
        raise Refusal(
            "No session ID. Give --session <id> (the skill gives "
            "${CLAUDE_SESSION_ID})."
        )
    if not SESSION_ID.match(session):
        raise Refusal(f"The session ID is not valid: {session}.")
    return session


def max_iterations_from(options):
    raw = options.get("--max-iterations", "0")
    if not re.match(r"^[0-9]+$", raw):
        raise Refusal(f"--max-iterations needs a number that is 0 or more, not {raw}.")
    return int(raw)


def begin(session, state):
    try:
        existing = read_state(session)
    except ValueError:
        existing = None
    if existing is not None:
        raise Refusal(
            "A Ralph loop is already active in this session "
            f"({', '.join(existing['tasks'])}). Stop it with /kairos:cancel-ralph first."
        )
    state.update(
        {
            "session_id": session,
            "iteration": 1,
            "completion_promise": COMPLETION_PROMISE,
            "stop_promise": STOP_PROMISE,
            "project_dir": os.environ.get("CLAUDE_PROJECT_DIR", os.getcwd()),
            "started_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        }
    )
    write_state(session, state)


# --- the instruction for each iteration ---------------------------------------

PROCEDURE = """Each task goes through the steps of /kairos:implement (the `implement` skill). Do them in this sequence, and do not skip one:

1. `get_item` the task. If the task is in the Completed column, go to the next task.
2. Do the repository check: the repository of the task must be the repository of this checkout.
3. Make sure that the task is workable: it has acceptance criteria that you can check, and no open blocker.
4. `transition_item` the task to Todo if necessary, then to Active, before the first change.
5. Do the work. Every few steps, `edit_item` short progress notes on the task.
6. Do the completion gate. Run the formatter check, the linter with warnings denied, and the full test suite.
7. For each acceptance criterion, write the command and its output on the task as evidence.
8. Review the diff (the `code-review` skill). Commit to the current branch.
9. Follow the commit rules of the repository and of the user. An example: a rule that forbids a session line in a commit.
10. `transition_item` the task to Completed. Then go to the next task.

Stop the loop, and do not guess, when one of these conditions occurs:
- a task is in the Blocked column;
- a task has no acceptance criteria that you can check;
- the work needs a design decision, a push, a deploy, or a change in a different repository.

To stop the loop:
- Write the reason on the task and on its parent initiative with `edit_item`.
- For a decision or another team, also `transition_item` the task to Blocked, as /kairos:implement says.
- Write the reason in your reply, and end the reply with <promise>{stop}</promise>.

When each task is in the Completed column, end your reply with <promise>{done}</promise>.

Write a promise only when it is true. Do not write a promise to get out of the loop. Refer to each item by its short code."""


def instruction(state):
    tasks = "\n".join(f"  {i + 1}. {code}" for i, code in enumerate(state["tasks"]))
    if state["mode"] == "initiative":
        head = (
            f"Continue the Ralph loop on the initiative {state['initiative']}. "
            "Do the tasks in this sequence (the sequence of their `blocks` edges):"
        )
    else:
        head = "Continue the Ralph loop. Do the tasks in this sequence:"
    text = f"{head}\n{tasks}\n"
    if state.get("skipped"):
        lines = "\n".join(f"  - {s['reason']}" for s in state["skipped"])
        text += f"\nThe loop does not do these tasks:\n{lines}\n"
    text += "\n" + PROCEDURE.format(stop=STOP_PROMISE, done=COMPLETION_PROMISE)
    return text


# --- subcommands --------------------------------------------------------------


def cmd_start(args):
    options, codes = parse_args(
        args,
        {"--session": "value", "--max-iterations": "value", "--missing": "list"},
    )
    if not codes:
        raise Refusal("Give one or more task short codes (for example COLLIERY-T-0001).")
    bad = [code for code in codes if not TASK_CODE.match(code)]
    if bad:
        raise Refusal(
            f"Not a task short code: {', '.join(bad)}. A task short code has the "
            "form PREFIX-T-NNNN. For an initiative, use /kairos:ralph-initiative."
        )
    if options["--missing"]:
        raise Refusal(
            f"Task not found: {', '.join(options['--missing'])}. No loop starts."
        )
    max_iterations = max_iterations_from(options)
    session = session_from(options)
    state = {"mode": "tasks", "tasks": codes, "max_iterations": max_iterations}
    begin(session, state)
    print(started_message(state))


def cmd_start_initiative(args, stdin):
    options, codes = parse_args(args, {"--session": "value", "--max-iterations": "value"})
    if len(codes) != 1:
        raise Refusal(
            "Give exactly one initiative short code"
            + (f", not {len(codes)}: {', '.join(codes)}." if codes else ".")
        )
    initiative = codes[0]
    if not INITIATIVE_CODE.match(initiative):
        raise Refusal(
            f"Not an initiative short code: {initiative}. An initiative short code "
            "has the form PREFIX-I-NNNN. For tasks, use /kairos:ralph."
        )
    max_iterations = max_iterations_from(options)
    session = session_from(options)
    order, skipped = order_initiative_tasks(parse_plan(stdin))
    if not order:
        reasons = "; ".join(s["reason"] for s in skipped) or "it has no open task"
        raise Refusal(f"No task of {initiative} can run: {reasons}. No loop starts.")
    state = {
        "mode": "initiative",
        "initiative": initiative,
        "tasks": order,
        "skipped": skipped,
        "max_iterations": max_iterations,
    }
    begin(session, state)
    print(started_message(state))


def started_message(state):
    limit = state["max_iterations"] or "no limit"
    return (
        f"The Ralph loop is active (iteration 1, limit: {limit}).\n"
        f"The state is in {state_path(state['session_id'])}.\n"
        "To stop it: /kairos:cancel-ralph.\n\n" + instruction(state)
    )


def cmd_cancel(args):
    options, extra = parse_args(args, {"--session": "value"})
    if extra:
        raise Refusal(f"Unknown argument: {', '.join(extra)}.")
    session = session_from(options)
    try:
        state = read_state(session)
    except ValueError:
        remove_state(session)
        print("Removed a Ralph state file that was not valid.")
        return
    if state is None:
        print("No Ralph loop is active in this session.")
        return
    remove_state(session)
    what = state.get("initiative") or ", ".join(state["tasks"])
    print(
        f"Cancelled the Ralph loop on {what} (at iteration {state['iteration']}). "
        "The items keep their columns and their notes."
    )


def last_assistant_text(payload):
    """The text of the last assistant message: the hook input field when
    Claude Code gives it, else the last assistant entry of the transcript."""
    text = payload.get("last_assistant_message")
    if isinstance(text, str):
        return text
    path = payload.get("transcript_path")
    if not path or not os.path.isfile(path):
        return ""
    last = ""
    with open(path, encoding="utf-8") as f:
        for line in f:
            try:
                entry = json.loads(line)
            except ValueError:
                continue
            message = entry.get("message") if isinstance(entry, dict) else None
            if not isinstance(message, dict) or message.get("role") != "assistant":
                continue
            content = message.get("content")
            if isinstance(content, str):
                parts = [content]
            elif isinstance(content, list):
                parts = [
                    c.get("text", "")
                    for c in content
                    if isinstance(c, dict) and c.get("type") == "text"
                ]
            else:
                parts = []
            joined = "\n".join(p for p in parts if p)
            if joined:
                last = joined
    return last


def final_promise(text):
    """The promise at the end of the message, or None. A promise in the
    middle of a message (a quotation) does not count."""
    match = re.search(r"<promise>\s*(.*?)\s*</promise>\s*$", text, re.S)
    if not match:
        return None
    return re.sub(r"\s+", " ", match.group(1))


def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")


def cmd_stop(stdin):
    try:
        payload = json.loads(stdin)
    except ValueError:
        return
    if not isinstance(payload, dict):
        return
    session = payload.get("session_id")
    if not isinstance(session, str) or not SESSION_ID.match(session):
        return
    try:
        state = read_state(session)
    except ValueError as error:
        remove_state(session)
        emit({"systemMessage": f"Ralph loop: the state was not valid ({error}). The loop ended."})
        return
    if state is None:
        return

    promise = final_promise(last_assistant_text(payload))
    if promise == state.get("completion_promise", COMPLETION_PROMISE):
        remove_state(session)
        emit({"systemMessage": "Ralph loop: each task is complete. The loop ended."})
        return
    if promise == state.get("stop_promise", STOP_PROMISE):
        remove_state(session)
        emit({"systemMessage": "Ralph loop: the agent stopped the loop. The reason is in the reply and on the items."})
        return

    limit = state["max_iterations"]
    if limit > 0 and state["iteration"] >= limit:
        remove_state(session)
        emit(
            {
                "systemMessage": (
                    f"Ralph loop: the iteration limit ({limit}) is reached. The loop "
                    "ended. A task can be in an intermediate state: read it with get_item."
                )
            }
        )
        return

    state["iteration"] += 1
    write_state(session, state)
    emit(
        {
            "decision": "block",
            "reason": instruction(state),
            "systemMessage": (
                f"Ralph loop iteration {state['iteration']}"
                + (f" of {limit}" if limit else "")
                + f" | end with <promise>{COMPLETION_PROMISE}</promise> "
                f"or <promise>{STOP_PROMISE}</promise>"
            ),
        }
    )


def main(argv, stdin=sys.stdin):
    if not argv:
        sys.stderr.write("Usage: ralph.py start|start-initiative|cancel|stop ...\n")
        return 2
    command, args = argv[0], argv[1:]
    if command == "stop":
        try:
            cmd_stop(stdin.read())
        except Exception as error:  # a broken hook must never trap a session
            sys.stderr.write(f"Ralph loop: the Stop hook failed: {error}\n")
        return 0
    try:
        if command == "start":
            cmd_start(args)
        elif command == "start-initiative":
            cmd_start_initiative(args, stdin.read())
        elif command == "cancel":
            cmd_cancel(args)
        else:
            raise Refusal(f"Unknown command: {command}.")
    except Refusal as refusal:
        sys.stderr.write(f"Ralph loop: {refusal}\n")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
