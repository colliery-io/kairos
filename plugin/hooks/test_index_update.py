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
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "index_update.py")
ROOT = os.path.dirname(os.path.dirname(HERE))

WIRING = "---\ndeployment_url: http://127.0.0.1:1\nrepository: kairos\n---\n"

# A fake `kairos`: it writes its arguments, its folder and the list of
# changed files (as the worker left it) to calls.txt, then sleeps and exits
# with the code in FAKE_KAIROS_EXIT.
FAKE_KAIROS = """#!/bin/sh
{
  echo "args: $*"
  echo "cwd: $(pwd)"
} >> "$FAKE_KAIROS_CALLS"
if [ -n "$FAKE_KAIROS_EDIT" ]; then
  echo "$FAKE_KAIROS_EDIT" >> "$FAKE_KAIROS_LIST"
fi
sleep "${FAKE_KAIROS_SLEEP:-0}"
echo "Structure: 3 files (fake)"
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
        self.assertTrue(
            self.wait_for(lambda: "args: index update" in self.calls_text()),
            "the background worker did not run `kairos index update`",
        )
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
        for tool in ("module_map", "code_search", "callers", "callees"):
            self.assertIn(f"`{tool}`", text)
        self.assertIn("before you read files", text)
        self.assertIn("kairos-code", text)
        self.assertIn("update.log", text)

    def test_no_kairos_on_path_is_a_silent_no_op(self):
        os.remove(os.path.join(self.bin, "kairos"))
        result = self.run_hook("start", {"session_id": "s1", "source": "startup"})
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertEqual(result.stderr, "")
        self.assertFalse(os.path.exists(self.state))

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


class Wiring(unittest.TestCase):
    """Scenarios: The code tools are registered; The Metis index is gone."""

    def load(self, *parts):
        with open(os.path.join(ROOT, *parts), encoding="utf-8") as handle:
            return json.load(handle)

    def test_the_manifest_registers_the_kairos_code_server(self):
        manifest = self.load(".claude-plugin", "plugin.json")
        self.assertEqual(
            manifest["mcpServers"]["kairos-code"],
            {"type": "stdio", "command": "kairos", "args": ["index", "mcp"]},
        )

    def test_the_hooks_run_the_script(self):
        hooks = self.load("plugin", "hooks", "hooks.json")["hooks"]
        starts = [
            hook["command"]
            for entry in hooks["SessionStart"]
            if entry.get("matcher") == "*"
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

    def test_the_plugin_version_is_0_8_0(self):
        self.assertEqual(self.load(".claude-plugin", "plugin.json")["version"], "0.8.0")
        market = self.load(".claude-plugin", "marketplace.json")
        self.assertEqual(market["plugins"][0]["version"], "0.8.0")


if __name__ == "__main__":
    unittest.main()
