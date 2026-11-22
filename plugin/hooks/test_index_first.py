#!/usr/bin/env python3
"""Unit tests for the code-tools-before-grep hook (KAIROS-T-0757,
KAIROS-T-0766): a message on each search over an indexed checkout that the
index answers (a definition, a call, a name over source files), and the
index instruction for each subagent. Stdlib only; run with
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
    def test_a_definition_search_inside_an_indexed_checkout_gets_the_message(self):
        text = self.context_of(self.call("Bash", {"command": "grep -rn 'fn fetch' src/"}))
        self.assertIsNotNone(text)
        self.assertIn("`symbol` gives the definition of fetch in checkout", text)
        self.assertIn("grep stays right for literal text", text)
        self.assertLessEqual(len(text.splitlines()), 2)
        self.assertNotIn("http", text)

    def test_a_grep_outside_an_indexed_checkout_is_quiet(self):
        result = self.call("Bash", {"command": "grep -rn 'fn foo' src/"}, cwd=self.other)
        self.assertEqual(result.stdout, "")
        self.assertEqual(self.log_lines(self.other), [])

    def test_a_grep_with_a_path_into_another_checkout_follows_the_path(self):
        result = self.call("Bash", {"command": f"rg 'def foo' {self.project}/src"}, cwd=self.other)
        self.assertIn("in checkout", self.context_of(result))

    def test_a_checkout_under_a_tmp_folder_is_not_a_log_folder(self):
        # CI runs the tests under /tmp: the folder rule reads the path from
        # the top of the checkout.
        base = os.path.join(os.path.dirname(self.project), "tmp", "build")
        checkout = os.path.join(base, "repo")
        os.makedirs(os.path.join(checkout, ".git"))
        os.makedirs(os.path.join(checkout, "src"))
        os.makedirs(os.path.join(checkout, "logs"))
        self.give_index(checkout)
        text = self.context_of(self.call("Bash", {"command": f"rg 'def foo' {checkout}/src"}, cwd=self.other))
        self.assertIn("in repo", text)
        result = self.call("Bash", {"command": f"rg 'def foo' {checkout}/logs"}, cwd=self.other)
        self.assertEqual(result.stdout, "")

    def test_each_search_that_the_index_answers_gets_the_message(self):
        given = [
            self.context_of(self.call("Bash", {"command": "grep -rn 'struct Index' src/"}))
            for _ in range(3)
        ]
        self.assertTrue(all(given), given)

    def test_a_definition_search_in_each_language(self):
        cases = {
            "rg 'pub struct Index' crates": "Index",
            "rg 'pub(crate) fn fetch'": "fetch",
            "rg 'impl Display for Index'": "Display",
            "rg 'enum Kind'": "Kind",
            "rg 'trait Store'": "Store",
            "rg 'mod index'": "index",
            "grep -rnE 'def cli_note' src": "cli_note",
            "grep -rn 'class Harness' src": "Harness",
            "rg 'export function render'": "render",
            "rg 'export interface Props'": "Props",
            "rg 'export const store ='": "store",
            "rg 'type Props ='": "Props",
            "rg 'func Start'": "Start",
            "rg '^func (s \\*Server) Start'": "Start",
            "rg 'type Server struct'": "Server",
            "rg 'resource \"aws_s3_bucket\"'": "aws_s3_bucket",
            "rg 'variable region'": "region",
            "grep -rni 'create table code_index_refs' src": "code_index_refs",
            "rg 'CREATE OR REPLACE FUNCTION touch'": "touch",
            "rg '(pub )?fn\\s+fetch'": "fetch",
        }
        for command, term in cases.items():
            text = self.context_of(self.call("Bash", {"command": command}))
            self.assertIsNotNone(text, command)
            self.assertIn(f"`symbol` gives the definition of {term} ", text, command)

    def test_a_call_search_names_callers(self):
        for command in ("rg 'fetch_index\\('", "grep -rn 'Index::source(' src"):
            text = self.context_of(self.call("Bash", {"command": command}))
            self.assertIn("`callers` gives each function in checkout that calls", text, command)

    def test_a_name_over_source_files_names_references(self):
        for command in (
            "rg -t rust tools_off",
            "grep -rn cli_note --include=*.py src",
            "git grep -n cli_note -- '*.py'",
            "grep -n cli_note src/a.py src/b.py",
        ):
            text = self.context_of(self.call("Bash", {"command": command}))
            self.assertIsNotNone(text, command)
            self.assertIn("`references` gives each use of", text, command)
        text = self.context_of(self.call("Grep", {"pattern": "cli_note", "type": "py"}))
        self.assertIn("`references` gives each use of cli_note", text)

    def test_literal_text_gets_no_message_and_is_logged_quiet(self):
        for command in (
            "rg 'connection refused'",
            "grep -rn 'type error in the file' src",
            "grep -n 'fn main' server.log",
            "rg 'class Foo' docs/guide.md",
            "grep -rn 'fn x' logs/",
            "grep -rn KAIROS_URL --include=*.toml .",
            "grep -rn tools_off src",
            "rg struct",
        ):
            result = self.call("Bash", {"command": command})
            self.assertEqual(result.stdout, "", command)
        self.assertTrue(all(line.endswith(" quiet") for line in self.log_lines()))

    def test_the_grep_tool_with_a_definition_and_the_glob_tool(self):
        text = self.context_of(self.call("Grep", {"pattern": "def fetch", "path": "src"}))
        self.assertIn("`symbol`", text)
        result = self.call("Glob", {"pattern": "**/*.rs", "path": self.project})
        self.assertEqual(result.stdout, "")

    def test_the_parts_of_a_search_are_read(self):
        parts = index_first.searches_of
        self.assertEqual(
            parts("grep -rnE 'fn x' --include=*.rs src"),
            [{"name": "grep", "pattern": "fn x", "paths": ["src"], "filters": ["*.rs"]}],
        )
        self.assertEqual(
            parts("rg -t rust -e foo crates"),
            [{"name": "rg", "pattern": "foo", "paths": ["crates"], "filters": ["rust"]}],
        )
        self.assertEqual(parts("rg -f patterns.txt src")[0]["pattern"], None)

    def test_a_grep_that_filters_a_pipe_is_not_a_search(self):
        result = self.call("Bash", {"command": "cargo test 2>&1 | grep -E 'test result'"})
        self.assertEqual(result.stdout, "")
        result = self.call("Bash", {"command": "cat log.txt | rg error"})
        self.assertEqual(result.stdout, "")

    def test_a_find_and_other_commands(self):
        result = self.call("Bash", {"command": "find . -name '*.rs' | head"})
        self.assertEqual(result.stdout, "")
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
        self.assertEqual(paths("git ls-files | xargs grep -l foo"), (True, []))
        self.assertEqual(paths("xargs -0 grep foo src"), (True, ["src"]))
        self.assertEqual(paths("cargo test 2>&1 | grep -E 'test result'"), (False, []))
        self.assertEqual(paths("grep x src | head"), (True, ["src"]))

    def test_no_session_id_still_gives_the_message_and_logs(self):
        payload = {
            "hook_event_name": "PreToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "grep 'fn foo' src/"},
            "cwd": self.project,
        }
        result = self.run_hook("tool", payload)
        self.assertIn("`symbol`", result.stdout)
        self.assertEqual(len(self.log_lines()), 1)

    def test_each_call_is_logged_with_the_agent(self):
        self.call("Bash", {"command": "grep 'fn foo' src/"}, agent_id="agent-7", agent_type="Explore")
        self.call("Grep", {"pattern": "foo bar"})
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
