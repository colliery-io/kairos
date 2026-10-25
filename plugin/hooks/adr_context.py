#!/usr/bin/env python3
"""The ADRs of a file before an edit (KAIROS-T-0702, COLLIERY-I-0659): a
PreToolUse hook on Write, Edit and MultiEdit. When the file cites ADRs by
short code (`KAIROS-A-0031`), it gives the agent one short block: the code,
the title and the column of each cited ADR, the successor of a superseded
ADR, and the amendments. Then the agent edits with the decisions of the
file in view, not only the decisions of the repository.

The states come from `kairos adrs check <file> --json` (the resolver of
KAIROS-T-0701), with the exit code and the JSON only (KAIROS-A-0031 rule
4). The hook runs only in a wired checkout: the nearest folder with `.git`
above the file has the wiring (`.claude/kairos.local.md`, or a
`[[project]]` of the config of the person), or the session folder above it
has.

Once per session per file: the record is
`$KAIROS_ADR_STATE_DIR/<session_id>.json`, default `~/.claude/kairos-adr/`,
with the files that got the block and the states of the ADRs that the
session read (the cache: an ADR is read once per session). Records older
than 30 days are removed.

It fails open. A file that cites nothing, a new file, no `kairos` on PATH,
no deployment, a check that is skipped, a timeout or any error: no output
and exit 0. The output is `additionalContext` only, with no
`permissionDecision`: the normal permission flow of the tool decides. No
credentials of its own: the CLI uses the login or the key of the person.
Python stdlib only.
"""

import json
import os
import re
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

import index_update  # noqa: E402  (checkout_of, is_below, is_wired, project_dir)

WRITE_TOOLS = ("Write", "Edit", "MultiEdit")
SESSION_ID = re.compile(r"^[A-Za-z0-9_.-]+$")
# The same codes as the scanner of `kairos adrs check`: an upper-case
# prefix (a letter, then letters or digits), `-A-`, four digits or more. A
# letter, a digit, `_` or `-` just before it, or a letter, a digit or `_`
# just after it, makes it part of a longer word.
# shared with kairos_index::is_adr_code (KAIROS-T-0703)
ADR_CODE = re.compile(r"(?<![A-Za-z0-9_-])([A-Z][A-Z0-9]*-A-[0-9]{4,})(?![A-Za-z0-9_])")
MAX_FILE_BYTES = 2 * 1024 * 1024
MAX_ADRS = 12
CHECK_SECONDS = 8
PRUNE_AFTER_SECONDS = 30 * 24 * 3600

HEAD = "ADRs that {path} cites (Kairos, once per file in this session):"
TAIL = (
    "Read an ADR with `get_item` before you change the code that it rules. "
    "Cite the successor of a superseded ADR, not the old code."
)


def state_dir():
    configured = os.environ.get("KAIROS_ADR_STATE_DIR")
    if configured:
        return configured
    return os.path.join(os.path.expanduser("~"), ".claude", "kairos-adr")


def record_path(session):
    return os.path.join(state_dir(), f"{session}.json")


def read_record(session):
    try:
        with open(record_path(session), encoding="utf-8") as handle:
            record = json.load(handle)
    except (OSError, ValueError):
        record = None
    if not isinstance(record, dict):
        record = {}
    if not isinstance(record.get("files"), list):
        record["files"] = []
    if not isinstance(record.get("adrs"), dict):
        record["adrs"] = {}
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


def cited_codes(text):
    """The ADR codes of `text`, once each, in the order of the first
    citation (pure; testable)."""
    seen = []
    for code in ADR_CODE.findall(text or ""):
        if code not in seen:
            seen.append(code)
    return seen


def read_text(path):
    """The text of a file, or None: no file, too large, or not UTF-8."""
    try:
        if os.path.getsize(path) > MAX_FILE_BYTES:
            return None
        with open(path, encoding="utf-8") as handle:
            return handle.read()
    except (OSError, UnicodeDecodeError):
        return None


def is_wired_checkout(path):
    """The checkout of the file has the wiring, or the session folder above
    it has. None when the file is in no checkout."""
    checkout = index_update.checkout_of(path)
    if checkout is None:
        return None
    project = index_update.project_dir()
    if index_update.is_wired(checkout):
        return checkout
    if index_update.is_below(checkout, project) and index_update.is_wired(project):
        return checkout
    return None


def check(path, checkout):
    """The states of the ADRs that `path` cites, by code, from `kairos adrs
    check`. None when the CLI is not there, the check is skipped, or it
    fails for any reason but a failed citation (exit 1 with a report)."""
    binary = shutil.which("kairos")
    if not binary:
        return None
    env = dict(os.environ, KAIROS_UPDATE_CHECK="0", KAIROS_QUIET="1")
    try:
        out = subprocess.run(
            [binary, "adrs", "check", path, "--json"],
            capture_output=True,
            text=True,
            timeout=CHECK_SECONDS,
            cwd=checkout,
            env=env,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    # 0: no citation fails; 1 with a report: a citation fails. Each other
    # code (2 auth, 64 an older CLI with no `adrs check`) gives nothing.
    if out.returncode not in (0, 1):
        return None
    try:
        report = json.loads(out.stdout)
    except ValueError:
        return None
    if not isinstance(report, dict) or report.get("skipped"):
        return None
    adrs = report.get("adrs")
    if not isinstance(adrs, list):
        return None
    return {
        adr["code"]: adr
        for adr in adrs
        if isinstance(adr, dict) and isinstance(adr.get("code"), str)
    }


def refs_text(refs):
    """`CODE (title), CODE (title)` of a list of refs (pure; testable)."""
    parts = []
    for ref in refs if isinstance(refs, list) else []:
        if isinstance(ref, dict) and ref.get("code"):
            title = ref.get("title")
            parts.append(f"{ref['code']} ({title})" if title else ref["code"])
    return ", ".join(parts)


def adr_line(code, state):
    """The line of one ADR (pure; testable)."""
    if not isinstance(state, dict) or not state.get("found"):
        if isinstance(state, dict) and state.get("status") == "not-in-snapshot":
            return f"- {code}: not in the snapshot of the check."
        return f"- {code}: no ADR has this code. Do not cite it."
    current = state.get("current_code") or code
    name = current if current == code else f"{code} (now {current})"
    line = f"- {name} [{state.get('status') or 'unknown'}]: {state.get('title') or ''}".rstrip()
    if state.get("archived_at"):
        line += " ARCHIVED."
    successors = refs_text(state.get("superseded_by"))
    if successors:
        line += f"\n  Superseded by {successors}."
    amendments = refs_text(state.get("amended_by"))
    if amendments:
        line += f"\n  Amended by {amendments}."
    # KAIROS-A-0070: when to look at the ADR again.
    when, on = state.get("revisit_when"), state.get("revisit_on")
    if isinstance(when, str) and when or isinstance(on, str) and on:
        line += f"\n  Revisit when: {when or 'no condition'}"
        line += f" (on {on})." if on else "."
    return line


def build_context(path, codes, states):
    """The block for one file (pure; testable)."""
    lines = [HEAD.format(path=path)]
    for code in codes[:MAX_ADRS]:
        lines.append(adr_line(code, states.get(code)))
    if len(codes) > MAX_ADRS:
        lines.append(
            f"- and {len(codes) - MAX_ADRS} more. Run `kairos adrs check {path}` for each one."
        )
    lines.append(TAIL)
    return "\n".join(lines)


def main():
    try:
        payload = json.loads(sys.stdin.read() or "null")
        if not isinstance(payload, dict) or payload.get("tool_name") not in WRITE_TOOLS:
            return
        tool_input = payload.get("tool_input")
        if not isinstance(tool_input, dict):
            return
        file_path = tool_input.get("file_path")
        session = payload.get("session_id")
        if not isinstance(file_path, str) or not file_path:
            return
        if not isinstance(session, str) or not SESSION_ID.match(session):
            return
        path = os.path.realpath(os.path.join(index_update.project_dir(), file_path))
        codes = cited_codes(read_text(path))
        if not codes:
            return
        record = read_record(session)
        if path in record["files"]:
            return
        checkout = is_wired_checkout(path)
        if checkout is None:
            return
        cache = record["adrs"]
        if any(code not in cache for code in codes):
            states = check(path, checkout)
            if states is None:
                return
            cache.update(states)
        record["files"].append(path)
        record["session_id"] = session
        write_record(session, record)
        shown = os.path.relpath(path, checkout)
        json.dump(
            {
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "additionalContext": build_context(shown, codes, cache),
                }
            },
            sys.stdout,
        )
    except Exception:  # never block a write
        return


if __name__ == "__main__":
    main()
