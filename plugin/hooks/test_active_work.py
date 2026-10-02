#!/usr/bin/env python3
"""Unit tests for the active work after a compaction (COLLIERY-T-1862):
the `record` side (the PostToolUse hook on the kairos MCP tools) and the
`compact` side (the SessionStart hook with the matcher `compact`). Stdlib
only; run with `python3 -m unittest plugin/hooks/test_active_work.py` (or
`angreal test unit`, which includes it).

The tests run `active_work.py` as a subprocess, the way Claude Code runs
it, with KAIROS_WORK_STATE_DIR, KAIROS_RALPH_STATE_DIR and
CLAUDE_PROJECT_DIR pointing at a temporary directory. The part that needs a
live session (a real compaction) is the manual check on the task."""

import json
import os
import subprocess
import sys
import tempfile
import time
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "active_work.py")
SESSION = "session-0001"

WIRING = (
    "---\n"
    "deployment_url: {url}\n"
    "tenant: acme\n"
    "repository: payments-api\n"
    "delivery_stream: platform\n"
    "team_board: platform-delivery\n"
    "initiative_board: initiatives\n"
    "---\n\n# Kairos wiring\n"
)

GET_ITEM = """# ACME-T-0042 — Refunds keep the original currency
- type: task (task) · lane: planned · repository: {repository} (owner: platform)
- board: platform-delivery / column: {column}
- version: 4
- updated: 2026-10-01T10:00:00Z

## Relationships
- parent chain: ACME-I-0007 (Multi-currency refunds) <- ACME-S-0001 (Payments everywhere)
- blocked by: ACME-T-0040 — Currency table [done]

## Content
## Objective

Refunds use the currency of the charge.

## Relationships

- parent chain: ACME-I-9999 (a heading in the content is not an edge)

## Status, 2026-10-01

- Read the refund path. Next step: an old note.
- Wrote the failing test for EUR. Next step: change `refund::amount` to keep the currency.
"""


def get_item_text(repository="payments-api", column="Active"):
    return GET_ITEM.format(repository=repository, column=column)


class ActiveWorkCase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.project = os.path.join(self.tmp.name, "project")
        os.makedirs(os.path.join(self.project, ".claude"))
        self.work_dir = os.path.join(self.tmp.name, "work")
        self.ralph_dir = os.path.join(self.tmp.name, "ralph")
        self.env = dict(
            os.environ,
            KAIROS_WORK_STATE_DIR=self.work_dir,
            KAIROS_RALPH_STATE_DIR=self.ralph_dir,
            CLAUDE_PROJECT_DIR=self.project,
        )
        self.wire("https://acme.kairos.example")

    def tearDown(self):
        self.tmp.cleanup()

    def wire(self, url):
        path = os.path.join(self.project, ".claude", "kairos.local.md")
        with open(path, "w", encoding="utf-8") as f:
            f.write(WIRING.format(url=url))

    def run_script(self, command, payload):
        stdin = payload if isinstance(payload, str) else json.dumps(payload)
        return subprocess.run(
            [sys.executable, SCRIPT, command],
            input=stdin,
            capture_output=True,
            text=True,
            env=self.env,
            cwd=self.project,
            check=False,
            timeout=20,
        )

    def tool_use(self, tool, tool_input, response, session=SESSION):
        """A PostToolUse call, with the MCP result as content blocks."""
        result = self.run_script(
            "record",
            {
                "session_id": session,
                "hook_event_name": "PostToolUse",
                "tool_name": f"mcp__kairos__{tool}",
                "tool_input": tool_input,
                "tool_response": [{"type": "text", "text": response}],
            },
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "", "the record hook gives no output")
        return result

    def compact(self, session=SESSION):
        """A SessionStart with source `compact`. Returns (result, context)."""
        result = self.run_script(
            "compact",
            {
                "session_id": session,
                "hook_event_name": "SessionStart",
                "source": "compact",
            },
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        if not result.stdout.strip():
            return result, ""
        output = json.loads(result.stdout)
        self.assertEqual(output["hookSpecificOutput"]["hookEventName"], "SessionStart")
        return result, output["hookSpecificOutput"]["additionalContext"]

    def write_ralph(self, state, session=SESSION):
        os.makedirs(self.ralph_dir, exist_ok=True)
        with open(os.path.join(self.ralph_dir, f"{session}.json"), "w") as f:
            json.dump(state, f)


class AnActiveTaskIsNamed(ActiveWorkCase):
    """Scenario: An active task is named after a compaction."""

    def test_the_context_has_code_title_initiative_and_next_step(self):
        self.tool_use("get_item", {"short_code": "ACME-T-0042"}, get_item_text())
        _, context = self.compact()
        self.assertIn("ACME-T-0042", context)
        self.assertIn("Refunds keep the original currency", context)
        self.assertIn("ACME-I-0007", context)
        self.assertIn("Multi-currency refunds", context)
        self.assertIn("change `refund::amount` to keep the currency", context)
        self.assertNotIn("an old note", context)

    def test_a_transition_and_an_edit_update_the_record(self):
        self.tool_use("get_item", {"short_code": "ACME-T-0042"}, get_item_text(column="Todo"))
        _, context = self.compact()
        self.assertNotIn("ACME-T-0042", context, "a task in Todo is not active work")
        self.tool_use(
            "transition_item",
            {"short_code": "ACME-T-0042", "to_column": "Active"},
            "Transitioned ACME-T-0042: Todo -> Active.",
        )
        self.tool_use(
            "edit_item",
            {
                "short_code": "ACME-T-0042",
                "search": "x",
                "replace": "- The test is green.\n- **Next step:** run the gates.",
            },
            "Edited ACME-T-0042 to version 5 (1 replacement).",
        )
        _, context = self.compact()
        self.assertIn("ACME-T-0042", context)
        self.assertIn("Refunds keep the original currency", context)
        self.assertIn("run the gates", context)
        self.assertIn("ACME-I-0007", context)

    def test_a_completed_task_is_not_active_work(self):
        self.tool_use("get_item", {"short_code": "ACME-T-0042"}, get_item_text())
        self.tool_use(
            "transition_item",
            {"short_code": "ACME-T-0042", "to_column": "Completed"},
            "Transitioned ACME-T-0042: Active -> Completed.",
        )
        _, context = self.compact()
        self.assertNotIn("ACME-T-0042", context)

    def test_a_task_of_a_different_repository_is_not_named(self):
        self.tool_use(
            "get_item", {"short_code": "ACME-T-0042"}, get_item_text(repository="web-app")
        )
        _, context = self.compact()
        self.assertNotIn("ACME-T-0042", context)

    def test_a_task_of_a_different_session_is_not_named(self):
        self.tool_use(
            "get_item", {"short_code": "ACME-T-0042"}, get_item_text(), session="other-session"
        )
        _, context = self.compact()
        self.assertNotIn("ACME-T-0042", context)

    def test_the_context_says_to_confirm_with_get_item(self):
        self.tool_use("get_item", {"short_code": "ACME-T-0042"}, get_item_text())
        _, context = self.compact()
        self.assertIn("get_item", context)
        self.assertIn("board_items", context)


class ARalphLoopIsNamed(ActiveWorkCase):
    """Scenario: A running Ralph loop is named after a compaction."""

    def test_the_context_names_the_tasks_left_and_the_iteration(self):
        self.write_ralph(
            {
                "mode": "tasks",
                "tasks": ["ACME-T-0041", "ACME-T-0042", "ACME-T-0043"],
                "iteration": 3,
                "max_iterations": 10,
                "session_id": SESSION,
            }
        )
        self.tool_use(
            "transition_item",
            {"short_code": "ACME-T-0041", "to_column": "Completed"},
            "Transitioned ACME-T-0041: Active -> Completed.",
        )
        _, context = self.compact()
        self.assertIn("Ralph loop", context)
        self.assertIn("iteration 3 of 10", context)
        self.assertIn("ACME-T-0042, ACME-T-0043", context)
        self.assertIn("2 tasks left", context)
        self.assertNotIn("ACME-T-0041", context)

    def test_an_initiative_loop_names_the_initiative(self):
        self.write_ralph(
            {
                "mode": "initiative",
                "initiative": "ACME-I-0007",
                "tasks": ["ACME-T-0042", "ACME-T-0043"],
                "iteration": 1,
                "max_iterations": 0,
                "session_id": SESSION,
            }
        )
        _, context = self.compact()
        self.assertIn("ACME-I-0007", context)
        self.assertIn("iteration 1 (no limit)", context)
        self.assertIn("ACME-T-0042, ACME-T-0043", context)

    def test_the_loop_of_a_different_session_is_not_named(self):
        self.write_ralph(
            {
                "mode": "tasks",
                "tasks": ["ACME-T-0042"],
                "iteration": 2,
                "max_iterations": 0,
            },
            session="other-session",
        )
        _, context = self.compact()
        self.assertNotIn("Ralph loop", context)


class NoKairosNoFailure(ActiveWorkCase):
    """Scenario: No Kairos, no failure."""

    def test_an_unreachable_deployment_does_not_block_and_names_board_items(self):
        self.wire("http://127.0.0.1:9")  # nothing listens on the discard port
        started = time.monotonic()
        result, context = self.compact()
        self.assertLess(time.monotonic() - started, 5)
        self.assertEqual(result.returncode, 0)
        self.assertNotIn('"decision"', result.stdout)
        self.assertIn("board_items", context)
        self.assertIn("platform-delivery", context)
        self.assertIn("repository=payments-api", context)
        self.assertIn("column=Active", context)

    def test_a_broken_record_and_a_broken_loop_state_still_name_board_items(self):
        os.makedirs(self.work_dir)
        with open(os.path.join(self.work_dir, f"{SESSION}.json"), "w") as f:
            f.write("{not json")
        os.makedirs(self.ralph_dir)
        with open(os.path.join(self.ralph_dir, f"{SESSION}.json"), "w") as f:
            f.write("[]")
        result, context = self.compact()
        self.assertEqual(result.returncode, 0)
        self.assertIn("board_items", context)

    def test_bad_hook_input_gives_no_output_and_exit_0(self):
        for stdin in ("", "not json", "[]", json.dumps({"session_id": "../x"})):
            for command in ("record", "compact"):
                result = self.run_script(command, stdin)
                self.assertEqual(result.returncode, 0, (command, stdin, result.stderr))
                self.assertEqual(result.stdout, "", (command, stdin))

    def test_not_a_kairos_repository_gives_no_output(self):
        os.remove(os.path.join(self.project, ".claude", "kairos.local.md"))
        result, context = self.compact()
        self.assertEqual(context, "")
        self.assertEqual(result.stdout, "")


class Record(ActiveWorkCase):
    def record(self):
        with open(os.path.join(self.work_dir, f"{SESSION}.json"), encoding="utf-8") as f:
            return json.load(f)

    def test_a_tool_of_a_different_server_is_not_recorded(self):
        result = self.run_script(
            "record",
            {
                "session_id": SESSION,
                "tool_name": "mcp__metis__read_document",
                "tool_input": {"short_code": "ACME-T-0042"},
                "tool_response": get_item_text(),
            },
        )
        self.assertEqual(result.returncode, 0)
        self.assertFalse(os.path.exists(os.path.join(self.work_dir, f"{SESSION}.json")))

    def test_a_plain_string_response_is_read(self):
        result = self.run_script(
            "record",
            {
                "session_id": SESSION,
                "tool_name": "mcp__plugin_kairos_kairos__get_item",
                "tool_input": {"short_code": "ACME-T-0042"},
                "tool_response": get_item_text(),
            },
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        task = self.record()["tasks"]["ACME-T-0042"]
        self.assertEqual(task["title"], "Refunds keep the original currency")
        self.assertEqual(task["column"], "Active")
        self.assertEqual(task["repository"], "payments-api")
        self.assertEqual(task["initiative"], "ACME-I-0007 (Multi-currency refunds)")

    def test_an_item_that_is_not_a_task_is_not_recorded(self):
        self.tool_use(
            "transition_item",
            {"short_code": "ACME-I-0007", "to_column": "Active"},
            "Transitioned ACME-I-0007: Ready -> Active.",
        )
        self.assertEqual(self.record()["tasks"], {})

    def test_an_update_item_takes_the_title_and_the_next_step(self):
        self.tool_use(
            "update_item",
            {
                "short_code": "ACME-T-0042",
                "title": "Refunds keep the currency",
                "content": "## Status\n\n- Next: write the docs.\n",
                "version": 4,
            },
            "Updated ACME-T-0042 to version 5.",
        )
        task = self.record()["tasks"]["ACME-T-0042"]
        self.assertEqual(task["title"], "Refunds keep the currency")
        self.assertEqual(task["next_step"], "write the docs.")


if __name__ == "__main__":
    unittest.main()
