#!/usr/bin/env python3
"""The code tools before grep (KAIROS-T-0757, KAIROS-T-0766,
COLLIERY-I-0671): a nudge on each search that the index answers, and the
index instruction for each subagent.

Two modes:

- `tool` (PreToolUse on Grep and Bash, KAIROS-T-0766): read the search
  (the pattern, the paths and the file filters of grep, rg, ag, ack or
  `git grep`; the `pattern`, `path`, `glob` and `type` of Grep). When the
  index answers it and the checkout of its path has an index in the user
  cache, give a message of two lines on each such call: the index tool
  and the name (`symbol` for a definition in one of the languages of the
  index, `callers` for a call `X(`, `references` for a name over source
  files), and that grep stays right for literal text. The hook allows the
  call. Literal text, a search over other files (logs, docs, config), a
  grep that reads a pipe, find and fd get no message.
- `subagent` (SubagentStart, each agent type): give the index instruction
  of the session start (the code tools of `kairos-code`), with the
  checkouts of the session that have an index.

Each call writes one line to `tool-calls.log` in the state folder of the
checkout: the time, the session, `agent_id`, `agent_type`, the hook event,
the tool, and `nudge` or `quiet`. The measurement (KAIROS-T-0758) reads it
to tell the main agent from the subagents. The hook input of a subagent
carries `agent_id` and `agent_type` (the hooks reference). What a Workflow
agent carries: not checked. `git grep` counts as grep; a searcher after
`xargs` too (it reads files).

Each error gives no output and exit 0: the hook never blocks a tool call
(KAIROS-T-0708). No credentials, no network. Python stdlib only.
"""

import datetime
import json
import os
import re
import shlex
import sys

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
# For grep and git grep, `-E` (extended regex) and `-d` take no value as
# the next word; for rg they do (`--encoding`, `--max-depth`).
GREP_FLAGS = ("-E", "-d")
# The options that limit a search to some files: a glob, a type or an
# extension.
FILTER_OPTIONS = ("--include", "-t", "--type", "-g", "--glob", "--iglob", "--extension")
LOG = "tool-calls.log"
# Folders that hold no source of the index: a search there is for literal
# text.
OTHER_FOLDERS = {"log", "logs", "tmp", "target", "node_modules", ".git", "dist", "build"}

# The source files of the index: the extensions of `language_of`
# (crates/kairos-index/src/extract.rs), and the rg and Grep-tool type
# names of the same languages.
SOURCE_EXTENSIONS = (
    "rs", "py", "pyi", "ts", "mts", "cts", "tsx", "js", "mjs", "cjs", "jsx",
    "go", "tf", "tfvars", "sql",
)
SOURCE_TYPES = (
    "rust", "py", "python", "ts", "typescript", "js", "javascript", "go",
    "sql", "tf", "terraform",
)

# The words that start a definition, for each language of the index.
# Rust, Python, TS/JS, Go and Terraform are case sensitive; SQL is not.
DEFINITION_WORDS = {
    # Rust
    "fn", "struct", "enum", "trait", "impl", "mod", "type", "const",
    # Python
    "def", "class",
    # TypeScript, TSX, JavaScript (also `class`, `type`, `enum`, `const`)
    "function", "interface",
    # Go (also `type`)
    "func",
    # Terraform
    "resource", "module", "variable", "data", "output",
}
# The words that can come before a definition word.
MODIFIERS = {
    "pub", "pub(crate)", "pub(super)", "pub(self)", "async", "unsafe", "extern",
    "static", "export", "default", "declare", "abstract",
}
# The words that can follow the name on a definition line.
AFTER_NAME = {"struct", "interface", "for", "where", "extends", "implements", "func", "as", "is"}
SQL_OBJECTS = {"table", "function", "view", "index", "procedure", "trigger", "type"}
SQL_FILLERS = {"or", "replace", "unique", "temp", "temporary", "materialized", "if", "not", "exists"}
IDENTIFIER = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*(?:(?:::|\.)[A-Za-z_][A-Za-z0-9_]*)*$")
# Regex parts that stand for white space, an anchor or a group: each
# reads as one space.
REGEX_SPACE = re.compile(
    r"\\s[+*?]?|\[\[:space:\]\][+*?]?|\\b|\\<|\\>| [+*?]|\^|\$|\(\?:|\)[?*]|\.\*|\\w[+*]"
)
CALL = re.compile(
    r"^(?:\\b)?\\?\.?((?:[A-Za-z_][A-Za-z0-9_]*(?:::|\\?\.))*[A-Za-z_][A-Za-z0-9_]*)\\?\($"
)

MESSAGES = {
    "symbol": (
        "Index first: `symbol` gives the definition of {term} in {name}: the file, "
        "the lines and the source."
    ),
    "callers": "Index first: `callers` gives each function in {name} that calls {term}.",
    "references": (
        "Index first: `references` gives each use of {term} in {name}: a call, "
        "a type, a field or an import."
    ),
}
LITERAL = "grep stays right for literal text: a log line, a config key, an error message."


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


def parse_search(words):
    """The parts of one search command (`grep ...`, after `search_words`):
    its name, its pattern (None for find and fd, and when it cannot be
    read), its path arguments and its file filters (`--include`, `-t`,
    `-g`, the pathspec of `git grep`). Pure."""
    name = os.path.basename(words[0])
    grep_like = name in ("grep", "egrep", "fgrep")
    args = []
    filters = []
    pattern = None
    rest = list(words[1:])
    while rest:
        word = rest.pop(0)
        if word == "--":
            args.extend(rest)
            break
        option, _, value = word.partition("=")
        if word.startswith("--") and value:
            if option in FILTER_OPTIONS:
                filters.append(value)
            elif option in ("--regexp",) and pattern is None:
                pattern = value
        elif grep_like and word in GREP_FLAGS:
            continue
        elif word in OPTIONS_WITH_VALUE:
            if rest:
                if word in ("-e", "--regexp"):
                    pattern = rest[0] if pattern is None else pattern
                elif word in PATTERN_OPTIONS:
                    pattern = ""  # a file of patterns: not read
                elif word in FILTER_OPTIONS:
                    filters.append(rest[0])
                rest = rest[1:]
        elif not word.startswith("-"):
            args.append(word)
    if name in ("find", "fd"):
        paths = args[:1] if name == "find" else args[1:2]
        return {"name": name, "pattern": None, "paths": paths, "filters": filters}
    if pattern is None and args:
        pattern = args.pop(0)
    return {"name": name, "pattern": pattern or None, "paths": args, "filters": filters}


def searches_of(command):
    """Each search over files of a shell line, as `parse_search` gives it.
    A searcher after a pipe reads the output of another command: not a
    search over files, unless `xargs` runs it. Pure."""
    found = []
    if not isinstance(command, str):
        return found
    for chain in stages_of(command):
        for position, stage in enumerate(chain):
            if position and not (stage and os.path.basename(stage[0]) == "xargs"):
                continue
            words = search_words(git_guard.words_of(" ".join(shlex.quote(w) for w in stage)))
            if words is not None:
                found.append(parse_search(words))
    return found


def search_paths(command):
    """(True, paths) when one command of the line runs a searcher over
    files, with its path arguments; (False, []) when none does. Pure."""
    searches = searches_of(command)
    paths = [p for search in searches for p in search["paths"]]
    return bool(searches), paths


def words_of_pattern(pattern):
    """The words of a regex pattern: the regex spaces, anchors and groups
    read as spaces; the quotes and escapes go. Pure."""
    text = REGEX_SPACE.sub(" ", pattern)
    text = text.replace("\\(", "(").replace("\\.", ".").replace("\\", "")
    text = text.replace('"', " ").replace("'", " ")
    # `(pub )?fn`: the group of an optional modifier opens before it.
    return [w[1:] if w.startswith("(") and w[1:] in MODIFIERS else w for w in text.split()]


def definition_term(words):
    """The name that a definition search looks for (`pub struct Index`,
    `def cli_note`, `CREATE TABLE refs`, `resource aws_s3_bucket`), else
    None. Pure."""
    if words and words[0].lower() == "create":
        tail = [w for w in words[1:] if w.lower() not in SQL_FILLERS]
        if 2 <= len(tail) <= 3 and tail[0].lower() in SQL_OBJECTS:
            term = tail[1].split("(")[0]
            return term if IDENTIFIER.match(term) else None
        return None
    while words and words[0] in MODIFIERS:
        words = words[1:]
    if len(words) < 2 or words[0] not in DEFINITION_WORDS:
        return None
    rest = words[1:]
    # `func (s *Server) Name`: the receiver of a Go method comes first.
    if words[0] == "func" and rest[0].startswith("("):
        while rest and not rest[0].endswith(")"):
            rest = rest[1:]
        rest = rest[1:]
    if not rest:
        return None
    term = re.split(r"[(<{:=\[]", rest[0])[0]
    if not IDENTIFIER.match(term):
        return None
    # Lower-case words after the name: a phrase ("type error in the file"),
    # not a definition. The words of a definition line can follow
    # (`type Foo struct`, `impl Display for Index`).
    if [w for w in rest[1:] if re.match(r"^[a-z]{2,}$", w) and w not in AFTER_NAME]:
        return None
    return term


def call_term(pattern):
    """The name that a call search looks for (`fetch_index(`,
    `Index::source\\(`), else None. Pure."""
    match = CALL.match(pattern)
    return match.group(1).replace("\\", "") if match else None


def is_source(name):
    """True when a path, a glob or a type names a source file of the index.
    Pure."""
    if name.lower() in SOURCE_TYPES:
        return True
    base = name.rsplit("/", 1)[-1]
    if "." not in base:
        return False
    extension = base.rsplit(".", 1)[-1].lower()
    return any(e.strip("{}* ") in SOURCE_EXTENSIONS for e in extension.split(","))


def names_files(name):
    """True when a path or a filter names files (an extension, a glob or a
    type), not a folder. Pure."""
    base = name.rsplit("/", 1)[-1].lstrip(".")
    return "." in base or "*" in base or name.lower() in SOURCE_TYPES


def classify(pattern, paths, filters):
    """(tool, term) when the code index answers the search, else None:
    `symbol` for a definition, `callers` for a call, `references` for a
    name over source files. A search that names only other files (logs,
    docs, config) is literal text. Pure."""
    if not pattern:
        return None
    if paths and all(set(p.strip("/").split("/")) & OTHER_FOLDERS for p in paths):
        return None
    named = list(filters) + [p for p in paths if names_files(p)]
    source = any(is_source(n) for n in named)
    if named and not source:
        return None
    term = call_term(pattern.strip())
    if term:
        return "callers", term
    words = words_of_pattern(pattern)
    term = definition_term(words)
    if term:
        return "symbol", term
    if source and len(words) == 1 and IDENTIFIER.match(words[0]) and len(words[0]) >= 3:
        return "references", words[0]
    return None


def search_of_call(tool_name, tool_input, cwd):
    """(target folder, answer) of a Grep or a Bash search, or None when the
    call is no search over files. The answer is the (tool, term) of
    `classify`, or None for a search that the index does not answer."""
    tool_input = tool_input if isinstance(tool_input, dict) else {}
    if tool_name == "Grep":
        path = tool_input.get("path")
        path = path if isinstance(path, str) and path else None
        filters = [
            v for v in (tool_input.get("glob"), tool_input.get("type")) if isinstance(v, str) and v
        ]
        pattern = tool_input.get("pattern")
        pattern = pattern if isinstance(pattern, str) else None
        answer = classify(pattern, [path] if path else [], filters)
        return (os.path.join(cwd, path) if path else cwd), answer
    if tool_name == "Bash":
        searches = searches_of(tool_input.get("command"))
        if not searches:
            return None
        chosen, answer = searches[0], None
        for search in searches:
            answer = classify(search["pattern"], search["paths"], search["filters"])
            if answer:
                chosen = search
                break
        paths = chosen["paths"]
        return (os.path.join(cwd, paths[0]) if paths else cwd), answer
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
    if tool_name not in ("Grep", "Bash"):
        return
    found = search_of_call(tool_name, payload.get("tool_input"), cwd_of(payload))
    if found is None:
        return
    target, answer = found
    checkout = indexed_checkout(target)
    if checkout is None:
        return
    log_call(index_update.state_dir(checkout), payload, tool_name, answer is not None)
    if answer is None:
        return
    index_tool, term = answer
    text = MESSAGES[index_tool].format(term=term, name=os.path.basename(checkout))
    json.dump(context("PreToolUse", text + "\n" + LITERAL), sys.stdout)


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
