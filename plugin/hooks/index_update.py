#!/usr/bin/env python3
"""The code index in each session (COLLIERY-T-1855, COLLIERY-I-0264).

Three modes, one for each hook:

- `start` (SessionStart): if `kairos` is on PATH and the checkout has the
  Kairos wiring (`.claude/kairos.local.md`), start `worker` as a detached
  process and give the agent the text about the code tools of the local
  `kairos-code` MCP server. The hook does not wait for the update, so the
  start of the session is not slower. With no `kairos` or no wiring: no
  output, no process.
- `worker` (the background job): take a lock, take the list of changed
  files, write it to the log, run `kairos index update --link-only` in the
  checkout, and clear the list only when the update succeeds. A failed update puts the
  list back for the next one. A second worker exits while one runs.
- `record` (PostToolUse on Edit, Write, MultiEdit, NotebookEdit): append the
  path (relative to the checkout) of an edited source file to the list.

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

The state is outside the checkout: `~/.claude/kairos-index/<name>-<hash>/`
(`changed-files`, `update.log`, `update.log.1`, `lock`);
KAIROS_INDEX_STATE_DIR overrides the parent folder. The hook holds no
credentials: `kairos index update` reads KAIROS_KEY or KAIROS_MCP_KEY and
`.claude/kairos.local.md` itself (COLLIERY-T-1854). Each error gives no
output and exit 0: the hook never blocks a session or a tool call. Python
stdlib only.
"""

import datetime
import fcntl
import hashlib
import json
import os
import shutil
import subprocess
import sys

SOURCE_SUFFIXES = (".rs", ".py", ".ts", ".tsx", ".js", ".jsx", ".go")
SKIPPED_DIRS = (".kairos", ".git", "target", "node_modules")
LOG_KEEP_BYTES = 256 * 1024


def update_command():
    """The command of the background update: link-only, unless
    KAIROS_INDEX_SUMMARIZE is 1."""
    if os.environ.get("KAIROS_INDEX_SUMMARIZE", "").strip() == "1":
        return ["kairos", "index", "update"]
    return ["kairos", "index", "update", "--link-only"]


def project_dir():
    return os.path.realpath(os.environ.get("CLAUDE_PROJECT_DIR") or os.getcwd())


def is_wired(project):
    return os.path.isfile(os.path.join(project, ".claude", "kairos.local.md"))


def is_checkout(project):
    """A git checkout: `.git` is a folder, or a file in a worktree."""
    return os.path.exists(os.path.join(project, ".git"))


# KAIROS-T-0348: a session that starts outside a checkout (a board folder
# with the clones below it) runs no update. The agent opens the checkout it
# works on with the tool `fetch_index` of the server.
NOT_A_CHECKOUT_TEXT = (
    "Code index: this folder is not a git checkout, so the local MCP server "
    "`kairos-code` has no index open. Call its tool `fetch_index` with `root`, "
    "the path of the checkout you work on (the clone of the repository of your "
    "task). It takes the nearest index from Kairos. Then use `module_map`, "
    "`code_search`, `symbol`, `callers`, `callees` and `path` before you read "
    "files."
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


def record(payload):
    project = project_dir()
    if not is_wired(project):
        return
    tool_input = payload.get("tool_input") or {}
    path = tool_input.get("file_path") or tool_input.get("notebook_path")
    relative = relative_source_path(project, path)
    if relative is None:
        return
    folder = state_dir(project)
    os.makedirs(folder, exist_ok=True)
    append_unique(os.path.join(folder, "changed-files"), [relative])


# ----------------------------------------------------------------- start


def session_text(project, log):
    lines = [
        "Code index of this checkout: the local MCP server `kairos-code` "
        "(`kairos index mcp`) gives the tools `module_map`, `code_search`, "
        "`symbol`, `callers`, `callees` and `path`. Use them before you read "
        "files. `module_map` gives the modules and their summaries. "
        "`code_search` finds code from a description of what it does. `symbol` "
        "gives the summary, the signature and the location of one symbol. "
        "`callers`, `callees` and `path` give the call graph. Then read only "
        "the files that the tools name.",
        "`kairos index update --link-only` runs in the background now, so the "
        "index can be some seconds old at the start of the session. It runs no "
        "model: it links the summaries that are in the pool, and a changed "
        "symbol can have no summary until Kairos or a person makes one "
        "(`kairos index update`). KAIROS_INDEX_SUMMARIZE=1 in the environment "
        f"makes the background update run the model. Its log: {log}.",
    ]
    if not os.path.isfile(os.path.join(project, ".kairos", "index.db")):
        lines.append(
            "This checkout has no local index yet. The update tries to download "
            "the base index from Kairos. If the log says that no base index is "
            "available, call `fetch_index` with this folder as `root`, or tell "
            "the user to run `kairos index --full`."
        )
    return "\n".join(lines)


def context(text):
    return {
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": text,
        }
    }


def start(payload):
    project = project_dir()
    if shutil.which("kairos") is None or not is_wired(project):
        return
    if not is_checkout(project):
        json.dump(context(NOT_A_CHECKOUT_TEXT), sys.stdout)
        return
    folder = state_dir(project)
    os.makedirs(folder, exist_ok=True)
    subprocess.Popen(
        [sys.executable, os.path.abspath(__file__), "worker", project],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        cwd=project,
        start_new_session=True,
        close_fds=True,
    )
    json.dump(
        context(session_text(project, os.path.join(folder, "update.log"))),
        sys.stdout,
    )


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
        run_update(project, folder)


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


def refused_link_only(log):
    """True when the last lines of the log are the refusal of an old kairos
    for --link-only (clap: "unexpected argument '--link-only'")."""
    try:
        with open(log, encoding="utf-8", errors="replace") as handle:
            tail = handle.read()[-2000:]
    except OSError:
        return False
    return "--link-only" in tail and "unexpected argument" in tail


def run_update(project, folder):
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
        out.write(f"Command: {' '.join(command)}\n")
        if entries:
            count = f"{len(entries)} file" + ("" if len(entries) == 1 else "s")
            out.write(f"Edited in a session since the last update: {count}\n")
            out.writelines(f"- {entry}\n" for entry in entries)
        else:
            out.write("Edited in a session since the last update: no files\n")
        out.flush()
        code = run_logged(command, project, out, log)
        if code == 2 and "--link-only" in command and refused_link_only(log):
            # A kairos from before COLLIERY-T-2529 has no --link-only. Run the
            # update that it has, so the index does not stop updating.
            command = [part for part in command if part != "--link-only"]
            out.write("This kairos has no --link-only option. Install a newer kairos. "
                      "The update runs again with no option.\n")
            out.write(f"Command: {' '.join(command)}\n")
            out.flush()
            code = run_logged(command, project, out, log)
        out.write(f"=== exit code {code}\n")
        if code == 0:
            os.remove(taken)
            out.write("The list of changed files is clear.\n")
        else:
            append_unique(changed, entries)
            os.remove(taken)
            out.write("The list of changed files is kept for the next update.\n")


# ------------------------------------------------------------------ main


def main():
    mode = sys.argv[1] if len(sys.argv) > 1 else ""
    try:
        if mode == "worker" and len(sys.argv) > 2:
            worker(sys.argv[2])
            return
        raw = sys.stdin.read()
        payload = json.loads(raw) if raw.strip() else {}
        if not isinstance(payload, dict):
            return
        if mode == "start":
            start(payload)
        elif mode == "record":
            record(payload)
    except Exception:  # never block a session or a tool call
        return


if __name__ == "__main__":
    main()
