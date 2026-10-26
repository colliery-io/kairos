#!/usr/bin/env python3
"""Unit tests for the memory reminder (KAIROS-T-0502, COLLIERY-I-0628): a
PreToolUse hook on Write, Edit and MultiEdit that reminds, once per session
per file, that a decision about the system belongs in Kairos when the agent
writes to its local memory folder. It never blocks. Stdlib only; run with
`python3 -m unittest plugin/hooks/test_memory_reminder.py` (or `angreal
test unit`, which includes it).

The tests run `memory_reminder.py` as a subprocess, the way Claude Code
runs it, with KAIROS_MEMORY_STATE_DIR and CLAUDE_PROJECT_DIR pointing at a
temporary directory. No test calls a server."""

import os as _os  # noqa: E402
import sys as _sys  # noqa: E402

_sys.path.insert(0, _os.path.dirname(_os.path.abspath(__file__)))
import hook_test_env  # noqa: E402,F401  (KAIROS-T-0712: no live env, a temp HOME)


import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "memory_reminder.py")
SPEC = importlib.util.spec_from_file_location("memory_reminder", SCRIPT)
memory_reminder = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(memory_reminder)

SESSION = "session-0001"
MEMORY = "/home/robin/.claude/projects/-home-robin-work-payments/memory/MEMORY.md"
NOTE = "/home/robin/.claude/projects/-home-robin-work-payments/memory/refunds.md"

WIRING = (
    "---\n"
    "deployment_url: https://kairos.example.com\n"
    "tenant: acme\n"
    "repository: payments-api\n"
    "team_board: platform-delivery\n"
    "adr_board: platform-adrs\n"
    "---\n\n# Kairos wiring\n"
)


def payload(tool_name="Write", file_path=MEMORY, session=SESSION):
    return json.dumps(
        {
            "session_id": session,
            "tool_name": tool_name,
            "tool_input": {"file_path": file_path, "content": "secret text that is never read"},
        }
    )


class MemoryReminderCase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.project = os.path.join(self.tmp.name, "project")
        os.makedirs(os.path.join(self.project, ".claude"))
        self.state_dir = os.path.join(self.tmp.name, "memory")
        self.env = dict(
            os.environ,
            KAIROS_MEMORY_STATE_DIR=self.state_dir,
            CLAUDE_PROJECT_DIR=self.project,
        )
        self.env.pop("CLAUDE_MEMORY_DIR", None)

    def tearDown(self):
        self.tmp.cleanup()

    def wire(self):
        with open(os.path.join(self.project, ".claude", "kairos.local.md"), "w") as handle:
            handle.write(WIRING)

    def run_hook(self, stdin, env=None):
        return subprocess.run(
            [sys.executable, SCRIPT],
            input=stdin,
            capture_output=True,
            text=True,
            env=env or self.env,
            cwd=self.tmp.name,
            check=False,
        )

    def context(self, result):
        self.assertEqual(result.returncode, 0, result.stderr)
        if not result.stdout:
            return None
        output = json.loads(result.stdout)["hookSpecificOutput"]
        self.assertEqual(output["hookEventName"], "PreToolUse")
        self.assertEqual(output.get("permissionDecision", "allow"), "allow")
        return output["additionalContext"]


class FiresOnAMemoryPath(MemoryReminderCase):
    def test_a_write_to_memory_gets_the_reminder_and_never_blocks(self):
        text = self.context(self.run_hook(payload()))
        self.assertIsNotNone(text)
        self.assertIn("local memory", text)
        self.assertIn("ADR", text)
        self.assertIn("`create_item`", text)
        self.assertIn("`append_note`", text)
        self.assertIn("COLLIERY-I-0628", text)
        self.assertNotIn("secret text", text)

    def test_each_write_tool_fires(self):
        for index, tool in enumerate(("Write", "Edit", "MultiEdit")):
            with self.subTest(tool=tool):
                text = self.context(self.run_hook(payload(tool, session=f"s-{index}")))
                self.assertIsNotNone(text)

    def test_the_adr_board_comes_from_the_wiring(self):
        self.wire()
        text = self.context(self.run_hook(payload()))
        self.assertIn("`board: platform-adrs`", text)

    def test_with_no_wiring_the_board_is_found_with_my_boards(self):
        text = self.context(self.run_hook(payload()))
        self.assertIn("`my_boards`", text)

    def test_claude_memory_dir_is_a_memory_folder_too(self):
        env = dict(self.env, CLAUDE_MEMORY_DIR=os.path.join(self.tmp.name, "mem"))
        path = os.path.join(self.tmp.name, "mem", "notes.md")
        self.assertIsNotNone(self.context(self.run_hook(payload(file_path=path), env=env)))

    def test_a_relative_memory_path_fires(self):
        self.assertIsNotNone(
            self.context(self.run_hook(payload(file_path=".claude/projects/x/memory/a.md")))
        )


class OncePerSessionPerFile(MemoryReminderCase):
    def test_the_second_write_to_the_same_file_is_silent(self):
        self.assertIsNotNone(self.context(self.run_hook(payload())))
        self.assertIsNone(self.context(self.run_hook(payload())))

    def test_a_different_file_fires_again(self):
        self.context(self.run_hook(payload()))
        self.assertIsNotNone(self.context(self.run_hook(payload(file_path=NOTE))))

    def test_a_different_session_fires_again(self):
        self.context(self.run_hook(payload()))
        self.assertIsNotNone(self.context(self.run_hook(payload(session="session-0002"))))


class SilentElsewhere(MemoryReminderCase):
    def test_other_paths_and_tools_give_no_output(self):
        shapes = (
            payload(file_path="/home/robin/work/payments/src/refund.rs"),
            payload(file_path="/home/robin/work/payments/docs/memory.md"),
            payload(file_path="/home/robin/.claude/projects/x/notes/a.md"),
            payload(file_path="/home/robin/work/memory/a.md"),
            payload(tool_name="Read"),
            payload(tool_name="Bash"),
        )
        for shape in shapes:
            with self.subTest(shape=shape):
                result = self.run_hook(shape)
                self.assertEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
        self.assertFalse(os.path.exists(self.state_dir))

    def test_bad_input_gives_no_output_and_exit_0(self):
        for stdin in ("", "not json", "[]", json.dumps({"tool_name": "Write"})):
            with self.subTest(stdin=stdin):
                result = self.run_hook(stdin)
                self.assertEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")


class Pure(unittest.TestCase):
    def test_is_memory_path(self):
        self.assertTrue(memory_reminder.is_memory_path(MEMORY, None))
        self.assertTrue(memory_reminder.is_memory_path("/x/.claude/projects/p/memory/n.md", None))
        self.assertFalse(memory_reminder.is_memory_path("/x/.claude/projects/p/n.md", None))
        self.assertFalse(memory_reminder.is_memory_path("/x/memory/n.md", None))
        self.assertTrue(memory_reminder.is_memory_path("/m/n.md", "/m"))
        self.assertFalse(memory_reminder.is_memory_path("/mm/n.md", "/m"))
        self.assertFalse(memory_reminder.is_memory_path(None, "/m"))


class Registered(unittest.TestCase):
    def test_the_hook_is_registered_on_the_write_tools(self):
        import re

        with open(os.path.join(HERE, "hooks.json"), encoding="utf-8") as handle:
            hooks = json.load(handle)
        entries = [
            entry
            for entry in hooks["hooks"]["PreToolUse"]
            if any("memory_reminder.py" in hook["command"] for hook in entry["hooks"])
        ]
        self.assertEqual(len(entries), 1)
        matcher = re.compile(entries[0]["matcher"])
        for name in ("Write", "Edit", "MultiEdit"):
            self.assertTrue(matcher.search(name), name)
        self.assertFalse(matcher.search("Read"))


if __name__ == "__main__":
    unittest.main()
