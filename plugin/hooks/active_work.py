#!/usr/bin/env python3
"""The active work of the session after a context compaction
(COLLIERY-T-1862). One stdlib module, two hook subcommands:

    active_work.py record     (PostToolUse on the kairos MCP item tools)
    active_work.py compact    (SessionStart with the matcher `compact`)

Why SessionStart `compact` and not PreCompact: in the Claude Code hook
reference, PreCompact has no `additionalContext`, its stdout goes to the
debug log only, and Claude Code discards its `systemMessage`. PostCompact
has no context field either. SessionStart fires again with
`source: "compact"` after each compaction, and its `additionalContext` is a
system reminder that the agent reads on the next request.

Credentials: none (KAIROS-A-0014). The hook does not read Kairos. `record`
keeps a small local record of each task that the agent read, moved or
edited with the kairos MCP tools in this session, from the tool input and
the tool result that Claude Code gives the PostToolUse hook:

    get_item          title, repository, column, parent chain, next step
    transition_item   column
    edit_item         next step (from the `replace` text)
    update_item       title, next step (from the `content`)

The record is `$KAIROS_WORK_STATE_DIR/<session_id>.json`, default
`~/.claude/kairos-work/`. `compact` reads it and the Ralph state of the
session (`ralph.py`), and gives the agent the Active tasks of this
repository, their initiatives, their next steps and the loop. The record
can be stale (a change in the web UI, by a different agent), so the text
always tells the agent to confirm with `get_item` and `board_items`.

The next step is the text after the last `Next step:` (or `Next:`) in the
notes of the task. Without one, the last line of the notes.

Each error gives no output and exit 0: a hook must never block a tool call
or a compaction. Not a Kairos repository (no `.claude/kairos.local.md`):
no output.
"""

import json
import os
import re
import sys
import time
from datetime import datetime, timezone

HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

import ralph  # noqa: E402  (the Ralph state: read_state, state_dir)
import session_start  # noqa: E402  (the wiring: read_frontmatter)

TASK_CODE = re.compile(r"^[A-Z][A-Z0-9]*-T-[0-9]+$")
SESSION_ID = re.compile(r"^[A-Za-z0-9_.-]+$")
KAIROS_TOOL = re.compile(
    r"^mcp__(?:plugin_kairos_)?kairos[A-Za-z0-9_-]*__"
    r"(get_item|transition_item|edit_item|update_item)$"
)
DONE_COLUMNS = ("completed", "done")
SHOWN_COLUMNS = ("active", "blocked")
MAX_TASKS = 5
MAX_TEXT = 240
PRUNE_AFTER_SECONDS = 30 * 24 * 3600

NEXT_STEP = re.compile(
    r"\bNext(?:\s+steps?)?(?:\*\*)?\s*:\s*(?:\*\*)?\s*(\S.*)$", re.IGNORECASE
)
HEADER = re.compile(r"^#\s+([A-Z][A-Z0-9]*-[A-Z]-[0-9]+)\s+—\s+(.+)$")
REPOSITORY = re.compile(r"·\s*repository:\s*([^\s(·]+)")
COLUMN = re.compile(r"^-\s*board:\s*.*?/\s*column:\s*(.+)$")
PARENT_CHAIN = re.compile(r"^-\s*parent chain:\s*(.+)$")
TRANSITIONED = re.compile(r"^Transitioned\s+\S+:\s*.*?->\s*(.+?)\.?$")


# --- the record ---------------------------------------------------------------


def work_dir():
    configured = os.environ.get("KAIROS_WORK_STATE_DIR")
    if configured:
        return configured
    return os.path.join(os.path.expanduser("~"), ".claude", "kairos-work")


def record_path(session):
    return os.path.join(work_dir(), f"{session}.json")


def read_record(session):
    """The record of the session; an empty record when there is none or
    when the file is not valid."""
    try:
        with open(record_path(session), encoding="utf-8") as f:
            record = json.load(f)
    except (OSError, ValueError):
        return {"tasks": {}}
    if not isinstance(record, dict) or not isinstance(record.get("tasks"), dict):
        return {"tasks": {}}
    return record


def write_record(session, record):
    os.makedirs(work_dir(), exist_ok=True)
    path = record_path(session)
    if not os.path.exists(path):
        prune()
    temporary = f"{path}.tmp.{os.getpid()}"
    with open(temporary, "w", encoding="utf-8") as f:
        json.dump(record, f, indent=2)
        f.write("\n")
    os.replace(temporary, path)


def prune():
    """Remove the records of old sessions. Errors are ignored."""
    limit = time.time() - PRUNE_AFTER_SECONDS
    try:
        names = os.listdir(work_dir())
    except OSError:
        return
    for name in names:
        path = os.path.join(work_dir(), name)
        try:
            if name.endswith(".json") and os.path.getmtime(path) < limit:
                os.remove(path)
        except OSError:
            pass


# --- reading the tool calls ---------------------------------------------------


def response_text(response):
    """The text of an MCP tool result: a string, a list of content blocks,
    or an object with `content` blocks."""
    if isinstance(response, str):
        return response
    if isinstance(response, dict):
        if isinstance(response.get("content"), (list, str)):
            return response_text(response["content"])
        if isinstance(response.get("text"), str):
            return response["text"]
        return ""
    if isinstance(response, list):
        return "\n".join(
            block.get("text", "")
            for block in response
            if isinstance(block, dict) and isinstance(block.get("text"), str)
        )
    return ""


def clip(text):
    text = re.sub(r"\s+", " ", text).strip()
    if len(text) > MAX_TEXT:
        text = text[: MAX_TEXT - 3].rstrip() + "..."
    return text


def next_step(text):
    """The text after the last `Next step:` in `text`, or None."""
    found = None
    for line in text.splitlines():
        match = NEXT_STEP.search(line)
        if match:
            found = match.group(1)
    if found is None:
        return None
    return clip(found.replace("**", ""))


def last_note(text):
    """The last line of `text` that has words, without the list mark."""
    for line in reversed(text.splitlines()):
        line = line.strip()
        if line and not line.startswith("#"):
            return clip(re.sub(r"^[-*]\s+", "", line))
    return None


def parse_get_item(text):
    """The fields of a get_item result. Returns {} when the text is not a
    get_item result."""
    lines = text.splitlines()
    if not lines:
        return {}
    header = HEADER.match(lines[0].strip())
    if not header:
        return {}
    fields = {"code": header.group(1), "title": header.group(2).strip()}
    # The order of get_item: the header lines, then `## Metadata`,
    # `## Relationships` and `## Development` when they have lines, then
    # `## Content` up to the end. The content can have `## ` headings of
    # its own, so the first `## Content` starts the content.
    content = []
    section = None
    for line in lines[1:]:
        if section == "content":
            content.append(line)
            continue
        if line.startswith("## "):
            section = "content" if line.strip() == "## Content" else line.strip()
            continue
        if section is None:
            repository = REPOSITORY.search(line)
            if repository:
                fields["repository"] = repository.group(1)
            column = COLUMN.match(line.strip())
            if column:
                fields["column"] = column.group(1).strip()
        elif section == "## Relationships":
            chain = PARENT_CHAIN.match(line.strip())
            if chain:
                fields["initiative"] = chain.group(1).split(" <- ")[0].strip()
    body = "\n".join(content)
    step = next_step(body)
    if step:
        fields["next_step"] = step
    else:
        note = last_note(body)
        if note:
            fields["last_note"] = note
    return fields


def changes_from(tool, tool_input, text):
    """(code, changes) for one kairos tool call, or (None, None)."""
    code = tool_input.get("short_code")
    if not isinstance(code, str):
        return None, None
    changes = {}
    if tool == "get_item":
        fields = parse_get_item(text)
        if fields.get("code") != code:
            return None, None
        fields.pop("code")
        changes = fields
        if "next_step" in changes:
            changes["last_note"] = None
        elif "last_note" in changes:
            changes["next_step"] = None
    elif tool == "transition_item":
        moved = TRANSITIONED.match(text.strip())
        column = moved.group(1) if moved else tool_input.get("to_column")
        if isinstance(column, str) and column.strip():
            changes["column"] = column.strip()
    elif tool in ("edit_item", "update_item"):
        written = tool_input.get("replace" if tool == "edit_item" else "content")
        if isinstance(written, str):
            step = next_step(written)
            if step:
                changes["next_step"] = step
                changes["last_note"] = None
        title = tool_input.get("title")
        if tool == "update_item" and isinstance(title, str) and title.strip():
            changes["title"] = title.strip()
        changes["edited"] = True
    return code, changes


def cmd_record(payload):
    session = payload.get("session_id")
    if not isinstance(session, str) or not SESSION_ID.match(session):
        return
    tool_name = payload.get("tool_name")
    match = KAIROS_TOOL.match(tool_name) if isinstance(tool_name, str) else None
    if not match:
        return
    tool_input = payload.get("tool_input")
    if not isinstance(tool_input, dict):
        return
    code, changes = changes_from(
        match.group(1), tool_input, response_text(payload.get("tool_response"))
    )
    if code is None:
        return
    record = read_record(session)
    if TASK_CODE.match(code):
        task = record["tasks"].get(code)
        if not isinstance(task, dict):
            task = {}
        for key, value in changes.items():
            if value is None:
                task.pop(key, None)
            else:
                task[key] = value
        task["updated_at"] = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        record["tasks"][code] = task
    record["session_id"] = session
    write_record(session, record)


# --- the context after a compaction -------------------------------------------


def column_of(task):
    column = task.get("column")
    return column.strip().lower() if isinstance(column, str) else ""


def active_tasks(record, repository):
    """The tasks of the record to name: in Active or Blocked, or edited by
    this session in an unknown column; of this repository when the record
    knows the repository. The newest first."""
    chosen = []
    for code, task in record["tasks"].items():
        if not TASK_CODE.match(code) or not isinstance(task, dict):
            continue
        column = column_of(task)
        if column in DONE_COLUMNS:
            continue
        if column not in SHOWN_COLUMNS and not (column == "" and task.get("edited")):
            continue
        if repository and task.get("repository") not in (None, repository):
            continue
        chosen.append((str(task.get("updated_at", "")), code, task))
    chosen.sort(reverse=True)
    return [(code, task) for _, code, task in chosen[:MAX_TASKS]]


def task_line(code, task):
    title = task.get("title")
    column = task.get("column") or "column not known"
    line = f"- {code}"
    if title:
        line += f' "{title}"'
    line += f" ({column})"
    if task.get("initiative"):
        line += f", initiative {task['initiative']}"
    else:
        line += ", initiative not known"
    line += "."
    if task.get("next_step"):
        line += f" Next step: {task['next_step']}"
    elif task.get("last_note"):
        line += f" Last note: {task['last_note']}"
    else:
        line += " No next step recorded."
    return line


def loop_lines(session, record):
    try:
        state = ralph.read_state(session)
    except Exception:
        return []
    if state is None:
        return []
    done = {
        code
        for code, task in record["tasks"].items()
        if isinstance(task, dict) and column_of(task) in DONE_COLUMNS
    }
    left = [code for code in state["tasks"] if code not in done]
    limit = state["max_iterations"]
    where = f" on the initiative {state['initiative']}" if state.get("initiative") else ""
    iteration = (
        f"iteration {state['iteration']} of {limit}"
        if limit
        else f"iteration {state['iteration']} (no limit)"
    )
    count = f"{len(left)} task{'' if len(left) == 1 else 's'} left"
    lines = [
        f"A Ralph loop of this session is active{where}: {iteration}, {count}"
        + (f": {', '.join(left)}." if left else ".")
    ]
    if left:
        lines.append(
            f"Continue the loop with {left[0]}. The Stop hook gives the steps; "
            "/kairos:cancel-ralph stops the loop."
        )
    return lines


def build_context(values, record, session):
    repository = values.get("repository")
    board = values.get("team_board") or "of your team"
    tasks = active_tasks(record, repository)
    loop = loop_lines(session, record)
    filters = "`column=Active`"
    if repository:
        filters = f"`repository={repository}`, " + filters
    read_live = (
        f"call `board_items` for the board {board} with {filters}. "
        "Then `get_item` each task"
    )
    lines = ["Kairos: Claude Code compacted the context."]
    if tasks:
        lines.append(
            "Your active tasks, from the local record of this session "
            "(the hook does not read Kairos):"
        )
        lines.extend(task_line(code, task) for code, task in tasks)
    else:
        lines.append("The local record of this session has no active task.")
    lines.extend(loop)
    if tasks or loop:
        lines.append(
            "The record can be stale. Before you continue, `get_item` each task "
            f"to confirm its column and its last note. To find each Active task, {read_live}."
        )
    else:
        lines.append(f"To find your Active tasks, {read_live}.")
    lines.append(
        "Write progress notes on the task with `edit_item`, and end each note "
        "with a `Next step:` line."
    )
    return "\n".join(lines)


def cmd_compact(payload):
    session = payload.get("session_id")
    if not isinstance(session, str) or not SESSION_ID.match(session):
        return
    project_dir = os.environ.get("CLAUDE_PROJECT_DIR") or os.getcwd()
    config_path = os.path.join(project_dir, ".claude", "kairos.local.md")
    if not os.path.isfile(config_path):
        return
    values = session_start.read_frontmatter(config_path)
    context = build_context(values, read_record(session), session)
    sys.stdout.write(
        json.dumps(
            {
                "hookSpecificOutput": {
                    "hookEventName": "SessionStart",
                    "additionalContext": context,
                }
            }
        )
        + "\n"
    )


def main(argv, stdin=sys.stdin):
    command = argv[0] if argv else ""
    if command not in ("record", "compact"):
        sys.stderr.write("Usage: active_work.py record|compact (hook input on stdin)\n")
        return 2
    try:
        payload = json.loads(stdin.read())
        if not isinstance(payload, dict):
            return 0
        if command == "record":
            cmd_record(payload)
        else:
            cmd_compact(payload)
    except Exception as error:  # a hook must never block a tool call or a compaction
        sys.stderr.write(f"kairos active work: the hook failed: {error}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
