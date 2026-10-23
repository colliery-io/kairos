#!/usr/bin/env python3
"""Unit tests for the code index in each session (COLLIERY-T-1855): the
`start` side (the SessionStart hook that starts `kairos index update` in the
background and gives the text about the code tools), the `worker` side (the
background job), the `record` side (the PostToolUse hook on the edit and
write tools), and the wiring in the manifest, `hooks.json` and the
`.claude/settings.json` of this repository. Stdlib only; run with
`python3 -m unittest plugin/hooks/test_index_update.py` (or
`angreal test unit`, which includes it).

The tests run `index_update.py` as a subprocess, the way Claude Code runs it,
with a fake `kairos` on PATH and KAIROS_INDEX_STATE_DIR and
CLAUDE_PROJECT_DIR pointing at a temporary directory. The parts that need a
live session (the start time of a real session, the tools of the MCP server)
are the manual check on the task."""

import fcntl
import hashlib
import json
import select
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "index_update.py")
ACTIVE_WORK = os.path.join(HERE, "active_work.py")
ROOT = os.path.dirname(os.path.dirname(HERE))
WRAPPER = os.path.join(ROOT, "plugin", "bin", "kairos-code")

WIRING = "---\ndeployment_url: http://127.0.0.1:1\nrepository: kairos\n---\n"

# A fake `kairos`: it writes its arguments, its folder and the list of
# changed files (as the worker left it) to calls.txt, then sleeps, writes
# FAKE_KAIROS_ERROR as an `error: ` line to stderr (as the CLI does), and
# exits with the code in FAKE_KAIROS_EXIT.
FAKE_KAIROS = """#!/bin/sh
{
  echo "args: $*"
  echo "cwd: $(pwd)"
} >> "$FAKE_KAIROS_CALLS"
if [ -n "$FAKE_KAIROS_OLD" ]; then
  case " $* " in *" --link-only "*)
    echo "error: unexpected argument '--link-only' found" >&2
    exit 64;;
  esac
fi
if [ -n "$FAKE_KAIROS_EDIT" ]; then
  echo "$FAKE_KAIROS_EDIT" >> "$FAKE_KAIROS_LIST"
fi
sleep "${FAKE_KAIROS_SLEEP:-0}"
echo "Structure: 3 files (fake)"
if [ -n "$FAKE_KAIROS_ERROR" ]; then
  echo "error: $FAKE_KAIROS_ERROR" >&2
fi
exit "${FAKE_KAIROS_EXIT:-0}"
"""


class Harness(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        base = os.path.realpath(self.tmp.name)
        self.project = os.path.join(base, "checkout")
        self.state = os.path.join(base, "state")
        self.bin = os.path.join(base, "bin")
        os.makedirs(os.path.join(self.project, ".claude"))
        os.makedirs(os.path.join(self.project, ".git"))
        os.makedirs(self.bin)
        self.calls = os.path.join(base, "calls.txt")
        self.wire()
        self.fake_kairos()

    def tearDown(self):
        self.tmp.cleanup()

    def wire(self):
        with open(
            os.path.join(self.project, ".claude", "kairos.local.md"), "w"
        ) as handle:
            handle.write(WIRING)

    def fake_kairos(self):
        path = os.path.join(self.bin, "kairos")
        with open(path, "w") as handle:
            handle.write(FAKE_KAIROS)
        os.chmod(path, 0o755)

    def env(self, **extra):
        env = {
            "PATH": self.bin + os.pathsep + "/usr/bin:/bin",
            "HOME": self.tmp.name,
            "CLAUDE_PROJECT_DIR": self.project,
            "KAIROS_INDEX_STATE_DIR": self.state,
            "FAKE_KAIROS_CALLS": self.calls,
            # HOME is the temporary folder: with no bytecode cache, the
            # detached worker writes nothing there while tearDown removes it.
            "PYTHONDONTWRITEBYTECODE": "1",
            # No debounced update after an edit (KAIROS-T-0524), so a test
            # leaves no detached worker. DebouncedUpdate sets a number.
            "KAIROS_INDEX_DEBOUNCE_SECONDS": "off",
        }
        env.update(extra)
        return env

    def run_hook(self, mode, payload=None, env=None, args=()):
        started = time.monotonic()
        result = subprocess.run(
            [sys.executable, SCRIPT, mode, *args],
            input=json.dumps(payload or {}),
            capture_output=True,
            text=True,
            env=env or self.env(),
            timeout=30,
        )
        result.elapsed = time.monotonic() - started
        return result

    def state_dir(self):
        """The one folder that the script made under KAIROS_INDEX_STATE_DIR."""
        names = os.listdir(self.state) if os.path.isdir(self.state) else []
        self.assertEqual(len(names), 1, names)
        return os.path.join(self.state, names[0])

    def list_path(self):
        return os.path.join(self.state_dir(), "changed-files")

    def changed(self):
        path = self.list_path()
        if not os.path.exists(path):
            return []
        with open(path) as handle:
            return [line for line in handle.read().splitlines() if line]

    def calls_text(self):
        if not os.path.exists(self.calls):
            return ""
        with open(self.calls) as handle:
            return handle.read()

    def wait_for(self, predicate, seconds=10):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if predicate():
                return True
            time.sleep(0.05)
        return predicate()

    def edit(self, tool, path, key="file_path"):
        return self.run_hook(
            "record",
            {
                "session_id": "s1",
                "hook_event_name": "PostToolUse",
                "tool_name": tool,
                "tool_input": {key: path},
            },
        )


class SessionStart(Harness):
    """Scenario: A session starts with a current index."""

    def test_the_start_does_not_wait_for_the_update(self):
        result = self.run_hook(
            "start",
            {"session_id": "s1", "source": "startup"},
            env=self.env(FAKE_KAIROS_SLEEP="3"),
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertLess(result.elapsed, 1.0)
        # Wait for the cwd line: the fake writes its args line first, and a
        # read between the 2 lines saw only the args (a race under load).
        self.assertTrue(
            self.wait_for(lambda: "cwd: " in self.calls_text()),
            "the background worker did not run `kairos index update`",
        )
        self.assertIn("args: index update", self.calls_text())
        self.assertIn(f"cwd: {self.project}", self.calls_text())
        # The update runs in the background; its log is outside the checkout.
        log = os.path.join(self.state_dir(), "update.log")
        def finished():
            with open(log) as handle:
                return "exit code 0" in handle.read()

        self.assertTrue(self.wait_for(finished, 15))
        self.assertFalse(log.startswith(self.project))

    def test_the_text_names_the_code_tools_before_files(self):
        result = self.run_hook("start", {"session_id": "s1", "source": "startup"})
        context = json.loads(result.stdout)["hookSpecificOutput"]
        self.assertEqual(context["hookEventName"], "SessionStart")
        text = context["additionalContext"]
        for tool in ("module_map", "code_search", "callers", "callees", "references"):
            self.assertIn(f"`{tool}`", text)
        self.assertIn("before you read files", text)
        self.assertIn("kairos-code", text)
        self.assertIn("update.log", text)

    def test_the_text_says_that_the_background_update_runs_no_model(self):
        result = self.run_hook("start", {"session_id": "s1", "source": "startup"})
        text = json.loads(result.stdout)["hookSpecificOutput"]["additionalContext"]
        self.assertIn("kairos index update --link-only", text)
        self.assertIn("KAIROS_INDEX_SUMMARIZE=1", text)

    def test_the_text_says_when_the_update_runs_after_an_edit(self):
        # KAIROS-T-0524: the update runs again after the edits.
        result = self.run_hook(
            "start",
            {"session_id": "s1", "source": "startup"},
            env=self.env(KAIROS_INDEX_DEBOUNCE_SECONDS="45"),
        )
        text = json.loads(result.stdout)["hookSpecificOutput"]["additionalContext"]
        self.assertIn("runs again 45 s after your last edit", text)
        result = self.run_hook("start", {"session_id": "s1", "source": "startup"})
        text = json.loads(result.stdout)["hookSpecificOutput"]["additionalContext"]
        self.assertIn("KAIROS_INDEX_DEBOUNCE_SECONDS=off", text)
        self.assertNotIn("runs again", text)

    def test_no_kairos_on_path_is_a_silent_no_op(self):
        os.remove(os.path.join(self.bin, "kairos"))
        result = self.run_hook("start", {"session_id": "s1", "source": "startup"})
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr, "")
        self.assertFalse(os.path.exists(self.state))

    def test_a_folder_that_is_not_a_checkout_runs_no_update_and_names_fetch_index(self):
        # KAIROS-T-0348: a session of a board folder, with the clones below it.
        os.rmdir(os.path.join(self.project, ".git"))
        result = self.run_hook("start", {"session_id": "s1", "source": "startup"})
        self.assertEqual(result.returncode, 0)
        text = json.loads(result.stdout)["hookSpecificOutput"]["additionalContext"]
        self.assertIn("not a git checkout", text)
        self.assertIn("`fetch_index`", text)
        self.assertIn("`root`", text)
        self.assertIn("lists the clones of this folder", text)  # KAIROS-T-0521
        self.assertNotIn("kairos index --full", text)
        time.sleep(0.3)
        self.assertEqual(self.calls_text(), "", "no update ran")

    def test_a_worktree_of_a_wired_clone_runs_the_update(self):
        # KAIROS-T-0520: the wiring is git-excluded, so the worktree has
        # none; the hook reads the file of its clone.
        base = os.path.dirname(self.project)
        clone = os.path.join(base, "clone")
        worktree = os.path.join(base, "wt-acme-t-0001")
        git_env = dict(os.environ, GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1")
        def git(*args):
            subprocess.run(
                ["git", "-c", "user.name=Robin", "-c", "user.email=robin@example.com", *args],
                cwd=clone, check=True, capture_output=True, env=git_env,
            )
        os.makedirs(os.path.join(clone, ".claude"))
        git("init", "-q", "-b", "main")
        git("commit", "-q", "--allow-empty", "-m", "first")
        with open(os.path.join(clone, ".claude", "kairos.local.md"), "w") as handle:
            handle.write(WIRING)
        git("worktree", "add", "-q", "-b", "acme-t-0001", worktree)
        self.assertFalse(os.path.exists(os.path.join(worktree, ".claude")))
        result = self.run_hook(
            "start",
            {"session_id": "s1", "source": "startup"},
            env=self.env(CLAUDE_PROJECT_DIR=worktree),
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        text = json.loads(result.stdout)["hookSpecificOutput"]["additionalContext"]
        self.assertIn("kairos-code", text)
        self.assertTrue(
            self.wait_for(lambda: f"cwd: {worktree}" in self.calls_text()),
            "the background worker did not run in the worktree",
        )
        def finished():
            for name in os.listdir(self.state):
                log = os.path.join(self.state, name, "update.log")
                if os.path.exists(log) and "exit code" in open(log).read():
                    return True
            return False

        self.assertTrue(self.wait_for(finished, 15))

    def test_a_checkout_with_no_wiring_is_a_silent_no_op(self):
        os.remove(os.path.join(self.project, ".claude", "kairos.local.md"))
        result = self.run_hook("start", {"session_id": "s1", "source": "startup"})
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        time.sleep(0.3)
        self.assertEqual(self.calls_text(), "")


class ChangedFiles(Harness):
    """Scenario: An edited file is in the list of changed files."""

    def test_an_edited_rust_file_is_in_the_list(self):
        path = os.path.join(self.project, "crates", "a", "src", "lib.rs")
        result = self.edit("Edit", path)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertEqual(self.changed(), ["crates/a/src/lib.rs"])

    def test_each_edit_and_write_tool_records_once(self):
        self.edit("Write", os.path.join(self.project, "a.py"))
        self.edit("MultiEdit", os.path.join(self.project, "b.ts"))
        self.edit("Edit", os.path.join(self.project, "a.py"))
        self.edit("NotebookEdit", os.path.join(self.project, "c.go"), "notebook_path")
        self.assertEqual(self.changed(), ["a.py", "b.ts", "c.go"])

    def test_other_files_are_not_in_the_list(self):
        self.edit("Edit", os.path.join(self.project, "README.md"))
        self.edit("Edit", "/somewhere/else/lib.rs")
        self.edit("Edit", os.path.join(self.project, ".kairos", "x.rs"))
        self.edit("Edit", os.path.join(self.project, ".git", "x.rs"))
        self.edit("Edit", "")
        self.assertFalse(os.path.exists(self.state) and self.changed())

    def test_no_wiring_records_nothing(self):
        os.remove(os.path.join(self.project, ".claude", "kairos.local.md"))
        self.edit("Edit", os.path.join(self.project, "lib.rs"))
        self.assertFalse(os.path.exists(self.state))

    def test_bad_input_never_fails_the_tool(self):
        result = subprocess.run(
            [sys.executable, SCRIPT, "record"],
            input="not json",
            capture_output=True,
            text=True,
            env=self.env(),
        )
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")

    def test_the_next_update_reads_the_list_first_and_clears_it(self):
        self.edit("Edit", os.path.join(self.project, "src", "lib.rs"))
        result = self.run_hook("worker", args=(self.project,))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("args: index update", self.calls_text())
        self.assertEqual(self.changed(), [])
        with open(os.path.join(self.state_dir(), "update.log")) as handle:
            log = handle.read()
        self.assertIn("src/lib.rs", log)
        self.assertLess(log.index("src/lib.rs"), log.index("Structure: 3 files"))
        self.assertIn("exit code 0", log)

    def test_the_background_update_runs_no_model_by_default(self):
        result = self.run_hook("worker", args=(self.project,))
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls_text().splitlines()
        self.assertIn("args: index update --link-only", calls)
        with open(os.path.join(self.state_dir(), "update.log")) as handle:
            log = handle.read()
        self.assertIn("Command: kairos index update --link-only", log)

    def test_kairos_index_summarize_runs_the_model_in_the_background(self):
        result = self.run_hook(
            "worker", env=self.env(KAIROS_INDEX_SUMMARIZE="1"), args=(self.project,)
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("args: index update", self.calls_text().splitlines())
        self.assertNotIn("--link-only", self.calls_text())
        with open(os.path.join(self.state_dir(), "update.log")) as handle:
            self.assertIn("Command: kairos index update\n", handle.read())

    def test_other_values_of_kairos_index_summarize_run_no_model(self):
        for value in ("", "0", "no"):
            self.run_hook(
                "worker", env=self.env(KAIROS_INDEX_SUMMARIZE=value), args=(self.project,)
            )
        calls = [c for c in self.calls_text().splitlines() if c.startswith("args:")]
        self.assertEqual(calls, ["args: index update --link-only"] * 3)

    def test_a_kairos_with_no_link_only_runs_the_plain_update(self):
        # A kairos that does not know --link-only refuses it with the usage
        # code 64 (KAIROS-T-0588). The hook reads the code, not the log.
        self.edit("Edit", os.path.join(self.project, "src", "lib.rs"))
        self.run_hook("worker", env=self.env(FAKE_KAIROS_OLD="1"), args=(self.project,))
        calls = [c for c in self.calls_text().splitlines() if c.startswith("args:")]
        self.assertEqual(calls, ["args: index update --link-only", "args: index update"])
        self.assertEqual(self.changed(), [])
        with open(os.path.join(self.state_dir(), "update.log")) as handle:
            log = handle.read()
        self.assertIn("This kairos has no --link-only option", log)
        self.assertIn("kairos self-update", log)
        self.assertIn("=== exit code 0", log)

    def test_an_auth_error_is_not_retried(self):
        # Exit 2 is an auth error (KAIROS-A-0015), also with clap's text in
        # the log: the hook no longer reads the log for it.
        self.edit("Edit", os.path.join(self.project, "src", "lib.rs"))
        self.run_hook("worker", env=self.env(FAKE_KAIROS_EXIT="2"), args=(self.project,))
        calls = [c for c in self.calls_text().splitlines() if c.startswith("args:")]
        self.assertEqual(calls, ["args: index update --link-only"])
        self.assertEqual(self.changed(), ["src/lib.rs"])

    def test_a_usage_error_without_link_only_is_not_retried(self):
        self.edit("Edit", os.path.join(self.project, "src", "lib.rs"))
        self.run_hook(
            "worker",
            env=self.env(FAKE_KAIROS_EXIT="64", KAIROS_INDEX_SUMMARIZE="1"),
            args=(self.project,),
        )
        calls = [c for c in self.calls_text().splitlines() if c.startswith("args:")]
        self.assertEqual(calls, ["args: index update"])
        self.assertEqual(self.changed(), ["src/lib.rs"])

    def test_a_failed_update_keeps_the_list(self):
        self.edit("Edit", os.path.join(self.project, "src", "lib.rs"))
        self.run_hook(
            "worker", env=self.env(FAKE_KAIROS_EXIT="1"), args=(self.project,)
        )
        self.assertEqual(self.changed(), ["src/lib.rs"])

    def test_an_edit_during_the_update_stays_for_the_next_one(self):
        self.edit("Edit", os.path.join(self.project, "src", "lib.rs"))
        self.run_hook(
            "worker",
            env=self.env(FAKE_KAIROS_EDIT="src/new.rs", FAKE_KAIROS_LIST=self.list_path()),
            args=(self.project,),
        )
        self.assertEqual(self.changed(), ["src/new.rs"])

    def test_a_second_worker_does_not_run_while_one_runs(self):
        self.edit("Edit", os.path.join(self.project, "src", "lib.rs"))
        with open(os.path.join(self.state_dir(), "lock"), "w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.run_hook("worker", args=(self.project,))
        self.assertEqual(self.calls_text(), "")
        self.assertEqual(self.changed(), ["src/lib.rs"])


class DebouncedUpdate(Harness):
    """Scenario: The index is new again after edits during a session
    (KAIROS-T-0524). An edit starts a detached worker for the checkout of
    the file. It runs the update when no edit came for N seconds."""

    QUIET = "0.3"

    def setUp(self):
        super().setUp()
        base = os.path.dirname(self.project)
        # A board folder: wired, not a checkout, with 2 clones below it.
        # The clones have no wiring of their own.
        self.board = os.path.join(base, "board")
        self.one = self.checkout(os.path.join(self.board, "one"))
        self.two = self.checkout(os.path.join(self.board, "two"))
        os.makedirs(os.path.join(self.board, ".claude"))
        with open(os.path.join(self.board, ".claude", "kairos.local.md"), "w") as handle:
            handle.write(WIRING)

    def tearDown(self):
        # No detached worker may outlive the test.
        for name in os.listdir(self.state) if os.path.isdir(self.state) else []:
            for lock in ("debounce.lock", "lock"):
                path = os.path.join(self.state, name, lock)
                if os.path.exists(path):
                    self.assertTrue(self.wait_for(lambda: not self.held(path), 15), path)
        super().tearDown()

    def checkout(self, path, git_file=False):
        os.makedirs(path)
        if git_file:  # a worktree: `.git` is a file
            with open(os.path.join(path, ".git"), "w") as handle:
                handle.write("gitdir: /nowhere\n")
        else:
            os.makedirs(os.path.join(path, ".git"))
        return path

    def folder_of(self, checkout):
        digest = hashlib.sha256(checkout.encode("utf-8")).hexdigest()[:12]
        return os.path.join(self.state, f"{os.path.basename(checkout)}-{digest}")

    def list_of(self, checkout):
        path = os.path.join(self.folder_of(checkout), "changed-files")
        if not os.path.exists(path):
            return []
        with open(path) as handle:
            return [line for line in handle.read().splitlines() if line]

    def log_of(self, checkout):
        path = os.path.join(self.folder_of(checkout), "update.log")
        if not os.path.exists(path):
            return ""
        with open(path) as handle:
            return handle.read()

    def held(self, path):
        with open(path, "a") as handle:
            try:
                fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except OSError:
                return True
            return False

    def runs(self, checkout=None):
        """The number of runs of the fake kairos (in checkout)."""
        lines = self.calls_text().splitlines()
        if checkout is None:
            return sum(1 for line in lines if line.startswith("cwd: "))
        return lines.count(f"cwd: {checkout}")

    def finished(self, checkout, count=1):
        return self.log_of(checkout).count("=== exit code") >= count

    def edit_in(self, path, session=None, quiet=None, **extra):
        return self.run_hook(
            "record",
            {"session_id": "s1", "tool_name": "Edit", "tool_input": {"file_path": path}},
            env=self.env(
                CLAUDE_PROJECT_DIR=session or self.board,
                KAIROS_INDEX_DEBOUNCE_SECONDS=quiet or self.QUIET,
                **extra,
            ),
        )

    def test_an_edit_in_a_board_folder_is_recorded_in_its_checkout(self):
        # index_update.py:139-149 before KAIROS-T-0524: the list was kept
        # under the board folder, with `one/` in each path, and no update ran.
        result = self.edit_in(os.path.join(self.one, "src", "lib.rs"), quiet="off")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "")
        self.assertEqual(self.list_of(self.one), ["src/lib.rs"])
        self.assertFalse(os.path.exists(self.folder_of(self.board)))
        time.sleep(0.5)
        self.assertEqual(self.calls_text(), "", "off: no update")

    def test_an_edit_in_a_board_folder_updates_its_checkout(self):
        self.edit_in(os.path.join(self.one, "src", "lib.rs"))
        self.assertTrue(self.wait_for(lambda: self.finished(self.one)))
        self.assertEqual(self.runs(), 1)
        self.assertIn(f"cwd: {self.one}", self.calls_text())
        self.assertIn("args: index update --link-only", self.calls_text())
        log = self.log_of(self.one)
        self.assertIn("- src/lib.rs", log)
        self.assertIn("=== exit code 0", log)
        self.assertEqual(self.list_of(self.one), [])
        # The update writes its result for the next start (KAIROS-T-0519).
        with open(os.path.join(self.folder_of(self.one), "last-update.json")) as handle:
            self.assertEqual(json.load(handle)["exit_code"], 0)

    def test_two_checkouts_have_a_list_and_an_update_each(self):
        self.edit_in(os.path.join(self.one, "src", "a.rs"), quiet="off")
        self.edit_in(os.path.join(self.two, "b.py"), quiet="off")
        self.assertEqual(self.list_of(self.one), ["src/a.rs"])
        self.assertEqual(self.list_of(self.two), ["b.py"])
        self.edit_in(os.path.join(self.one, "src", "c.rs"))
        self.edit_in(os.path.join(self.two, "d.py"))
        self.assertTrue(self.wait_for(lambda: self.finished(self.one) and self.finished(self.two)))
        self.assertEqual(self.runs(self.one), 1)
        self.assertEqual(self.runs(self.two), 1)
        self.assertIn("- src/a.rs\n- src/c.rs", self.log_of(self.one))
        self.assertIn("- b.py\n- d.py", self.log_of(self.two))
        self.assertNotIn("b.py", self.log_of(self.one))

    def start_in_board(self):
        result = self.run_hook(
            "start",
            {"session_id": "s1", "source": "startup"},
            env=self.env(CLAUDE_PROJECT_DIR=self.board),
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)["hookSpecificOutput"]["additionalContext"]

    def test_the_start_in_a_board_folder_updates_each_checkout(self):
        # #272: before, a session that started in a board folder updated no
        # index; the agent used grep instead of the code tools.
        hidden = self.checkout(os.path.join(self.board, ".hidden"))
        os.makedirs(os.path.join(self.board, "scripts"))  # no repository
        text = self.start_in_board()
        self.assertIn("`fetch_index`", text)
        self.assertIn("each checkout of this folder: one, two.", text)
        self.assertTrue(self.wait_for(lambda: self.finished(self.one) and self.finished(self.two)))
        self.assertEqual(self.runs(self.one), 1)
        self.assertEqual(self.runs(self.two), 1)
        self.assertEqual(self.runs(hidden), 0)
        self.assertEqual(self.runs(), 2, "no update in the board folder or scripts/")
        self.assertIn("Started by: the start of a session", self.log_of(self.one))

    def test_the_start_in_a_board_folder_gives_the_last_failure_of_a_checkout(self):
        folder = self.folder_of(self.two)
        os.makedirs(folder)
        with open(os.path.join(folder, "last-update.json"), "w") as handle:
            json.dump(
                {
                    "format": 1,
                    "exit_code": 3,
                    "finished": "2026-10-23T09:00:00+00:00",
                    "finished_epoch": time.time(),
                    "command": "kairos index update --link-only",
                    "refusal": "error: the deployment refused the key",
                },
                handle,
            )
        text = self.start_in_board()
        self.assertIn("two: The last background update of the index failed", text)
        self.assertIn("the deployment refused the key", text)
        self.assertNotIn("one: The last", text)
        self.assertTrue(self.wait_for(lambda: self.finished(self.one) and self.finished(self.two)))

    def test_the_start_in_a_board_folder_with_no_checkout_runs_no_update(self):
        for checkout in (self.one, self.two):
            shutil.rmtree(checkout)
        text = self.start_in_board()
        self.assertIn("not a git checkout", text)
        self.assertNotIn("runs in the background now", text)
        time.sleep(0.3)
        self.assertEqual(self.calls_text(), "")

    def test_a_worktree_inside_a_clone_is_its_own_checkout(self):
        worktree = self.checkout(
            os.path.join(self.one, ".claude", "worktrees", "w1"), git_file=True
        )
        self.edit_in(os.path.join(worktree, "src", "lib.rs"), quiet="off")
        self.assertEqual(self.list_of(worktree), ["src/lib.rs"])
        self.assertEqual(self.list_of(self.one), [])

    def test_a_second_edit_inside_the_quiet_time_starts_no_second_update(self):
        quiet = 1.5
        self.edit_in(os.path.join(self.one, "a.rs"), quiet=str(quiet))
        time.sleep(0.5)
        last = time.monotonic()  # the second edit is at this time or later
        self.edit_in(os.path.join(self.one, "b.rs"), quiet=str(quiet))
        # Not sooner than the quiet time after the last edit.
        time.sleep(1.0)
        self.assertEqual(self.calls_text(), "", "the update ran before the quiet time")
        self.assertTrue(self.wait_for(lambda: self.finished(self.one)))
        self.assertGreaterEqual(time.monotonic() - last, quiet)
        time.sleep(quiet + 0.5)
        self.assertEqual(self.runs(), 1, self.calls_text())
        self.assertIn("- a.rs\n- b.rs", self.log_of(self.one))
        self.assertEqual(self.list_of(self.one), [])

    def test_a_waiting_worker_starts_no_second_worker(self):
        folder = self.folder_of(self.one)
        os.makedirs(folder)
        with open(os.path.join(folder, "debounce.lock"), "w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.edit_in(os.path.join(self.one, "a.rs"))
            time.sleep(1.2)
        self.assertEqual(self.calls_text(), "")
        self.assertEqual(self.list_of(self.one), ["a.rs"])

    def test_the_worker_waits_for_an_update_that_runs(self):
        # An update of the session start runs: the worker waits for it.
        folder = self.folder_of(self.one)
        os.makedirs(folder)
        with open(os.path.join(folder, "lock"), "w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.edit_in(os.path.join(self.one, "a.rs"))
            time.sleep(1.0)
            self.assertEqual(self.calls_text(), "")
        self.assertTrue(self.wait_for(lambda: self.finished(self.one)))
        self.assertEqual(self.runs(), 1)
        self.assertEqual(self.list_of(self.one), [])

    def test_an_edit_during_the_update_runs_the_update_again(self):
        self.edit_in(os.path.join(self.one, "a.rs"), FAKE_KAIROS_SLEEP="1.5")
        self.assertTrue(self.wait_for(lambda: self.runs() == 1))
        self.edit_in(os.path.join(self.one, "b.rs"), FAKE_KAIROS_SLEEP="1.5")
        self.assertTrue(self.wait_for(lambda: self.finished(self.one, 2), 15))
        self.assertEqual(self.runs(), 2)
        second = self.log_of(self.one).split("Edited in a session")[2]
        self.assertIn("- b.rs", second)
        self.assertNotIn("- a.rs", second)
        self.assertEqual(self.list_of(self.one), [])

    def test_an_edit_that_an_update_took_already_runs_no_second_update(self):
        self.edit_in(os.path.join(self.one, "a.rs"), quiet="1")
        # The update of the session start takes the list first.
        self.run_hook("worker", args=(self.one,))
        self.assertEqual(self.runs(), 1)
        time.sleep(1.8)
        self.assertEqual(self.runs(), 1, self.calls_text())

    def test_an_edit_outside_a_checkout_records_and_starts_nothing(self):
        self.edit_in(os.path.join(self.board, "notes.py"))
        self.edit_in("/somewhere/else/lib.rs")
        time.sleep(1.0)
        self.assertFalse(os.path.exists(self.state))
        self.assertEqual(self.calls_text(), "")

    def test_an_edit_in_an_unwired_checkout_outside_the_session_records_nothing(self):
        other = self.checkout(os.path.join(os.path.dirname(self.project), "other"))
        self.edit_in(os.path.join(other, "lib.rs"), session=self.project)
        time.sleep(1.0)
        self.assertFalse(os.path.exists(self.state))
        self.assertEqual(self.calls_text(), "")

    def test_a_wired_checkout_outside_the_session_is_updated(self):
        self.edit_in(os.path.join(self.project, "src", "lib.rs"), session=self.board)
        self.assertTrue(self.wait_for(lambda: self.finished(self.project)))
        self.assertIn(f"cwd: {self.project}", self.calls_text())

    def test_no_kairos_on_path_records_the_edit_and_starts_nothing(self):
        os.remove(os.path.join(self.bin, "kairos"))
        self.edit_in(os.path.join(self.one, "a.rs"))
        time.sleep(1.0)
        self.assertEqual(self.list_of(self.one), ["a.rs"])
        self.assertEqual(self.log_of(self.one), "")


class DebounceSeconds(unittest.TestCase):
    """The value of KAIROS_INDEX_DEBOUNCE_SECONDS."""

    def seconds(self, value):
        sys.path.insert(0, HERE)
        try:
            import index_update
        finally:
            sys.path.remove(HERE)
        saved = os.environ.get("KAIROS_INDEX_DEBOUNCE_SECONDS")
        try:
            if value is None:
                os.environ.pop("KAIROS_INDEX_DEBOUNCE_SECONDS", None)
            else:
                os.environ["KAIROS_INDEX_DEBOUNCE_SECONDS"] = value
            return index_update.debounce_seconds()
        finally:
            if saved is None:
                os.environ.pop("KAIROS_INDEX_DEBOUNCE_SECONDS", None)
            else:
                os.environ["KAIROS_INDEX_DEBOUNCE_SECONDS"] = saved

    def test_the_default_is_30_seconds(self):
        self.assertEqual(self.seconds(None), 30.0)
        self.assertEqual(self.seconds(""), 30.0)

    def test_a_number_of_seconds_is_used(self):
        self.assertEqual(self.seconds("5"), 5.0)
        self.assertEqual(self.seconds(" 0.5 "), 0.5)
        self.assertEqual(self.seconds("0"), 0.0)

    def test_off_stops_the_update_after_an_edit(self):
        self.assertIsNone(self.seconds("off"))
        self.assertIsNone(self.seconds("OFF"))

    def test_a_bad_value_gives_the_default(self):
        for value in ("-1", "soon", "nan", "inf"):
            self.assertEqual(self.seconds(value), 30.0, value)


REFUSAL = (
    "503 files changed since abc1234, the nearest indexed commit in Kairos. "
    "The limit is 200, so the CLI built nothing."
)
FAILED = "The last background update of the index failed"


class PreviousResult(Harness):
    """Scenario: The session hears that the last background update failed
    (KAIROS-T-0519)."""

    def folder(self):
        digest = hashlib.sha256(self.project.encode("utf-8")).hexdigest()[:12]
        return os.path.join(self.state, f"checkout-{digest}")

    def result_path(self):
        return os.path.join(self.folder(), "last-update.json")

    def result(self):
        with open(self.result_path(), encoding="utf-8") as handle:
            return json.load(handle)

    def write_result(self, text):
        os.makedirs(self.folder(), exist_ok=True)
        with open(self.result_path(), "w", encoding="utf-8") as handle:
            handle.write(text)

    def a_result(self, **fields):
        result = {
            "format": 1,
            "exit_code": 1,
            "finished": "2026-10-17T10:00:00+00:00",
            "finished_epoch": time.time() - 60,
            "command": "kairos index update --link-only",
            "refusal": REFUSAL,
        }
        result.update(fields)
        return json.dumps(result)

    def start_text(self):
        result = self.run_hook("start", {"session_id": "s1", "source": "startup"})
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)["hookSpecificOutput"]["additionalContext"]

    def failure_lines(self, text):
        return [line for line in text.splitlines() if line.startswith(FAILED)]

    def compact_text(self):
        result = subprocess.run(
            [sys.executable, ACTIVE_WORK, "compact"],
            input=json.dumps(
                {"session_id": "s1", "hook_event_name": "SessionStart", "source": "compact"}
            ),
            capture_output=True,
            text=True,
            env=self.env(),
            cwd=self.project,
            timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)["hookSpecificOutput"]["additionalContext"]

    # -- the worker records the result

    def test_a_successful_update_records_exit_code_0_and_no_refusal(self):
        self.run_hook("worker", args=(self.project,))
        result = self.result()
        self.assertEqual(result["exit_code"], 0)
        self.assertIsNone(result["refusal"])
        self.assertEqual(result["command"], "kairos index update --link-only")
        self.assertLess(time.time() - result["finished_epoch"], 60)
        self.assertFalse(os.path.exists(self.result_path() + ".tmp"))

    def test_a_refused_update_records_the_exit_code_and_the_refusal(self):
        self.run_hook(
            "worker",
            env=self.env(FAKE_KAIROS_EXIT="1", FAKE_KAIROS_ERROR=REFUSAL),
            args=(self.project,),
        )
        result = self.result()
        self.assertEqual(result["exit_code"], 1)
        self.assertEqual(result["refusal"], REFUSAL)

    def test_a_refusal_with_no_error_line_records_the_last_line(self):
        self.run_hook("worker", env=self.env(FAKE_KAIROS_EXIT="1"), args=(self.project,))
        self.assertEqual(self.result()["refusal"], "Structure: 3 files (fake)")

    def test_a_kairos_that_does_not_start_records_no_exit_code(self):
        os.remove(os.path.join(self.bin, "kairos"))
        self.run_hook("worker", args=(self.project,))
        result = self.result()
        self.assertIsNone(result["exit_code"])
        self.assertTrue(result["refusal"].startswith("Could not start kairos"))

    def test_the_result_of_the_plain_update_after_the_usage_code_is_recorded(self):
        self.run_hook("worker", env=self.env(FAKE_KAIROS_OLD="1"), args=(self.project,))
        result = self.result()
        self.assertEqual(result["exit_code"], 0)
        self.assertEqual(result["command"], "kairos index update")
        self.assertIsNone(result["refusal"])

    def test_the_refusal_comes_from_the_last_run_only(self):
        self.run_hook(
            "worker",
            env=self.env(FAKE_KAIROS_EXIT="1", FAKE_KAIROS_ERROR="the first refusal"),
            args=(self.project,),
        )
        self.run_hook(
            "worker", env=self.env(FAKE_KAIROS_EXIT="2"), args=(self.project,)
        )
        result = self.result()
        self.assertEqual(result["exit_code"], 2)
        self.assertEqual(result["refusal"], "Structure: 3 files (fake)")

    # -- the start hook reads it

    def test_a_previous_success_gives_no_line(self):
        self.write_result(self.a_result(exit_code=0, refusal=None))
        self.assertEqual(self.failure_lines(self.start_text()), [])

    def test_a_previous_refusal_gives_one_line_with_the_text_and_the_command(self):
        self.write_result(self.a_result())
        lines = self.failure_lines(self.start_text())
        self.assertEqual(len(lines), 1, lines)
        line = lines[0]
        self.assertIn("exit code 1", line)
        self.assertIn(f'"{REFUSAL}"', line)
        self.assertIn("run `kairos index update --link-only` in this checkout", line)
        self.assertIn(os.path.join(self.folder(), "update.log"), line)

    def test_a_refusal_of_the_worker_is_in_the_next_start(self):
        self.run_hook(
            "worker",
            env=self.env(FAKE_KAIROS_EXIT="1", FAKE_KAIROS_ERROR=REFUSAL),
            args=(self.project,),
        )
        lines = self.failure_lines(self.start_text())
        self.assertEqual(len(lines), 1, lines)
        self.assertIn(REFUSAL, lines[0])

    def test_no_previous_run_gives_no_line(self):
        self.assertEqual(self.failure_lines(self.start_text()), [])

    def test_a_kairos_that_did_not_start_gives_a_line_with_no_exit_code(self):
        self.write_result(
            self.a_result(exit_code=None, refusal="Could not start kairos: not found")
        )
        lines = self.failure_lines(self.start_text())
        self.assertEqual(len(lines), 1, lines)
        self.assertIn("with no exit code", lines[0])
        self.assertIn("Could not start kairos", lines[0])

    def test_a_stale_result_gives_no_line(self):
        self.write_result(self.a_result(finished_epoch=time.time() - 8 * 24 * 3600))
        self.assertEqual(self.failure_lines(self.start_text()), [])

    def test_an_unreadable_result_gives_no_line(self):
        for text in (
            "not json",
            "[1, 2]",
            "",
            self.a_result(format=2),
            self.a_result(finished_epoch="yesterday"),
            self.a_result(exit_code="1"),
        ):
            with self.subTest(text=text):
                self.write_result(text)
                self.assertEqual(self.failure_lines(self.start_text()), [])

    def test_a_result_with_no_read_permission_gives_no_line(self):
        self.write_result(self.a_result())
        os.chmod(self.result_path(), 0)
        try:
            if os.access(self.result_path(), os.R_OK):
                self.skipTest("this user reads a file with mode 000")
            self.assertEqual(self.failure_lines(self.start_text()), [])
        finally:
            os.chmod(self.result_path(), 0o600)

    def test_a_long_refusal_is_clipped(self):
        self.write_result(self.a_result(refusal="word " * 500))
        line = self.failure_lines(self.start_text())[0]
        self.assertLess(len(line), 1200)
        self.assertIn("...", line)

    # -- the compaction hook reads it

    def test_the_compaction_gives_the_line_of_a_previous_refusal(self):
        self.write_result(self.a_result())
        lines = self.failure_lines(self.compact_text())
        self.assertEqual(len(lines), 1, lines)
        self.assertIn(REFUSAL, lines[0])
        self.assertIn("`kairos index update --link-only`", lines[0])

    def test_the_compaction_gives_no_line_after_a_success_or_no_run(self):
        self.assertEqual(self.failure_lines(self.compact_text()), [])
        self.write_result(self.a_result(exit_code=0, refusal=None))
        self.assertEqual(self.failure_lines(self.compact_text()), [])

    def test_the_compaction_gives_no_line_for_an_unreadable_result(self):
        self.write_result("not json")
        text = self.compact_text()
        self.assertIn("Kairos: Claude Code compacted the context.", text)
        self.assertEqual(self.failure_lines(text), [])


class KairosCodeWrapper(Harness):
    """The `kairos-code` MCP server starts through `plugin/bin/kairos-code`
    (COLLIERY-T-2529): with `kairos` on PATH it is `kairos index mcp`; with no
    `kairos` it is an MCP server with no tools, so that Claude Code shows no
    connection error."""

    def start_wrapper(self, env):
        return subprocess.Popen(
            [WRAPPER],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env=env,
            cwd=self.project,
        )

    def ask(self, process, message):
        process.stdin.write(json.dumps(message) + "\n")
        process.stdin.flush()
        if "id" not in message:
            return None
        ready, _, _ = select.select([process.stdout], [], [], 5)
        self.assertTrue(ready, f"no answer to {message}")
        return json.loads(process.stdout.readline())

    def no_kairos_env(self):
        os.remove(os.path.join(self.bin, "kairos"))
        return self.env()

    def test_the_wrapper_can_run(self):
        self.assertTrue(os.access(WRAPPER, os.X_OK))

    def test_with_kairos_on_path_the_wrapper_runs_kairos_index_mcp(self):
        process = self.start_wrapper(self.env())
        _, stderr = process.communicate("", timeout=10)
        self.assertEqual(process.returncode, 0, stderr)
        self.assertIn("args: index mcp", self.calls_text().splitlines())
        self.assertIn(f"cwd: {self.project}", self.calls_text())

    def test_with_kairos_on_path_the_exit_code_of_kairos_is_kept(self):
        process = self.start_wrapper(self.env(FAKE_KAIROS_EXIT="3"))
        process.communicate("", timeout=10)
        self.assertEqual(process.returncode, 3)

    def test_with_no_kairos_the_wrapper_is_a_server_with_no_tools(self):
        process = self.start_wrapper(self.no_kairos_env())
        try:
            answer = self.ask(
                process,
                {
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-06-18",
                        "capabilities": {},
                        "clientInfo": {"name": "claude-code", "version": "2"},
                    },
                },
            )
            self.assertEqual(answer["id"], 1)
            result = answer["result"]
            self.assertEqual(result["protocolVersion"], "2025-06-18")
            self.assertEqual(result["serverInfo"]["name"], "kairos-code")
            self.assertIn("tools", result["capabilities"])
            self.assertIn("`kairos` is not on PATH", result["instructions"])
            self.assertIsNone(
                self.ask(process, {"jsonrpc": "2.0", "method": "notifications/initialized"})
            )
            answer = self.ask(process, {"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
            self.assertEqual(answer, {"jsonrpc": "2.0", "id": 2, "result": {"tools": []}})
            answer = self.ask(process, {"jsonrpc": "2.0", "id": "p", "method": "ping"})
            self.assertEqual(answer, {"jsonrpc": "2.0", "id": "p", "result": {}})
        finally:
            _, stderr = process.communicate("", timeout=10)
        self.assertEqual(process.returncode, 0)
        self.assertEqual(stderr, "", "no error output")

    def test_with_no_kairos_an_unknown_method_is_refused_and_named(self):
        process = self.start_wrapper(self.no_kairos_env())
        try:
            answer = self.ask(
                process, {"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {}}
            )
        finally:
            process.communicate("", timeout=10)
        self.assertEqual(answer["id"], 7)
        self.assertEqual(answer["error"]["code"], -32601)
        self.assertIn("tools/call", answer["error"]["message"])

    def test_with_no_kairos_bad_input_gets_a_parse_error(self):
        process = self.start_wrapper(self.no_kairos_env())
        process.stdin.write("not json\n")
        process.stdin.flush()
        ready, _, _ = select.select([process.stdout], [], [], 5)
        self.assertTrue(ready)
        answer = json.loads(process.stdout.readline())
        _, stderr = process.communicate("", timeout=10)
        self.assertEqual(answer["error"]["code"], -32700)
        self.assertIsNone(answer["id"])
        self.assertEqual(stderr, "")


class Wiring(unittest.TestCase):
    """Scenarios: The code tools are registered; The Metis index is gone."""

    def load(self, *parts):
        with open(os.path.join(ROOT, *parts), encoding="utf-8") as handle:
            return json.load(handle)

    def test_the_manifest_registers_the_kairos_code_server(self):
        manifest = self.load(".claude-plugin", "plugin.json")
        self.assertEqual(
            manifest["mcpServers"]["kairos-code"],
            {
                "type": "stdio",
                "command": "${CLAUDE_PLUGIN_ROOT}/plugin/bin/kairos-code",
                "args": [],
            },
        )

    def test_the_hooks_run_the_script(self):
        hooks = self.load("plugin", "hooks", "hooks.json")["hooks"]
        starts = [
            hook["command"]
            for entry in hooks["SessionStart"]
            if entry.get("matcher") == "startup|clear"  # KAIROS-T-0525: not on a compaction
            for hook in entry["hooks"]
        ]
        self.assertIn('"${CLAUDE_PLUGIN_ROOT}/plugin/hooks/index_update.py" start', starts)
        records = [
            entry
            for entry in hooks["PostToolUse"]
            for hook in entry["hooks"]
            if hook["command"].endswith("index_update.py\" record")
        ]
        self.assertEqual(len(records), 1)
        matcher = re.compile(records[0]["matcher"])
        for tool in ("Edit", "Write", "MultiEdit", "NotebookEdit"):
            self.assertTrue(matcher.fullmatch(tool), tool)
        for tool in ("Read", "Bash", "mcp__kairos__edit_item"):
            self.assertFalse(matcher.fullmatch(tool), tool)

    def test_no_hook_of_this_repository_runs_metis_index(self):
        with open(os.path.join(ROOT, ".claude", "settings.json")) as handle:
            text = handle.read()
        self.assertNotIn("metis index", text)
        json.loads(text)

    def test_the_implement_skill_uses_the_code_tools_before_files(self):
        path = os.path.join(ROOT, "plugin", "skills", "workflow", "implement", "SKILL.md")
        with open(path, encoding="utf-8") as handle:
            text = handle.read()
        self.assertIn("before you read files", text)
        for tool in ("module_map", "code_search", "callers", "callees"):
            self.assertIn(f"`{tool}`", text)

    def test_the_two_manifests_have_the_same_plugin_version(self):
        # The version goes up once per release, not per task, so the test
        # checks that the manifests agree and not a fixed number.
        version = self.load(".claude-plugin", "plugin.json")["version"]
        market = self.load(".claude-plugin", "marketplace.json")
        self.assertEqual(market["plugins"][0]["version"], version)


if __name__ == "__main__":
    unittest.main()
