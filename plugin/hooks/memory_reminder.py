#!/usr/bin/env python3
"""The memory reminder (KAIROS-T-0502, COLLIERY-I-0628): a PreToolUse hook
on Write, Edit and MultiEdit. When the file is in the agent's local memory
folder, it reminds: a fact about the system under work belongs in Kairos
(an ADR, the spec, a note on the initiative); local memory holds the
preferences and the tooling of the person. It reminds; it never refuses
(Dylan, 2026-10-15), and it does not read the text of the write.

A memory folder is `.claude/projects/<name>/memory/` (any parent), or the
folder in CLAUDE_MEMORY_DIR when it is set.

Once per session per file: the record is
`$KAIROS_MEMORY_STATE_DIR/<session_id>.json`, default
`~/.claude/kairos-memory/`, with the paths that got the reminder. Records
older than 30 days are removed.

The ADR board comes from `adr_board` of the wiring (read with
session_start.wiring: `kairos config show`); without one the text says
`my_boards`.

The output is `additionalContext` only. No `permissionDecision`: the
normal permission flow of the tool decides, and "allow" would skip the
prompt of the person. Each error gives no output and exit 0. No
credentials, no network. Python stdlib only.
"""

import json
import os
import re
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

import session_start  # noqa: E402  (the wiring)

WRITE_TOOLS = ("Write", "Edit", "MultiEdit")
SESSION_ID = re.compile(r"^[A-Za-z0-9_.-]+$")
MEMORY_FOLDER = re.compile(r"(^|/)\.claude/projects/[^/]+/memory/")
PRUNE_AFTER_SECONDS = 30 * 24 * 3600

REMINDER = (
    "Kairos: this write goes to local memory ({path}). Local memory holds the "
    "preferences and the tooling of the person. A fact about the system under "
    "work belongs in Kairos, where the next session and the next person find "
    "it: an ADR (`create_item` with `item_type: adr`, {board}, `parent: <the "
    "task or the initiative>`), the spec document (`edit_item`), or a note on "
    "the initiative (`append_note`). COLLIERY-I-0628. The write goes on."
)


def state_dir():
    configured = os.environ.get("KAIROS_MEMORY_STATE_DIR")
    if configured:
        return configured
    return os.path.join(os.path.expanduser("~"), ".claude", "kairos-memory")


def record_path(session):
    return os.path.join(state_dir(), f"{session}.json")


def read_record(session):
    try:
        with open(record_path(session), encoding="utf-8") as handle:
            record = json.load(handle)
    except (OSError, ValueError):
        return {"paths": []}
    if not isinstance(record, dict) or not isinstance(record.get("paths"), list):
        return {"paths": []}
    return record


def write_record(session, record):
    os.makedirs(state_dir(), exist_ok=True)
    path = record_path(session)
    if not os.path.exists(path):
        prune()
    temporary = f"{path}.tmp.{os.getpid()}"
    with open(temporary, "w", encoding="utf-8") as handle:
        json.dump(record, handle, indent=2)
        handle.write("\n")
    os.replace(temporary, path)


def prune():
    """Remove the records of old sessions. Errors are ignored."""
    limit = time.time() - PRUNE_AFTER_SECONDS
    try:
        names = os.listdir(state_dir())
    except OSError:
        return
    for name in names:
        path = os.path.join(state_dir(), name)
        try:
            if name.endswith(".json") and os.path.getmtime(path) < limit:
                os.remove(path)
        except OSError:
            pass


def is_memory_path(file_path, memory_dir):
    """True when `file_path` is in a memory folder (pure; testable)."""
    if not isinstance(file_path, str) or not file_path:
        return False
    normalized = file_path.replace("\\", "/")
    if MEMORY_FOLDER.search(normalized):
        return True
    if memory_dir:
        root = os.path.normpath(memory_dir)
        target = os.path.normpath(file_path)
        return target == root or target.startswith(root + os.sep)
    return False


def adr_board():
    return (session_start.wiring() or {}).get("adr_board", "")


def build_context(path, board):
    """The reminder for one path (pure; testable)."""
    if board:
        where = f"`board: {board}`"
    else:
        where = "`board: <the ADR board of your team, from `my_boards`>`"
    return REMINDER.format(path=path, board=where)


def main():
    try:
        payload = json.loads(sys.stdin.read() or "null")
        if not isinstance(payload, dict) or payload.get("tool_name") not in WRITE_TOOLS:
            return
        tool_input = payload.get("tool_input")
        if not isinstance(tool_input, dict):
            return
        file_path = tool_input.get("file_path")
        if not is_memory_path(file_path, os.environ.get("CLAUDE_MEMORY_DIR")):
            return
        session = payload.get("session_id")
        if not isinstance(session, str) or not SESSION_ID.match(session):
            return
        record = read_record(session)
        if file_path in record["paths"]:
            return
        record["paths"].append(file_path)
        record["session_id"] = session
        write_record(session, record)
        json.dump(
            {
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "additionalContext": build_context(file_path, adr_board()),
                }
            },
            sys.stdout,
        )
    except Exception as error:  # never block a write (KAIROS-T-0708: the reason goes to the log)
        sys.stderr.write(f"kairos memory reminder: the hook failed: {error}\n")
        return


if __name__ == "__main__":
    main()
