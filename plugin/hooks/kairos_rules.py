#!/usr/bin/env python3
"""The Kairos rules, once per session (KAIROS-T-0498, COLLIERY-I-0631): a
PreToolUse hook that gives the rules as additional context on the first
Kairos tool call of a session, and then stays silent.

A Kairos tool call is one of:

    mcp__plugin_kairos_*__<tool>, mcp__kairos*__<tool>   the MCP tools
    Skill with a skill named kairos* (kairos:implement, kairos-vocabulary)
    Bash whose command runs `kairos` (also after `env`, `env -u X`, VAR=x)

The marker is `$KAIROS_RULES_STATE_DIR/<session_id>-<RULES_VERSION>`,
default `~/.claude/kairos-rules/`. A marker that is there means silence.
In a wired folder `session_start.py` gives the rules at the start of the
session and writes the marker (KAIROS-T-0525), and `active_work.py compact`
gives them again after a compaction; this hook then covers the sessions
that start with no wiring.
The marker carries the version of the rules, so a plugin update with a
higher RULES_VERSION gives the new text once more. Markers older than 30
days are removed.

The rules cite items. With `deployment_url` in `.claude/kairos.local.md`
(read with session_start.read_frontmatter), each short code in the rules is
the link `<deployment_url>/items/<CODE>`, and the rules say to cite that
way. With no URL, the codes stay bare and the rules say so.

Each error gives no output and exit 0: the hook never blocks a tool call.
No credentials, no network. Python stdlib only.
"""

import json
import os
import re
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

import git_guard  # noqa: E402  (simple_commands, words_of)
import session_start  # noqa: E402  (read_frontmatter)

RULES_VERSION = 2

KAIROS_TOOL = re.compile(r"^mcp__(?:plugin_kairos_)?kairos[A-Za-z0-9_-]*__")
SESSION_ID = re.compile(r"^[A-Za-z0-9_.-]+$")
SHORT_CODE = re.compile(r"(?<![/\w-])([A-Z][A-Z0-9]*-[A-Z]-[0-9]+)")
PRUNE_AFTER_SECONDS = 30 * 24 * 3600
ENV_OPTIONS_WITH_VALUE = ("-u", "--unset", "-C", "--chdir")

RULES = """Kairos rules (once per session, from the kairos plugin):
- {cite}
- Name work by its short code and the words of Kairos: Backlog, Todo, Active, Completed; task, initiative, ADR, document. Not "ticket", "sprint", "epic", "done".
- Ground before you plan: read the ADRs and the documents of the repository and the documents of the parent initiative, and name what you read (COLLIERY-I-0627). Two routes give them: `get_repository <slug>` (the items that impact the repository, and the ADRs of the owner team) and `team_pages <team>` (the tree of the documents and the ADRs of the team).
- A decision about the system under work goes to Kairos: an ADR, the spec, or a note on the initiative. Not to local memory (COLLIERY-I-0628).
- Mark what you did not check as "not checked", in notes and in replies."""

CITE_WITH_URL = (
    "Cite every item as a link: `{url}/items/<CODE>`, for example {example}. "
    "Never a bare short code when a deployment URL is wired."
)
CITE_BARE = (
    "Cite every item by its bare short code (no deployment_url is wired in "
    ".claude/kairos.local.md; `/kairos:bootstrap` wires one)."
)


def rules_version():
    override = os.environ.get("KAIROS_RULES_VERSION_OVERRIDE")
    return int(override) if override and override.isdigit() else RULES_VERSION


def state_dir():
    configured = os.environ.get("KAIROS_RULES_STATE_DIR")
    if configured:
        return configured
    return os.path.join(os.path.expanduser("~"), ".claude", "kairos-rules")


def marker_path(session):
    return os.path.join(state_dir(), f"{session}-{rules_version()}")


def prune():
    """Remove the markers of old sessions. Errors are ignored."""
    limit = time.time() - PRUNE_AFTER_SECONDS
    try:
        names = os.listdir(state_dir())
    except OSError:
        return
    for name in names:
        path = os.path.join(state_dir(), name)
        try:
            if os.path.getmtime(path) < limit:
                os.remove(path)
        except OSError:
            pass


def linkify(text, url):
    """Each bare short code in `text` as `<url>/items/<CODE>`; the text as
    it is with no url (pure; testable)."""
    url = (url or "").rstrip("/")
    if not url:
        return text
    return SHORT_CODE.sub(lambda match: f"{url}/items/{match.group(1)}", text)


def build_context(url):
    """The rules text for a deployment url, or for none (pure; testable)."""
    url = (url or "").rstrip("/")
    if url:
        cite = CITE_WITH_URL.format(url=url, example=f"{url}/items/COLLIERY-I-0627")
    else:
        cite = CITE_BARE
    return linkify(RULES.format(cite=cite), url)


def runs_kairos(command):
    """True when one simple command of a Bash command line runs `kairos`."""
    if not isinstance(command, str):
        return False
    for simple in git_guard.simple_commands(command):
        words = git_guard.words_of(simple)
        while words and words[0].startswith("-"):
            drop = 2 if words[0] in ENV_OPTIONS_WITH_VALUE else 1
            words = words[drop:]
            while words and git_guard.ASSIGNMENT.match(words[0]):
                words = words[1:]
        if words and words[0] == "kairos":
            return True
    return False


def is_kairos_call(tool_name, tool_input):
    """True for a Kairos MCP tool, a kairos skill or a `kairos` command."""
    if not isinstance(tool_name, str):
        return False
    if KAIROS_TOOL.match(tool_name):
        return True
    tool_input = tool_input if isinstance(tool_input, dict) else {}
    if tool_name == "Skill":
        skill = tool_input.get("skill")
        return isinstance(skill, str) and skill.startswith("kairos")
    if tool_name == "Bash":
        return runs_kairos(tool_input.get("command"))
    return False


def deployment_url():
    return (session_start.wiring() or {}).get("deployment_url", "")


def mark(session):
    """Write the marker of `session`: the rules were given. Returns False
    when the session id is not usable or the marker was there already
    (KAIROS-T-0525: session_start marks too, so a wired session gets the
    rules at its start and this hook then stays silent)."""
    if not isinstance(session, str) or not SESSION_ID.match(session):
        return False
    marker = marker_path(session)
    if os.path.exists(marker):
        return False
    os.makedirs(state_dir(), exist_ok=True)
    prune()
    with open(marker, "w", encoding="utf-8") as handle:
        handle.write("")
    return True


def main():
    try:
        payload = json.loads(sys.stdin.read() or "null")
        if not isinstance(payload, dict):
            return
        if not is_kairos_call(payload.get("tool_name"), payload.get("tool_input")):
            return
        if not mark(payload.get("session_id")):
            return
        json.dump(
            {
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "additionalContext": build_context(deployment_url()),
                }
            },
            sys.stdout,
        )
    except Exception:  # never block a tool call
        return


if __name__ == "__main__":
    main()
