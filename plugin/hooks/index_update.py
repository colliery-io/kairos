#!/usr/bin/env python3
"""The code index in each session (COLLIERY-T-1855, COLLIERY-I-0264).

Four modes:

- `start` (SessionStart): if `kairos` is on PATH and the checkout has the
  Kairos wiring (`kairos config show`), start `worker` as a detached
  process and give the agent the text about the code tools of the local
  `kairos-code` MCP server. The hook does not wait for the update, so the
  start of the session is not slower. With no `kairos` or no wiring: no
  output, no process. In a wired board folder, start `worker` in each
  clone and worktree below it (#272).
- `worker` (the background job): take a lock, take the list of changed
  files, write it to the log, run `kairos index update --link-only` in the
  checkout, and clear the list only when the update succeeds. A failed update puts the
  list back for the next one. A second worker exits while one runs.
- `record` (PostToolUse on Edit, Write, MultiEdit, NotebookEdit): find the
  checkout of the edited source file (the nearest folder above it with
  `.git`), and append the path relative to that checkout to the list of that
  checkout. Then start `debounce` for that checkout, if none waits already.
- `debounce` (the background job after edits, KAIROS-T-0524): wait until no
  edit came for N seconds, wait for an update that runs, then run the same
  update as `worker`. One for each checkout at a time (`debounce.lock`).

In a board folder (a folder with the clones below it, KAIROS-T-0348), each
edit goes to the list of its own clone or worktree, and the update runs
there. An edit outside a git checkout records nothing and starts nothing.
The checkout must have the wiring (its own, or the one of its clone), or be
below the folder of the session, which has the wiring.

N is 30 s: an agent edits files some seconds apart, so one update runs for
a burst of edits, and the index is new about a minute after the last edit.
KAIROS_INDEX_DEBOUNCE_SECONDS overrides N; `off` runs no update after an
edit.

The background update runs no model (COLLIERY-T-2529): `--link-only` links
the summaries that are in the pool (from Kairos or from an earlier update)
and makes no new ones, so a `kairos` built with `llama` does not summarize
for minutes with about 4 GB of memory at each session start. Summaries come
from the server, or from a `kairos index update` that a person runs. With
KAIROS_INDEX_SUMMARIZE=1 in the environment of the session, the background
update runs `kairos index update` with the model, as before.

The update itself finds each changed file by its content hash (a check of
about 0.1 s on Kairos), so the list does not make it faster today. The list
tells the log what the agent edited, and it is ready for an update that
parses only the listed files.

The worker writes the result of each update to `last-update.json` in the
state folder (KAIROS-T-0519): the exit code, the time, the command and the
refusal (the last `error: ` line of the output). The next `start`, and
`active_work.py compact`, read it. When the update failed, they give the
agent one line with the refusal and the command to run again. A result
older than 7 days, or a file that does not parse, gives no line.

The state is outside the checkout: `~/.claude/kairos-index/<name>-<hash>/`
(`changed-files`, `update.log`, `update.log.1`, `last-update.json`, `lock`,
`last-edit`, `debounce.lock`);
KAIROS_INDEX_STATE_DIR overrides the parent folder. The hook holds no
credentials: `kairos index update` reads KAIROS_KEY or KAIROS_MCP_KEY and
the binding of the checkout itself (COLLIERY-T-1854). Each error gives no
output and exit 0: the hook never blocks a session or a tool call. Python
stdlib only.
"""

import datetime
import fcntl
import hashlib
import json
import math
import os
import shutil
import subprocess
import sys
import time

SOURCE_SUFFIXES = (".rs", ".py", ".ts", ".tsx", ".js", ".jsx", ".go")
SKIPPED_DIRS = (".kairos", ".git", "target", "node_modules")
LOG_KEEP_BYTES = 256 * 1024
RESULT_FILE = "last-update.json"
RESULT_FORMAT = 1
# A result older than this gives no line: the CLI or the checkout can have
# changed since (KAIROS-T-0519).
RESULT_MAX_AGE_SECONDS = 7 * 24 * 3600
REFUSAL_MAX_CHARS = 600
# The debounced update after edits (KAIROS-T-0524). See the module text.
DEBOUNCE_ENV = "KAIROS_INDEX_DEBOUNCE_SECONDS"
DEBOUNCE_SECONDS = 30.0
EDIT_STAMP = "last-edit"
DEBOUNCE_LOCK = "debounce.lock"
# The most checkouts of a board folder that a session start updates, as
# session_start.MAX_CHECKOUTS lists (#272).
MAX_BOARD_CHECKOUTS = 40


def update_command():
    """The command of the background update: link-only, unless
    KAIROS_INDEX_SUMMARIZE is 1."""
    if os.environ.get("KAIROS_INDEX_SUMMARIZE", "").strip() == "1":
        return ["kairos", "index", "update"]
    return ["kairos", "index", "update", "--link-only"]


def debounce_seconds():
    """The quiet time before the update after an edit, in seconds, or None
    when KAIROS_INDEX_DEBOUNCE_SECONDS is `off`. A bad value gives 30 s."""
    raw = os.environ.get(DEBOUNCE_ENV, "").strip()
    if raw.lower() == "off":
        return None
    try:
        value = float(raw)
    except ValueError:
        return DEBOUNCE_SECONDS
    if not math.isfinite(value) or value < 0:
        return DEBOUNCE_SECONDS
    return value


def project_dir():
    return os.path.realpath(os.environ.get("CLAUDE_PROJECT_DIR") or os.getcwd())


def is_wired(project):
    """The checkout has the wiring, or its clone has (KAIROS-T-0520)."""
    # Imported here, not at the top: the detached worker imports nothing,
    # so it writes no bytecode cache while a test removes its folder.
    here = os.path.dirname(os.path.abspath(__file__))
    if here not in sys.path:
        sys.path.insert(0, here)
    import session_start

    # The CLI gives the binding (#270, KAIROS-T-0708): a [[project]] of the
    # config of the person, else the file. A CLI that fails is no wiring.
    values = session_start.wiring(project)
    return values is not None and not getattr(values, "error", None)


def is_checkout(project):
    """A git checkout: `.git` is a folder, or a file in a worktree."""
    return os.path.exists(os.path.join(project, ".git"))


# KAIROS-T-0348: a session that starts outside a checkout (a board folder
# with the clones below it) runs no update. The agent opens the checkout it
# works on with the tool `fetch_index` of the server.
NOT_A_CHECKOUT_TEXT = (
    "Code index: this folder is not a git checkout, so the local MCP server "
    "`kairos-code` has no index open. Call its tool `fetch_index` with `root`, "
    "the path of the checkout you work on (the worktree of the clone of the "
    "repository of your task; the wiring context lists the clones of this "
    "folder). It takes the nearest index from Kairos. Then use `module_map`, "
    "`code_search`, `symbol`, `callers`, `callees`, `references` and `path` "
    "before you read files."
)


def state_dir(project):
    parent = os.environ.get("KAIROS_INDEX_STATE_DIR") or os.path.join(
        os.path.expanduser("~"), ".claude", "kairos-index"
    )
    digest = hashlib.sha256(project.encode("utf-8")).hexdigest()[:12]
    name = os.path.basename(project) or "root"
    return os.path.join(parent, f"{name}-{digest}")


def read_list(path):
    try:
        with open(path, encoding="utf-8") as handle:
            return [line.strip() for line in handle if line.strip()]
    except OSError:
        return []


def append_unique(path, entries):
    """Append each entry that is not in the list yet, under a lock of the
    list file, so that 2 hooks at the same time do not lose a line."""
    with open(path, "a+", encoding="utf-8") as handle:
        fcntl.flock(handle, fcntl.LOCK_EX)
        handle.seek(0)
        known = {line.strip() for line in handle if line.strip()}
        for entry in entries:
            if entry not in known:
                handle.write(entry + "\n")
                known.add(entry)


# ---------------------------------------------------------------- record


def relative_source_path(project, path):
    """The path of an edited file relative to the checkout, or None when the
    file is not a source file of the checkout."""
    if not path:
        return None
    absolute = os.path.realpath(os.path.join(project, path))
    if not absolute.startswith(project + os.sep):
        return None
    relative = os.path.relpath(absolute, project)
    if relative.split(os.sep)[0] in SKIPPED_DIRS:
        return None
    if not relative.endswith(SOURCE_SUFFIXES):
        return None
    return relative.replace(os.sep, "/")


def checkout_of(path):
    """The checkout of a file: the nearest folder above it with `.git` (a
    folder, or a file in a worktree), as git finds its toplevel. None when
    no folder above the file has `.git`."""
    folder = os.path.dirname(path)
    while True:
        if is_checkout(folder):
            return folder
        parent = os.path.dirname(folder)
        if parent == folder:
            return None
        folder = parent


def is_below(folder, top):
    return folder == top or folder.startswith(top.rstrip(os.sep) + os.sep)


def record(payload):
    tool_input = payload.get("tool_input") or {}
    path = tool_input.get("file_path") or tool_input.get("notebook_path")
    if not path or not isinstance(path, str):
        return
    project = project_dir()
    absolute = os.path.realpath(os.path.join(project, path))
    checkout = checkout_of(absolute)
    if checkout is None:
        return
    relative = relative_source_path(checkout, absolute)
    if relative is None:
        return
    # The wiring of the checkout, or of the session folder above it (a
    # board folder).
    if not is_wired(checkout) and not (is_below(checkout, project) and is_wired(project)):
        return
    folder = state_dir(checkout)
    os.makedirs(folder, exist_ok=True)
    append_unique(os.path.join(folder, "changed-files"), [relative])
    # The time of the last edit, after the list: a worker that reads this
    # time and then takes the list has each edit up to this time.
    stamp = os.path.join(folder, EDIT_STAMP)
    with open(stamp, "a"):
        pass
    os.utime(stamp, None)
    seconds = debounce_seconds()
    if seconds is None or shutil.which("kairos") is None:
        return
    if is_held(os.path.join(folder, DEBOUNCE_LOCK)):
        return  # a worker waits; it reads the new time of the last edit
    spawn(["debounce", checkout, repr(seconds)], checkout)


def is_held(path):
    """Another process holds the lock file path."""
    with open(path, "a") as handle:
        try:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            return True
        return False


def spawn(args, cwd):
    """Start this script with args as a detached process, and do not wait."""
    subprocess.Popen(
        [sys.executable, os.path.abspath(__file__), *args],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        cwd=cwd,
        start_new_session=True,
        close_fds=True,
    )


# ----------------------------------------------------------------- start


def session_text(project, log):
    lines = [
        "Code index of this checkout: the local MCP server `kairos-code` "
        "(`kairos index mcp`) gives the tools `module_map`, `code_search`, "
        "`symbol`, `callers`, `callees`, `references` and `path`. Use them "
        "before you read files. `module_map` gives the modules and their "
        "summaries. `code_search` finds code from a description of what it "
        "does. `symbol` gives the summary, the signature and the location of "
        "one symbol. `callers`, `callees` and `path` give the call graph. "
        "`references` gives each use of a symbol: in Rust, also the uses as a "
        "type, a field or an import. Then read only the files that the tools "
        "name.",
        "`kairos index update --link-only` runs in the background now, so the "
        "index can be some seconds old at the start of the session. It runs no "
        "model: it links the summaries that are in the pool, and a changed "
        "symbol can have no summary until Kairos or a person makes one "
        "(`kairos index update`). KAIROS_INDEX_SUMMARIZE=1 in the environment "
        f"makes the background update run the model. Its log: {log}.",
        after_edit_text(),
    ]
    if not os.path.isfile(os.path.join(project, ".kairos", "index.db")):
        lines.append(
            "This checkout has no local index yet. The update tries to download "
            "the base index from Kairos. If the log says that no base index is "
            "available, call `fetch_index` with this folder as `root`, or tell "
            "the user to run `kairos index --full`."
        )
    return "\n".join(lines)


def after_edit_text():
    seconds = debounce_seconds()
    if seconds is None:
        return (
            "KAIROS_INDEX_DEBOUNCE_SECONDS=off: no update runs after an edit. Run "
            "`kairos index update --link-only` in the checkout after you change code."
        )
    return (
        f"After you edit a source file, the update runs again {seconds:g} s after "
        "your last edit, in the checkout of that file. Until then, the code tools "
        "can show the old code."
    )


# --------------------------------------------------- the previous result


def read_result(folder):
    """The result of the last update in folder, or None when there is no
    file, the file does not parse, or the result is too old."""
    try:
        with open(os.path.join(folder, RESULT_FILE), encoding="utf-8") as handle:
            result = json.load(handle)
    except (OSError, ValueError):
        return None
    if not isinstance(result, dict) or result.get("format") != RESULT_FORMAT:
        return None
    finished = result.get("finished_epoch")
    if not isinstance(finished, (int, float)) or isinstance(finished, bool):
        return None
    if time.time() - finished > RESULT_MAX_AGE_SECONDS:
        return None
    code = result.get("exit_code")
    if code is not None and (not isinstance(code, int) or isinstance(code, bool)):
        return None
    return result


def failure_line(folder):
    """One line for the agent when the last update in folder failed, else
    None. It gives the refusal of kairos and the command to run again."""
    result = read_result(folder)
    if result is None or result.get("exit_code") == 0:
        return None
    code = result.get("exit_code")
    status = "no exit code" if code is None else f"exit code {code}"
    when = result.get("finished")
    when = f" at {when}" if isinstance(when, str) and when else ""
    refusal = result.get("refusal")
    if not isinstance(refusal, str) or not refusal.strip():
        refusal = "no error text"
    refusal = clip(refusal.strip())
    command = result.get("command")
    if not isinstance(command, str) or not command.startswith("kairos "):
        command = " ".join(update_command())
    log = os.path.join(folder, "update.log")
    return (
        f"The last background update of the index failed{when}, with {status}. "
        f'The index can be old. The update gave this error: "{refusal}" '
        f"To try again, run `{command}` in this checkout. The log: {log}."
    )


def last_failure_line(project):
    """failure_line for the state folder of project (for active_work.py)."""
    return failure_line(state_dir(os.path.realpath(project)))


def clip(text):
    text = " ".join(text.split())
    if len(text) > REFUSAL_MAX_CHARS:
        text = text[: REFUSAL_MAX_CHARS - 3].rstrip() + "..."
    return text


def context(text):
    return {
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": text,
        }
    }


def board_checkouts(board):
    """The checkouts in the immediate subfolders of a board folder, sorted
    by name: each subfolder that is not hidden and has `.git` (a clone, or
    a worktree). At most MAX_BOARD_CHECKOUTS. Empty when the folder cannot
    be read (#272)."""
    try:
        names = sorted(os.listdir(board))
    except OSError:
        return []
    found = [
        os.path.join(board, name)
        for name in names
        if not name.startswith(".") and is_checkout(os.path.join(board, name))
    ]
    return found[:MAX_BOARD_CHECKOUTS]


def board_text(started):
    """The start text of a board folder: open a checkout with `fetch_index`,
    and the checkouts whose update runs now, each with its log and the
    failure of its last update."""
    lines = [NOT_A_CHECKOUT_TEXT]
    if started:
        names = ", ".join(os.path.basename(checkout) for checkout, _ in started)
        lines.append(
            "`kairos index update --link-only` runs in the background now in each "
            f"checkout of this folder: {names}. So `fetch_index` opens an index "
            "that is some seconds old or newer. Each log is in the state folder "
            "of its checkout, under ~/.claude/kairos-index/."
        )
        for checkout, failure in started:
            if failure:
                lines.append(f"{os.path.basename(checkout)}: {failure}")
    lines.append(after_edit_text())
    return "\n".join(lines)


def start(payload):
    project = project_dir()
    if shutil.which("kairos") is None or not is_wired(project):
        return
    if not is_checkout(project):
        # A board folder (#272): the update of each clone and worktree below
        # it, with the wiring of the board folder, as after an edit
        # (KAIROS-T-0524).
        started = []
        for checkout in board_checkouts(project):
            folder = state_dir(checkout)
            os.makedirs(folder, exist_ok=True)
            started.append((checkout, failure_line(folder)))
            spawn(["worker", checkout], checkout)
        json.dump(context(board_text(started)), sys.stdout)
        return
    folder = state_dir(project)
    os.makedirs(folder, exist_ok=True)
    # Read the result of the previous update before the new one starts.
    failure = failure_line(folder)
    spawn(["worker", project], project)
    text = session_text(project, os.path.join(folder, "update.log"))
    if failure:
        text += "\n" + failure
    json.dump(context(text), sys.stdout)


# ---------------------------------------------------------------- worker


def rotate(log):
    try:
        if os.path.getsize(log) > LOG_KEEP_BYTES:
            os.replace(log, log + ".1")
    except OSError:
        pass


def worker(project):
    project = os.path.realpath(project)
    folder = state_dir(project)
    os.makedirs(folder, exist_ok=True)
    with open(os.path.join(folder, "lock"), "w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            return  # an update runs already; it or the next one reads the list
        run_update(project, folder, "the start of a session")


def last_edit(stamp):
    """The time of the last recorded edit, or None."""
    try:
        return os.stat(stamp).st_mtime
    except OSError:
        return None


def has_list(folder):
    """The list of changed files, or a list that a stopped worker left, has
    an entry."""
    changed = os.path.join(folder, "changed-files")
    return bool(read_list(changed) or read_list(changed + ".taken"))


def debounce(project, seconds):
    """Run the update when no edit came for `seconds` (KAIROS-T-0524). One
    for each checkout at a time: a second one exits at once."""
    project = os.path.realpath(project)
    folder = state_dir(project)
    stamp = os.path.join(folder, EDIT_STAMP)
    while True:
        with open(os.path.join(folder, DEBOUNCE_LOCK), "a") as lock:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except OSError:
                return  # a worker waits already
            handled = wait_and_update(project, folder, stamp, seconds)
        # An edit after the last check, while this worker held the lock,
        # started no worker: run again for it.
        edited = last_edit(stamp)
        if handled is None or edited is None or edited <= handled:
            return


def wait_and_update(project, folder, stamp, seconds):
    """Wait for the quiet time and run the update until no edit is new. The
    time of the last edit that an update has, or None."""
    handled = None
    while True:
        edited = last_edit(stamp)
        if edited is None or (handled is not None and edited <= handled):
            return handled
        wait = edited + seconds - time.time()
        if wait > 0:
            time.sleep(wait)
            continue
        with open(os.path.join(folder, "lock"), "a") as lock:
            # Wait for an update that runs (the one of the session start).
            fcntl.flock(lock, fcntl.LOCK_EX)
            edited = last_edit(stamp)
            if edited is not None and edited + seconds > time.time():
                continue  # an edit came during the wait
            handled = edited
            if has_list(folder):
                run_update(
                    project, folder, f"edits, {seconds:g} s after the last one"
                )
            # Else an update took the list after the edits: it has them.


def run_logged(command, project, out, log):
    """Run command in project, with its output in the log. The exit code, or
    None when kairos does not start."""
    out.flush()
    try:
        code = subprocess.run(
            command,
            cwd=project,
            stdin=subprocess.DEVNULL,
            stdout=out,
            stderr=subprocess.STDOUT,
            check=False,
        ).returncode
    except OSError as error:
        out.write(f"Could not start kairos: {error}\n")
        code = None
    out.flush()
    return code


# The exit code of kairos for an argument that it does not know
# (KAIROS-T-0588): EX_USAGE. An auth error is 2, a failure 1.
EXIT_USAGE = 64


def run_update(project, folder, reason):
    changed = os.path.join(folder, "changed-files")
    taken = changed + ".taken"
    # Take the list first. A list left by a stopped worker is taken again.
    entries = read_list(taken)
    if os.path.exists(changed):
        os.replace(changed, changed + ".new")
        entries += [e for e in read_list(changed + ".new") if e not in entries]
        os.remove(changed + ".new")
    with open(taken, "w", encoding="utf-8") as handle:
        handle.writelines(entry + "\n" for entry in entries)

    log = os.path.join(folder, "update.log")
    rotate(log)
    with open(log, "a", encoding="utf-8") as out:
        now = datetime.datetime.now().astimezone().isoformat(timespec="seconds")
        command = update_command()
        out.write(f"\n=== {now} kairos index update in {project}\n")
        out.write(f"Started by: {reason}\n")
        out.write(f"Command: {' '.join(command)}\n")
        if entries:
            count = f"{len(entries)} file" + ("" if len(entries) == 1 else "s")
            out.write(f"Edited in a session since the last update: {count}\n")
            out.writelines(f"- {entry}\n" for entry in entries)
        else:
            out.write("Edited in a session since the last update: no files\n")
        out.flush()
        start = out.tell()
        code = run_logged(command, project, out, log)
        if code == EXIT_USAGE and "--link-only" in command:
            # A kairos that does not know --link-only refuses it with the
            # usage code (KAIROS-T-0588). Run the update that it has, so the
            # index does not stop updating.
            command = [part for part in command if part != "--link-only"]
            out.write("This kairos has no --link-only option. Run `kairos self-update`. "
                      "The update runs again with no option.\n")
            out.write(f"Command: {' '.join(command)}\n")
            out.flush()
            start = out.tell()
            code = run_logged(command, project, out, log)
        refusal = None if code == 0 else refusal_text(log, start)
        out.write(f"=== exit code {code}\n")
        if code == 0:
            os.remove(taken)
            out.write("The list of changed files is clear.\n")
        else:
            append_unique(changed, entries)
            os.remove(taken)
            out.write("The list of changed files is kept for the next update.\n")
    write_result(folder, code, command, refusal)


def refusal_text(log, start):
    """The refusal in the output of one run of kairos: the log from start.
    The last `error: ` line, else the last line that is not empty."""
    try:
        with open(log, "rb") as handle:
            handle.seek(start)
            output = handle.read().decode("utf-8", errors="replace")
    except OSError:
        return None
    lines = [line.strip() for line in output.splitlines() if line.strip()]
    for line in reversed(lines):
        if line.startswith("error: "):
            return line[len("error: "):].strip()
    return lines[-1] if lines else None


def write_result(folder, code, command, refusal):
    """Write last-update.json in folder: a temporary file, then a rename,
    so that a hook never reads half a file."""
    now = datetime.datetime.now().astimezone()
    result = {
        "format": RESULT_FORMAT,
        "exit_code": code,
        "finished": now.isoformat(timespec="seconds"),
        "finished_epoch": now.timestamp(),
        "command": " ".join(command),
        "refusal": refusal,
    }
    path = os.path.join(folder, RESULT_FILE)
    temporary = path + ".tmp"
    with open(temporary, "w", encoding="utf-8") as handle:
        json.dump(result, handle)
        handle.write("\n")
    os.replace(temporary, path)


# ------------------------------------------------------------------ main


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else ""
    try:
        if mode == "worker" and len(sys.argv) > 2:
            worker(sys.argv[2])
            return
        if mode == "debounce" and len(sys.argv) > 3:
            debounce(sys.argv[2], float(sys.argv[3]))
            return
        raw = sys.stdin.read()
        payload = json.loads(raw) if raw.strip() else {}
        if not isinstance(payload, dict):
            return
        if mode == "start":
            start(payload)
        elif mode == "record":
            record(payload)
    except Exception as error:  # never block a session or a tool call (KAIROS-T-0708: the reason goes to the log)
        sys.stderr.write(f"kairos index update: the hook failed: {error}\n")
        return


if __name__ == "__main__":
    main()
