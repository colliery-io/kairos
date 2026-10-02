#!/usr/bin/env python3
"""Unit tests for the Ralph loop of the kairos plugin (COLLIERY-T-1861):
the start commands, the Stop hook, cancel, and the order of the tasks of an
initiative. Stdlib only; run with
`python3 -m unittest plugin/hooks/test_ralph.py` (or `angreal test unit`,
which includes it).

The tests run `ralph.py` as a subprocess, the way Claude Code runs it, with
KAIROS_RALPH_STATE_DIR pointing at a temporary directory. The part of a
scenario that needs a live session (the agent doing the work) is the manual
run on the task."""

import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "ralph.py")
SPEC = importlib.util.spec_from_file_location("ralph", SCRIPT)
ralph = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ralph)

SESSION = "session-0001"


class RalphCase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.state_dir = os.path.join(self.tmp.name, "state")
        self.env = dict(os.environ, KAIROS_RALPH_STATE_DIR=self.state_dir)
        self.env.pop("CLAUDE_SESSION_ID", None)

    def tearDown(self):
        self.tmp.cleanup()

    def run_script(self, *args, stdin=""):
        return subprocess.run(
            [sys.executable, SCRIPT, *args],
            input=stdin,
            capture_output=True,
            text=True,
            env=self.env,
            cwd=self.tmp.name,
            check=False,
        )

    def state_path(self, session=SESSION):
        return os.path.join(self.state_dir, f"{session}.json")

    def state(self, session=SESSION):
        with open(self.state_path(session), encoding="utf-8") as f:
            return json.load(f)

    def transcript(self, text):
        """A transcript in the JSONL shape of Claude Code, with one
        assistant message whose text is `text`."""
        path = os.path.join(self.tmp.name, "transcript.jsonl")
        lines = [
            {"type": "user", "message": {"role": "user", "content": "go"}},
            {
                "type": "assistant",
                "message": {
                    "role": "assistant",
                    "content": [
                        {"type": "tool_use", "name": "Bash", "input": {}},
                        {"type": "text", "text": text},
                    ],
                },
            },
        ]
        with open(path, "w", encoding="utf-8") as f:
            for line in lines:
                f.write(json.dumps(line) + "\n")
        return path

    def stop(self, text="Working on it.", session=SESSION, use_field=True):
        """Run the Stop hook. With `use_field` the last message comes in
        `last_assistant_message`; without it the hook reads the transcript."""
        payload = {
            "session_id": session,
            "hook_event_name": "Stop",
            "stop_hook_active": True,
            "cwd": self.tmp.name,
            "transcript_path": self.transcript(text),
        }
        if use_field:
            payload["last_assistant_message"] = text
        return self.run_script("stop", stdin=json.dumps(payload))

    def start(self, *codes, extra=()):
        return self.run_script("start", "--session", SESSION, *extra, *codes)


class StartWithTaskCodes(RalphCase):
    def test_valid_codes_write_the_state(self):
        result = self.start("COLLIERY-T-0001", "COLLIERY-T-0002")
        self.assertEqual(result.returncode, 0, result.stderr)
        state = self.state()
        self.assertEqual(state["mode"], "tasks")
        self.assertEqual(state["tasks"], ["COLLIERY-T-0001", "COLLIERY-T-0002"])
        self.assertEqual(state["iteration"], 1)
        self.assertEqual(state["max_iterations"], 0)
        self.assertEqual(state["completion_promise"], "ALL TASKS COMPLETE")
        self.assertEqual(state["session_id"], SESSION)
        self.assertIn("COLLIERY-T-0001", result.stdout)
        self.assertIn("<promise>ALL TASKS COMPLETE</promise>", result.stdout)

    def test_the_state_is_outside_the_repository(self):
        self.start("COLLIERY-T-0001")
        self.assertTrue(os.path.isfile(self.state_path()))
        self.assertFalse(os.path.exists(os.path.join(self.tmp.name, ".claude")))

    def test_max_iterations_is_recorded(self):
        result = self.start("COLLIERY-T-0001", extra=("--max-iterations", "2"))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.state()["max_iterations"], 2)

    def test_the_session_comes_from_the_environment(self):
        self.env["CLAUDE_SESSION_ID"] = "from-env"
        result = self.run_script("start", "COLLIERY-T-0001")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.state("from-env")["session_id"], "from-env")


class StartRefuses(RalphCase):
    def assert_refused(self, result, *names):
        self.assertNotEqual(result.returncode, 0)
        for name in names:
            self.assertIn(name, result.stderr)
        self.assertFalse(os.path.exists(self.state_dir) and os.listdir(self.state_dir))

    def test_a_missing_task_stops_the_start_and_is_named(self):
        result = self.start(
            "COLLIERY-T-0001",
            "COLLIERY-T-0002",
            extra=("--missing", "COLLIERY-T-0002"),
        )
        self.assert_refused(result, "COLLIERY-T-0002")

    def test_a_malformed_code_is_refused_and_named(self):
        self.assert_refused(self.start("COLLIERY-I-0001"), "COLLIERY-I-0001")
        self.assert_refused(self.start("t-12"), "t-12")

    def test_no_code_is_refused(self):
        self.assert_refused(self.start(), "short code")

    def test_an_unknown_option_is_refused_and_named(self):
        self.assert_refused(
            self.start("COLLIERY-T-0001", extra=("--max-iteration", "2")),
            "--max-iteration",
        )

    def test_a_bad_iteration_limit_is_refused(self):
        self.assert_refused(
            self.start("COLLIERY-T-0001", extra=("--max-iterations", "two")),
            "--max-iterations",
        )

    def test_no_session_is_refused(self):
        result = self.run_script("start", "COLLIERY-T-0001")
        self.assert_refused(result, "session")

    def test_a_second_loop_in_the_same_session_is_refused(self):
        self.start("COLLIERY-T-0001")
        result = self.start("COLLIERY-T-0002")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("cancel-ralph", result.stderr)
        self.assertEqual(self.state()["tasks"], ["COLLIERY-T-0001"])


class StopHook(RalphCase):
    def test_no_loop_lets_the_session_stop(self):
        result = self.stop()
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout.strip(), "")

    def test_the_hook_blocks_and_gives_the_next_step(self):
        self.start("COLLIERY-T-0001", "COLLIERY-T-0002")
        result = self.stop("COLLIERY-T-0001 is Completed.")
        self.assertEqual(result.returncode, 0, result.stderr)
        output = json.loads(result.stdout)
        self.assertEqual(output["decision"], "block")
        reason = output["reason"]
        for text in (
            "COLLIERY-T-0001",
            "COLLIERY-T-0002",
            "/kairos:implement",
            "<promise>ALL TASKS COMPLETE</promise>",
            "<promise>LOOP STOPPED</promise>",
            "commit rules",
        ):
            self.assertIn(text, reason)
        self.assertIn("iteration 2", output["systemMessage"])
        self.assertEqual(self.state()["iteration"], 2)

    def test_the_hook_reads_the_transcript_when_the_field_is_absent(self):
        self.start("COLLIERY-T-0001")
        result = self.stop("<promise>ALL TASKS COMPLETE</promise>", use_field=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("block", result.stdout)
        self.assertFalse(os.path.exists(self.state_path()))

    def test_the_completion_promise_ends_the_loop(self):
        self.start("COLLIERY-T-0001", "COLLIERY-T-0002")
        self.stop("COLLIERY-T-0001 is Completed.")
        result = self.stop(
            "COLLIERY-T-0002 is Completed.\n\n<promise>ALL TASKS COMPLETE</promise>"
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn('"block"', result.stdout)
        self.assertFalse(os.path.exists(self.state_path()))

    def test_the_stop_promise_ends_the_loop(self):
        self.start("COLLIERY-T-0001")
        result = self.stop(
            "COLLIERY-T-0001 needs a design decision.\n"
            "<promise>LOOP STOPPED</promise>"
        )
        self.assertNotIn('"block"', result.stdout)
        self.assertFalse(os.path.exists(self.state_path()))

    def test_a_quoted_promise_in_the_middle_does_not_end_the_loop(self):
        self.start("COLLIERY-T-0001")
        result = self.stop("I will write <promise>SOMETHING ELSE</promise> later.")
        self.assertEqual(json.loads(result.stdout)["decision"], "block")

    def test_the_iteration_limit_ends_the_loop(self):
        self.start("COLLIERY-T-0001", extra=("--max-iterations", "2"))
        first = self.stop()
        self.assertEqual(json.loads(first.stdout)["decision"], "block")
        second = self.stop()
        self.assertEqual(second.returncode, 0)
        self.assertNotIn('"block"', second.stdout)
        self.assertIn("2", json.loads(second.stdout)["systemMessage"])
        self.assertFalse(os.path.exists(self.state_path()))

    def test_a_loop_of_a_different_session_is_not_touched(self):
        self.start("COLLIERY-T-0001")
        result = self.stop(session="other-session")
        self.assertEqual(result.stdout.strip(), "")
        self.assertEqual(self.state()["iteration"], 1)

    def test_a_corrupt_state_lets_the_session_stop_and_is_removed(self):
        os.makedirs(self.state_dir)
        with open(self.state_path(), "w", encoding="utf-8") as f:
            f.write("{not json")
        result = self.stop()
        self.assertEqual(result.returncode, 0)
        self.assertNotIn('"block"', result.stdout)
        self.assertFalse(os.path.exists(self.state_path()))

    def test_bad_hook_input_lets_the_session_stop(self):
        result = self.run_script("stop", stdin="not json")
        self.assertEqual(result.returncode, 0)
        self.assertEqual(result.stdout.strip(), "")


class Cancel(RalphCase):
    def test_cancel_removes_the_state_and_the_next_stop_is_not_blocked(self):
        self.start("COLLIERY-T-0001")
        result = self.run_script("cancel", "--session", SESSION)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("COLLIERY-T-0001", result.stdout)
        self.assertIn("iteration 1", result.stdout)
        self.assertFalse(os.path.exists(self.state_path()))
        self.assertEqual(self.stop().stdout.strip(), "")

    def test_cancel_with_no_loop_says_so(self):
        result = self.run_script("cancel", "--session", SESSION)
        self.assertEqual(result.returncode, 0)
        self.assertIn("No Ralph loop", result.stdout)

    def test_cancel_refuses_an_unknown_argument(self):
        result = self.run_script("cancel", "--session", SESSION, "--all")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("--all", result.stderr)


def task(code, column="Todo", blockers=()):
    return {
        "code": code,
        "column": column,
        "blockers": [{"code": c, "done": d} for c, d in blockers],
    }


class InitiativeOrder(unittest.TestCase):
    def test_a_blocker_goes_first(self):
        order, skipped = ralph.order_initiative_tasks(
            [
                task("X-T-0002", blockers=[("X-T-0003", False)]),
                task("X-T-0003"),
                task("X-T-0001"),
            ]
        )
        self.assertEqual(order, ["X-T-0003", "X-T-0002", "X-T-0001"])
        self.assertEqual(skipped, [])

    def test_the_given_order_holds_when_no_edge_decides(self):
        order, _ = ralph.order_initiative_tasks(
            [task("X-T-0003"), task("X-T-0001"), task("X-T-0002")]
        )
        self.assertEqual(order, ["X-T-0003", "X-T-0001", "X-T-0002"])

    def test_a_chain_runs_in_blocks_order(self):
        order, _ = ralph.order_initiative_tasks(
            [
                task("X-T-0001", blockers=[("X-T-0002", False)]),
                task("X-T-0002", blockers=[("X-T-0003", False)]),
                task("X-T-0003"),
            ]
        )
        self.assertEqual(order, ["X-T-0003", "X-T-0002", "X-T-0001"])

    def test_a_done_blocker_outside_the_list_is_resolved(self):
        order, skipped = ralph.order_initiative_tasks(
            [task("X-T-0001", blockers=[("Y-T-0009", True)])]
        )
        self.assertEqual(order, ["X-T-0001"])
        self.assertEqual(skipped, [])

    def test_a_task_whose_blocker_is_not_done_is_skipped(self):
        order, skipped = ralph.order_initiative_tasks(
            [
                task("X-T-0001", blockers=[("Y-T-0009", False)]),
                task("X-T-0002", blockers=[("X-T-0001", False)]),
                task("X-T-0003"),
            ]
        )
        self.assertEqual(order, ["X-T-0003"])
        self.assertEqual([s["code"] for s in skipped], ["X-T-0001", "X-T-0002"])
        self.assertIn("Y-T-0009", skipped[0]["reason"])
        self.assertIn("X-T-0001", skipped[1]["reason"])

    def test_a_blocked_task_is_skipped_with_its_dependents(self):
        order, skipped = ralph.order_initiative_tasks(
            [
                task("X-T-0001", column="Blocked"),
                task("X-T-0002", blockers=[("X-T-0001", False)]),
            ]
        )
        self.assertEqual(order, [])
        self.assertEqual([s["code"] for s in skipped], ["X-T-0001", "X-T-0002"])
        self.assertIn("Blocked", skipped[0]["reason"])

    def test_a_cycle_is_skipped(self):
        order, skipped = ralph.order_initiative_tasks(
            [
                task("X-T-0001", blockers=[("X-T-0002", False)]),
                task("X-T-0002", blockers=[("X-T-0001", False)]),
                task("X-T-0003"),
            ]
        )
        self.assertEqual(order, ["X-T-0003"])
        self.assertEqual({s["code"] for s in skipped}, {"X-T-0001", "X-T-0002"})
        self.assertIn("cycle", skipped[0]["reason"])


class StartWithAnInitiative(RalphCase):
    def plan(self, tasks):
        return json.dumps({"tasks": tasks})

    def test_the_initiative_writes_the_tasks_in_blocks_order(self):
        result = self.run_script(
            "start-initiative",
            "--session",
            SESSION,
            "COLLIERY-I-0007",
            stdin=self.plan(
                [
                    task("COLLIERY-T-0002", blockers=[("COLLIERY-T-0001", False)]),
                    task("COLLIERY-T-0003", blockers=[("COLLIERY-T-0099", False)]),
                    task("COLLIERY-T-0001"),
                ]
            ),
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        state = self.state()
        self.assertEqual(state["mode"], "initiative")
        self.assertEqual(state["initiative"], "COLLIERY-I-0007")
        self.assertEqual(state["tasks"], ["COLLIERY-T-0001", "COLLIERY-T-0002"])
        self.assertEqual(state["skipped"][0]["code"], "COLLIERY-T-0003")
        self.assertIn("COLLIERY-T-0003", result.stdout)
        reason = json.loads(self.stop().stdout)["reason"]
        self.assertIn("COLLIERY-I-0007", reason)
        self.assertIn("COLLIERY-T-0003", reason)

    def test_an_initiative_with_no_runnable_task_does_not_start(self):
        result = self.run_script(
            "start-initiative",
            "--session",
            SESSION,
            "COLLIERY-I-0007",
            stdin=self.plan([task("COLLIERY-T-0001", column="Blocked")]),
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("COLLIERY-T-0001", result.stderr)
        self.assertFalse(os.path.exists(self.state_path()))

    def test_a_task_code_in_place_of_the_initiative_is_refused(self):
        result = self.run_script(
            "start-initiative",
            "--session",
            SESSION,
            "COLLIERY-T-0001",
            stdin=self.plan([task("COLLIERY-T-0002")]),
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("COLLIERY-T-0001", result.stderr)

    def test_an_unknown_field_in_the_plan_is_refused_and_named(self):
        bad = task("COLLIERY-T-0001")
        bad["priority"] = "high"
        result = self.run_script(
            "start-initiative",
            "--session",
            SESSION,
            "COLLIERY-I-0007",
            stdin=self.plan([bad]),
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("priority", result.stderr)
        self.assertFalse(os.path.exists(self.state_path()))

    def test_a_plan_that_is_not_json_is_refused(self):
        result = self.run_script(
            "start-initiative", "--session", SESSION, "COLLIERY-I-0007", stdin="nope"
        )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("JSON", result.stderr)


if __name__ == "__main__":
    unittest.main()
