#!/usr/bin/env python3
"""The code tools before grep (KAIROS-T-0757, COLLIERY-I-0671): a nudge on
a search over an indexed checkout, and the index instruction for each
subagent.

Two modes:

- `tool` (PreToolUse on Grep, Glob and Bash): find the path of the search
  (the `path` of Grep and Glob; the path argument of grep, rg, ag, ack,
  find or fd in a Bash command; else the folder of the call). When the
  checkout of that path has an index in the user cache, give one short
  message: the checkout, the index tools that answer the same question,
  and that grep stays right for literal text. The hook allows the call.
  The message comes on the first call of a session for a checkout, then
  on each tenth call (`index-first-<session_id>` in the state folder of
  the checkout). A grep that reads a pipe filters output: no message.
- `subagent` (SubagentStart, each agent type): give the index instruction
  of the session start (the code tools of `kairos-code`), with the
  checkouts of the session that have an index.

Each call writes one line to `tool-calls.log` in the state folder of the
checkout: the time, the session, `agent_id`, `agent_type`, the hook event,
the tool, and `nudge` or `quiet`. The measurement (KAIROS-T-0758) reads it
to tell the main agent from the subagents. The hook input of a subagent
carries `agent_id` and `agent_type` (the hooks reference). What a Workflow
agent carries: not checked. `git grep` counts as grep; a searcher after
`xargs` too.

Each error gives no output and exit 0: the hook never blocks a tool call
(KAIROS-T-0708). No credentials, no network. Python stdlib only.
"""

import datetime
import json
import os
import re
import shlex
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

import git_guard  # noqa: E402  (words_of)
import index_update  # noqa: E402  (state_dir, checkout_top, has_index, texts)

SEARCHERS = ("grep", "egrep", "fgrep", "rg", "ag", "ack", "find", "fd")
# The options of grep, rg, ag, ack and fd that take a value as the next word.
OPTIONS_WITH_VALUE = (
    "-e", "-f", "-A", "-B", "-C", "-m", "-t", "-T", "-g", "-E", "-d", "-j", "-M",
    "--regexp", "--file", "--include", "--exclude", "--exclude-dir", "--type",
    "--type-not", "--glob", "--iglob", "--max-count", "--max-depth", "--threads",
    "--context", "--after-context", "--before-context", "--extension",
)
PATTERN_OPTIONS = ("-e", "--regexp", "-f", "--file")
EVERY = 10
PRUNE_AFTER_SECONDS = 30 * 24 * 3600
COUNTER = "index-first-{session}"
LOG = "tool-calls.log"
SESSION_ID = re.compile(r"^[A-Za-z0-9_.-]+$")
CHAINS = re.compile(r"\|\||&&|;|\n")

MESSAGE = (
    "Index first: the checkout {name} has a code index. `code_search` answers "
    '"what does X". `symbol` and `references` answer a name. `callers` answers '
    "who calls.\n"
    "grep stays right for literal text: a log line, a config key, an error message."
)


def stages_of(command):
    """The commands of a shell line as lists of words, in the order of the
    line: each chain (`;`, `&&`, `||`, a new line) gives its pipe stages.
    Quotes are read (`grep "a|b"` is one word). Pure."""
    try:
        lexer = shlex.shlex(command, posix=True, punctuation_chars=True)
        lexer.whitespace_split = True
        tokens = list(lexer)
    except ValueError:
        tokens = command.replace("|", " | ").replace(";", " ; ").split()
    chains = [[[]]]
    for token in tokens:
        if token in (";", "&&", "||", "\n"):
            chains.append([[]])
        elif token == "|":
            chains[-1].append([])
        elif token.startswith("|") or token.endswith("|"):
            # `2>&1|`, `||x`: the lexer glued the pipe to a neighbour.
            chains[-1].append([])
        else:
            chains[-1][-1].append(token)
    return chains


def search_words(words):
    """The words of a search over files (`grep ...`), or None: a searcher,
    also after `xargs` or as `git grep`."""
    while words and os.path.basename(words[0]) == "xargs":
        words = words[1:]
        while words and words[0].startswith("-"):
            words = words[2:] if words[0] in ("-I", "-n", "-P", "-L", "-s", "-d") else words[1:]
    if len(words) > 1 and os.path.basename(words[0]) == "git" and words[1] == "grep":
        words = words[1:]
    if words and os.path.basename(words[0]) in SEARCHERS:
        return words
    return None


def search_paths(command):
    """(True, paths) when one command of the line runs a searcher over
    files, with its path arguments; (False, []) when none does. A searcher
    after a pipe reads the output of another command: not a search over
    files. Pure."""
    found = False
    paths = []
    if not isinstance(command, str):
        return found, paths
    for chain in stages_of(command):
        words = search_words(git_guard.words_of(" ".join(shlex.quote(w) for w in chain[0])))
        if words is None:
            continue
        found = True
        name = os.path.basename(words[0])
        args = []
        rest = words[1:]
        # `-e PATTERN` and `-f FILE` give the pattern: each argument is a path.
        pattern_given = False
        while rest:
            word = rest.pop(0)
            if word == "--":
                args.extend(rest)
                break
            if word in OPTIONS_WITH_VALUE:
                pattern_given |= word in PATTERN_OPTIONS
                rest = rest[1:]
            elif not word.startswith("-"):
                args.append(word)
        if name == "find":
            candidates = args[:1]
        elif name == "fd":
            candidates = args[1:2]
        else:
            candidates = args if pattern_given else args[1:]
        paths.extend(candidates)
    return found, paths


def search_target(tool_name, tool_input, cwd):
    """The folder that the call searches, or None when the call is not a
    search: the `path` of Grep and Glob, the first path of a searcher in a
    Bash command, else `cwd`."""
    tool_input = tool_input if isinstance(tool_input, dict) else {}
    if tool_name in ("Grep", "Glob"):
        path = tool_input.get("path")
        return os.path.join(cwd, path) if isinstance(path, str) and path else cwd
    if tool_name == "Bash":
        found, paths = search_paths(tool_input.get("command"))
        if not found:
            return None
        return os.path.join(cwd, paths[0]) if paths else cwd
    return None


def indexed_checkout(target):
    """The top of the checkout of `target` when it has an index in the user
    cache, else None."""
    real = os.path.realpath(target)
    if not os.path.isdir(real):
        real = os.path.dirname(real)
    top = index_update.checkout_top(real)
    if not index_update.is_checkout(top) or not index_update.has_index(top):
        return None
    return top


def count(folder, session):
    """Read the count of searches of this session over the checkout, then
    write one more. None when the session id is not usable: no count, no
    message. Counters of old sessions go after 30 days."""
    if not isinstance(session, str) or not SESSION_ID.match(session):
        return None
    os.makedirs(folder, exist_ok=True)
    prune(folder)
    path = os.path.join(folder, COUNTER.format(session=session))
    try:
        with open(path, encoding="utf-8") as handle:
            seen = int(handle.read().strip() or 0)
    except (OSError, ValueError):
        seen = 0
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(f"{seen + 1}\n")
    return seen


def prune(folder):
    """Remove the counters of old sessions in folder. Errors are ignored."""
    limit = time.time() - PRUNE_AFTER_SECONDS
    prefix = COUNTER.format(session="")
    try:
        names = [n for n in os.listdir(folder) if n.startswith(prefix)]
    except OSError:
        return
    for name in names:
        path = os.path.join(folder, name)
        try:
            if os.path.getmtime(path) < limit:
                os.remove(path)
        except OSError:
            pass


def log_call(folder, payload, tool, nudged):
    """One line in tool-calls.log of the state folder (KAIROS-T-0758)."""
    os.makedirs(folder, exist_ok=True)
    log = os.path.join(folder, LOG)
    index_update.rotate(log)
    now = datetime.datetime.now().astimezone().isoformat(timespec="seconds")
    fields = [
        now,
        str(payload.get("session_id") or "-"),
        str(payload.get("agent_id") or "-"),
        str(payload.get("agent_type") or "-"),
        str(payload.get("hook_event_name") or "-"),
        tool,
        "nudge" if nudged else "quiet",
    ]
    with open(log, "a", encoding="utf-8") as handle:
        handle.write(" ".join(field.replace(" ", "_") for field in fields) + "\n")


def context(event, text):
    return {"hookSpecificOutput": {"hookEventName": event, "additionalContext": text}}


def cwd_of(payload):
    cwd = payload.get("cwd")
    return cwd if isinstance(cwd, str) and cwd else index_update.project_dir()


def tool(payload):
    tool_name = payload.get("tool_name")
    if tool_name not in ("Grep", "Glob", "Bash"):
        return
    cwd = cwd_of(payload)
    target = search_target(tool_name, payload.get("tool_input"), cwd)
    if target is None:
        return
    checkout = indexed_checkout(target)
    if checkout is None:
        return
    folder = index_update.state_dir(checkout)
    seen = count(folder, payload.get("session_id"))
    nudged = seen is not None and seen % EVERY == 0
    log_call(folder, payload, tool_name, nudged)
    if nudged:
        json.dump(
            context("PreToolUse", MESSAGE.format(name=os.path.basename(checkout))),
            sys.stdout,
        )


def session_checkouts(folder):
    """The checkouts with an index for a session folder: the checkout
    itself, or the checkouts below a board folder."""
    top = index_update.checkout_top(folder)
    if index_update.is_checkout(top):
        return [top] if index_update.has_index(top) else []
    return [c for c in index_update.board_checkouts(folder) if index_update.has_index(c)]


def subagent_text(checkouts):
    lines = [index_update.tools_text()]
    if checkouts:
        names = ", ".join(os.path.basename(c) for c in checkouts)
        lines.append(
            f"Checkouts with an index: {names}. `fetch_index` with the path of one "
            "opens it; the tools then answer for it."
        )
    else:
        lines.append(
            "No checkout of this session has a local index yet: call `fetch_index` "
            "with the path of the checkout you work on."
        )
    return "\n".join(lines)


def subagent(payload):
    checkouts = session_checkouts(cwd_of(payload))
    # The log goes to the state folder of a checkout with an index; a folder
    # with none gets no state folder.
    if checkouts:
        log_call(index_update.state_dir(checkouts[0]), payload, "-", True)
    json.dump(context("SubagentStart", subagent_text(checkouts)), sys.stdout)


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else ""
    try:
        raw = sys.stdin.read()
        payload = json.loads(raw) if raw.strip() else {}
        if not isinstance(payload, dict):
            return
        if mode == "tool":
            tool(payload)
        elif mode == "subagent":
            subagent(payload)
    except Exception as error:  # never block a tool call (KAIROS-T-0708: the reason goes to the log)
        sys.stderr.write(f"kairos index first: the hook failed: {error}\n")
        return


if __name__ == "__main__":
    main()
