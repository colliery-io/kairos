#!/usr/bin/env python3
"""Unit tests for the code-tools-before-grep hook (KAIROS-T-0757): a nudge
on a search over an indexed checkout, once and then each tenth call, and
the index instruction for each subagent. Stdlib only; run with
`python3 -m unittest plugin/hooks/test_index_first.py` (or `angreal test
unit`, which includes it).

The tests run `index_first.py` as a subprocess, the way Claude Code runs
it, with KAIROS_INDEX_STATE_DIR and KAIROS_INDEX_DIR in a temporary
folder. No test calls a server."""

import os as _os  # noqa: E402
import sys as _sys  # noqa: E402

_sys.path.insert(0, _os.path.dirname(_os.path.abspath(__file__)))
import hook_test_env  # noqa: E402,F401  (KAIROS-T-0712: no live env, a temp HOME)

import hashlib
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "index_first.py")
SPEC = importlib.util.spec_from_file_location("index_first", SCRIPT)
index_first = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(index_first)
index_update = index_first.index_update

SESSION = "session-0001"


class Harness(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        base = os.path.realpath(self.tmp.name)
        self.project = os.path.join(base, "checkout")
        self.other = os.path.join(base, "other")
        self.state = os.path.join(base, "state")
        self.index_dir = os.path.join(base, "index")
        for folder in (self.project, self.other):
            os.makedirs(os.path.join(folder, ".git"))
            os.makedirs(os.path.join(folder, "src"))
        self.give_index(self.project)

    def tearDown(self):
        self.tmp.cleanup()

    def give_index(self, checkout):
        folder = os.path.join(self.index_dir, index_update.checkout_key(checkout))
        os.makedirs(folder, exist_ok=True)
        with open(os.path.join(folder, "index.db"), "w") as handle:
            handle.write("")

    def env(self):
        return {
            "PATH": "/usr/bin:/bin",
            "HOME": self.tmp.name,
            "CLAUDE_PROJECT_DIR": self.project,
            "KAIROS_INDEX_STATE_DIR": self.state,
            "KAIROS_INDEX_DIR": self.index_dir,
            "PYTHONDONTWRITEBYTECODE": "1",
        }

    def run_hook(self, mode, payload):
        result = subprocess.run(
            [sys.executable, SCRIPT, mode],
            input=json.dumps(payload),
            capture_output=True,
            text=True,
            env=self.env(),
            timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return result

    def call(self, tool_name, tool_input, cwd=None, session=SESSION, **extra):
        payload = {
            "session_id": session,
            "hook_event_name": "PreToolUse",
            "tool_name": tool_name,
            "tool_input": tool_input,
            "cwd": cwd or self.project,
        }
        payload.update(extra)
        return self.run_hook("tool", payload)

    def context_of(self, result):
        if not result.stdout.strip():
            return None
        output = json.loads(result.stdout)["hookSpecificOutput"]
        self.assertEqual(output["hookEventName"], "PreToolUse")
        self.assertNotIn("permissionDecision", output, "the call is allowed as usual")
        return output["additionalContext"]

    def state_dir(self, checkout=None):
        """The state folder of a checkout under KAIROS_INDEX_STATE_DIR, as the
        hook computes it (the test process has no such variable)."""
        checkout = checkout or self.project
        digest = hashlib.sha256(checkout.encode("utf-8")).hexdigest()[:12]
        return os.path.join(self.state, f"{os.path.basename(checkout)}-{digest}")

    def log_lines(self, checkout=None):
        path = os.path.join(self.state_dir(checkout), "tool-calls.log")
        if not os.path.exists(path):
            return []
        with open(path, encoding="utf-8") as handle:
            return handle.read().splitlines()


class Searches(Harness):
    def test_a_bash_grep_inside_an_indexed_checkout_gets_the_message(self):
        text = self.context_of(self.call("Bash", {"command": "grep -rn 'fn fetch' src/"}))
        self.assertIsNotNone(text)
        self.assertIn("the checkout checkout has a code index", text)
        self.assertIn("`code_search`", text)
        self.assertIn("`symbol` and `references`", text)
        self.assertIn("`callers`", text)
        self.assertIn("grep stays right for literal text", text)
        self.assertLessEqual(len(text.splitlines()), 2)
        self.assertNotIn("http", text)

    def test_a_grep_outside_an_indexed_checkout_is_quiet(self):
        result = self.call("Bash", {"command": "grep -rn foo src/"}, cwd=self.other)
        self.assertEqual(result.stdout, "")
        self.assertEqual(self.log_lines(self.other), [])

    def test_a_grep_with_a_path_into_another_checkout_follows_the_path(self):
        result = self.call("Bash", {"command": f"rg foo {self.project}/src"}, cwd=self.other)
        self.assertIn("checkout checkout", self.context_of(result))

    def test_the_message_comes_on_the_first_call_and_then_each_tenth(self):
        given = []
        for i in range(22):
            text = self.context_of(self.call("Bash", {"command": "grep foo src/"}))
            if text:
                given.append(i)
        self.assertEqual(given, [0, 10, 20])
        with open(os.path.join(self.state_dir(), f"index-first-{SESSION}")) as handle:
            self.assertEqual(handle.read().strip(), "22")

    def test_another_session_has_its_own_count(self):
        self.call("Bash", {"command": "grep foo src/"})
        text = self.context_of(self.call("Bash", {"command": "grep foo src/"}, session="s2"))
        self.assertIsNotNone(text)

    def test_the_grep_tool_and_the_glob_tool_with_a_path(self):
        text = self.context_of(self.call("Grep", {"pattern": "fetch", "path": "src"}))
        self.assertIn("code index", text)
        text = self.context_of(self.call("Glob", {"pattern": "**/*.rs"}, cwd=self.other))
        self.assertIsNone(text)
        # The first Grep counted for the checkout: another session for the Glob.
        text = self.context_of(
            self.call("Glob", {"pattern": "**/*.rs", "path": self.project}, cwd=self.other, session="s3")
        )
        self.assertIn("code index", text)

    def test_a_grep_that_filters_a_pipe_is_not_a_search(self):
        result = self.call("Bash", {"command": "cargo test 2>&1 | grep -E 'test result'"})
        self.assertEqual(result.stdout, "")
        result = self.call("Bash", {"command": "cat log.txt | rg error"})
        self.assertEqual(result.stdout, "")

    def test_a_find_and_other_commands(self):
        text = self.context_of(self.call("Bash", {"command": "find . -name '*.rs' | head"}))
        self.assertIn("code index", text)
        result = self.call("Bash", {"command": "cargo build"})
        self.assertEqual(result.stdout, "")
        result = self.call("Bash", {"command": "ls src && cat src/main.rs"})
        self.assertEqual(result.stdout, "")

    def test_the_paths_of_a_search_are_read_from_the_command(self):
        paths = index_first.search_paths
        self.assertEqual(paths("grep -rn 'fn x' src/ crates/"), (True, ["src/", "crates/"]))
        self.assertEqual(paths("rg foo"), (True, []))
        self.assertEqual(paths("find . -name '*.py'"), (True, ["."]))
        self.assertEqual(paths("fd main src"), (True, ["src"]))
        self.assertEqual(paths("cat x | grep y"), (False, []))
        self.assertEqual(paths("cargo build && grep foo src"), (True, ["src"]))
        self.assertEqual(paths("ls"), (False, []))
        self.assertEqual(paths(None), (False, []))
        # Quotes, option values, `--`, git grep and xargs.
        self.assertEqual(paths('grep "a|b" src/'), (True, ["src/"]))
        self.assertEqual(paths("grep -e foo -A 3 src/"), (True, ["src/"]))
        self.assertEqual(paths("rg -t rust foo crates"), (True, ["crates"]))
        self.assertEqual(paths("grep -- -foo src/"), (True, ["src/"]))
        self.assertEqual(paths("git grep -n foo -- crates"), (True, ["crates"]))
        self.assertEqual(paths("git ls-files | xargs grep -l foo"), (False, []))
        self.assertEqual(paths("xargs -0 grep foo src"), (True, ["src"]))
        self.assertEqual(paths("cargo test 2>&1 | grep -E 'test result'"), (False, []))
        self.assertEqual(paths("grep x src | head"), (True, ["src"]))

    def test_no_session_id_gives_no_message_and_still_logs(self):
        payload = {
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "grep foo src/"},
            "cwd": self.project,
        }
        result = self.run_hook("tool", payload)
        self.assertEqual(result.stdout, "")
        self.assertEqual(len(self.log_lines()), 1)

    def test_each_call_is_logged_with_the_agent(self):
        self.call("Bash", {"command": "grep foo src/"}, agent_id="agent-7", agent_type="Explore")
        self.call("Grep", {"pattern": "foo"})
        lines = self.log_lines()
        self.assertEqual(len(lines), 2)
        first = lines[0].split()
        self.assertEqual(first[1:], [SESSION, "agent-7", "Explore", "PreToolUse", "Bash", "nudge"])
        self.assertEqual(lines[1].split()[1:], [SESSION, "-", "-", "PreToolUse", "Grep", "quiet"])

    def test_bad_input_never_fails_the_call(self):
        for stdin in ("", "null", "[1]", "{not json"):
            result = subprocess.run(
                [sys.executable, SCRIPT, "tool"],
                input=stdin,
                capture_output=True,
                text=True,
                env=self.env(),
                timeout=30,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, "")


class Subagents(Harness):
    def start(self, cwd=None, agent_type="Explore"):
        return self.run_hook(
            "subagent",
            {
                "session_id": SESSION,
                "hook_event_name": "SubagentStart",
                "agent_id": "agent-9",
                "agent_type": agent_type,
                "cwd": cwd or self.project,
            },
        )

    def test_a_subagent_gets_the_index_instruction_with_the_checkout(self):
        output = json.loads(self.start().stdout)["hookSpecificOutput"]
        self.assertEqual(output["hookEventName"], "SubagentStart")
        text = output["additionalContext"]
        self.assertTrue(text.startswith("Code index of this checkout"), text)
        self.assertIn("`code_search`", text)
        self.assertIn("Checkouts with an index: checkout.", text)
        self.assertEqual(
            self.log_lines()[0].split()[1:],
            [SESSION, "agent-9", "Explore", "SubagentStart", "-", "nudge"],
        )

    def test_a_subagent_in_a_board_folder_gets_the_indexed_checkouts(self):
        board = os.path.dirname(self.project)
        text = json.loads(self.start(cwd=board).stdout)["hookSpecificOutput"]["additionalContext"]
        self.assertIn("Checkouts with an index: checkout.", text)
        self.give_index(self.other)
        text = json.loads(self.start(cwd=board).stdout)["hookSpecificOutput"]["additionalContext"]
        self.assertIn("Checkouts with an index: checkout, other.", text)

    def test_a_subagent_with_no_indexed_checkout_is_told_to_fetch(self):
        text = json.loads(self.start(cwd=self.other).stdout)["hookSpecificOutput"]["additionalContext"]
        self.assertIn("No checkout of this session has a local index yet", text)
        self.assertIn("`fetch_index`", text)


if __name__ == "__main__":
    unittest.main()
