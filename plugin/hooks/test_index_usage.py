#!/usr/bin/env python3
"""Unit tests for `bin/kairos-index-usage` (KAIROS-T-0758): the index usage
of the local sessions from a fixture of transcript lines. Neutral names,
no real session data. Stdlib only; run with
`python3 -m unittest plugin/hooks/test_index_usage.py` (or `angreal test
unit`, which includes it)."""

import os as _os  # noqa: E402
import sys as _sys  # noqa: E402

_sys.path.insert(0, _os.path.dirname(_os.path.abspath(__file__)))
import hook_test_env  # noqa: E402,F401  (KAIROS-T-0712: no live env, a temp HOME)

import datetime
import importlib.machinery
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "..", "bin", "kairos-index-usage")
LOADER = importlib.machinery.SourceFileLoader("kairos_index_usage", SCRIPT)
SPEC = importlib.util.spec_from_loader("kairos_index_usage", LOADER)
usage = importlib.util.module_from_spec(SPEC)
LOADER.exec_module(usage)

NOW = datetime.datetime(2026, 11, 8, 12, 0, tzinfo=datetime.timezone.utc)


def at(days_ago, hour=10):
    return (NOW - datetime.timedelta(days=days_ago)).replace(hour=hour).isoformat().replace("+00:00", "Z")


def tool_use(name, tool_input, use_id, days_ago=1, agent=None, session="s-robin"):
    event = {
        "type": "assistant",
        "timestamp": at(days_ago),
        "sessionId": session,
        "message": {"role": "assistant", "content": [{"type": "tool_use", "id": use_id, "name": name, "input": tool_input}]},
    }
    if agent:
        event["agentId"] = agent
        event["isSidechain"] = True
    return event


def tool_result(use_id, content, is_error=False, days_ago=1, agent=None, session="s-robin"):
    event = {
        "type": "user",
        "timestamp": at(days_ago),
        "sessionId": session,
        "message": {
            "role": "user",
            "content": [{"type": "tool_result", "tool_use_id": use_id, "content": content, "is_error": is_error}],
        },
    }
    if agent:
        event["agentId"] = agent
    return event


def prompt(text, days_ago=1, agent=None, session="s-robin"):
    event = {"type": "user", "timestamp": at(days_ago), "sessionId": session, "message": {"role": "user", "content": text}}
    if agent:
        event["agentId"] = agent
    return event


def write_jsonl(path, events):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as handle:
        for event in events:
            handle.write(json.dumps(event) + "\n")
        handle.write("not json\n")


class Fixture(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = os.path.join(self.tmp.name, "claude")
        project = os.path.join(self.root, "projects", "-Users-robin-Boards-ledger")
        # The main session: one index call, two greps (a Bash one too), a
        # read, an edit, a Kairos tool, and an index call outside the window.
        write_jsonl(
            os.path.join(project, "s-robin.jsonl"),
            [
                prompt("Fix the ledger"),
                tool_use("mcp__plugin_kairos_kairos-code__fetch_index", {"root": "/x"}, "t1"),
                tool_result("t1", "The server opened the index."),
                tool_use("Grep", {"pattern": "fn settle", "path": "src"}, "t2"),
                tool_use("Bash", {"command": "rg -n 'fn settle' crates/ | head"}, "t3"),
                tool_use("Bash", {"command": "sed -n 1,40p src/lib.rs"}, "t4"),
                tool_use("Bash", {"command": "cargo test 2>&1 | grep -E 'test result'"}, "t5"),
                tool_use("Read", {"file_path": "src/lib.rs"}, "t6"),
                tool_use("Edit", {"file_path": "src/lib.rs"}, "t7"),
                tool_use("mcp__kairos__append_note", {"short_code": "LEDGER-T-0001"}, "t8"),
                tool_use("Agent", {"prompt": "review"}, "t9"),
                tool_use("mcp__plugin_kairos_kairos-code__code_search", {"query": "old"}, "t0", days_ago=20),
            ],
        )
        # A Workflow harness subagent with an index call that failed.
        write_jsonl(
            os.path.join(project, "s-robin", "subagents", "agent-a1.jsonl"),
            [
                prompt("[Workflow harness — user request] The harness relays, verbatim, ...", agent="a1"),
                tool_use("mcp__plugin_kairos_kairos-code__symbol", {"name": "settle"}, "u1", agent="a1"),
                tool_result("u1", [{"type": "text", "text": "DOWNLOAD: the download of the index of abc failed: 500"}], is_error=True, agent="a1"),
                tool_use("Grep", {"pattern": "fn settle"}, "u2", agent="a1"),
            ],
        )
        # An Agent tool subagent with greps only.
        write_jsonl(
            os.path.join(project, "s-robin", "subagents", "agent-a2.jsonl"),
            [
                prompt("You review a code change for conformance ...", agent="a2"),
                tool_use("Bash", {"command": "grep -rn settle src/"}, "v1", agent="a2"),
                tool_use("Bash", {"command": "cat src/lib.rs"}, "v2", agent="a2"),
            ],
        )
        # A subagent of another project, outside the window.
        write_jsonl(
            os.path.join(self.root, "projects", "-Users-robin-other", "s-old", "subagents", "agent-a3.jsonl"),
            [tool_use("Grep", {"pattern": "x"}, "w1", days_ago=25, agent="a3", session="s-old")],
        )
        # The hook log of KAIROS-T-0757 names the agent type of a1.
        log = os.path.join(self.root, "kairos-index", "ledger-abc", "tool-calls.log")
        os.makedirs(os.path.dirname(log))
        with open(log, "w") as handle:
            handle.write("2026-11-08T10:00:00+00:00 s-robin a1 Explore PreToolUse Grep nudge\n")
            handle.write("2026-11-08T10:00:01+00:00 s-robin - - PreToolUse Bash quiet\n")

    def tearDown(self):
        self.tmp.cleanup()

    def report(self, days=7, projects="*"):
        return usage.build(self.root, days, projects, now=NOW)


class Counts(Fixture):
    def test_the_main_session_counts_each_kind_of_call_in_the_window(self):
        main = self.report()["totals"]["main"]
        self.assertEqual(main["runs"], 1)
        self.assertEqual(main["index"], 1, "the call of 20 days ago is out of the window")
        self.assertEqual(main["index_tools"], {"fetch_index": 1})
        self.assertEqual(main["grep"], 1)
        self.assertEqual(main["bash_search"], 1, "the grep after a pipe is not a search")
        self.assertEqual(main["bash_read"], 1)
        self.assertEqual(main["read"], 1)
        self.assertEqual(main["edit"], 1)
        self.assertEqual(main["work"], 1)
        self.assertEqual(main["agent"], 1)
        self.assertEqual(main["other"], 1, "the cargo test command")
        self.assertEqual(main["runs_with_an_index_call"], 1)

    def test_the_subagents_are_counted_apart_with_their_kind(self):
        report = self.report()
        sub = report["totals"]["subagent"]
        self.assertEqual(sub["runs"], 2)
        self.assertEqual(sub["index"], 1)
        self.assertEqual(sub["runs_with_an_index_call"], 1)
        self.assertEqual(sub["share_with_an_index_call"], 0.5)
        self.assertEqual(sub["bash_search"], 1)
        self.assertEqual(sub["bash_read"], 1)
        self.assertEqual(report["subagent_kinds"]["workflow"], {"runs": 1, "runs_with_an_index_call": 1, "index": 1})
        self.assertEqual(report["subagent_kinds"]["agent"], {"runs": 1, "runs_with_an_index_call": 0, "index": 0})

    def test_the_window_is_the_timestamp_of_the_event(self):
        self.assertEqual(self.report(days=30)["totals"]["main"]["index"], 2)
        self.assertEqual(self.report(days=30)["totals"]["subagent"]["runs"], 3)
        self.assertEqual(self.report(days=7)["totals"]["subagent"]["runs"], 2)

    def test_the_projects_glob_filters_the_folders(self):
        report = self.report(days=30, projects="*-other")
        self.assertEqual(report["totals"]["main"]["runs"], 0)
        self.assertEqual(report["totals"]["subagent"]["runs"], 1)

    def test_the_top_patterns_and_the_index_errors(self):
        report = self.report()
        self.assertEqual(report["top_patterns"][0], {"pattern": "fn settle", "count": 3})
        self.assertEqual(report["top_patterns"][1], {"pattern": "settle", "count": 1})
        self.assertEqual(len(report["index_errors"]), 1)
        error = report["index_errors"][0]
        self.assertEqual(error["tool"], "symbol")
        self.assertEqual(error["agent_id"], "a1")
        self.assertEqual(error["agent_type"], "Explore", "from the hook log")
        self.assertIn("DOWNLOAD", error["error"])

    def test_the_agent_types_come_from_the_hook_log(self):
        self.assertEqual(self.report()["agent_types"], {"Explore": 1})

    def test_the_kind_of_a_subagent_comes_from_its_first_prompt_before_the_window(self):
        write_jsonl(
            os.path.join(self.root, "projects", "-Users-robin-Boards-ledger", "s-robin", "subagents", "agent-a4.jsonl"),
            [
                prompt("[Workflow harness — user request] ...", days_ago=12, agent="a4"),
                tool_use("Grep", {"pattern": "late"}, "x1", days_ago=2, agent="a4"),
            ],
        )
        report = self.report()
        self.assertEqual(report["subagent_kinds"]["workflow"]["runs"], 2)

    def test_a_run_with_no_tool_call_counts_as_a_run(self):
        write_jsonl(
            os.path.join(self.root, "projects", "-Users-robin-Boards-ledger", "s-quiet.jsonl"),
            [prompt("Hello", session="s-quiet")],
        )
        main = self.report()["totals"]["main"]
        self.assertEqual(main["runs"], 2)
        self.assertEqual(main["share_with_an_index_call"], 0.5)


class Text(Fixture):
    def test_the_table_names_the_totals_and_the_share(self):
        text = usage.table(self.report())
        self.assertIn("main sessions", text)
        self.assertIn("subagent runs", text)
        self.assertIn("share with an index call", text)
        self.assertIn("100%", text)
        self.assertIn("50%", text)
        self.assertIn("Top grep patterns:", text)
        self.assertIn("fn settle", text)
        self.assertIn("Index calls with an error (1):", text)
        self.assertIn("symbol [a1]: DOWNLOAD", text)
        self.assertIn("workflow", text)

    def test_the_report_holds_no_file_content(self):
        text = usage.table(self.report())
        self.assertNotIn("Fix the ledger", text)
        self.assertNotIn("The server opened", text)
        self.assertNotIn("src/lib.rs", text)

    def test_the_command_line_gives_the_table_and_the_json(self):
        env = dict(os.environ, CLAUDE_CONFIG_DIR=self.root)
        result = subprocess.run([sys.executable, SCRIPT, "--days", "30"], capture_output=True, text=True, env=env, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Index usage from", result.stdout)
        result = subprocess.run([sys.executable, SCRIPT, "--json"], capture_output=True, text=True, env=env, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(set(report), {"window", "totals", "subagent_kinds", "agent_types", "top_patterns", "index_errors"})
        result = subprocess.run([sys.executable, SCRIPT, "--days", "0"], capture_output=True, text=True, env=env, timeout=30)
        self.assertEqual(result.returncode, 2)


class Classification(unittest.TestCase):
    def test_bash_commands(self):
        kind = usage.bash_kind
        self.assertEqual(kind("grep -rn 'fn x' src/"), ("search", "fn x"))
        self.assertEqual(kind("rg -t rust -e foo crates"), ("search", "foo"))
        self.assertEqual(kind("git grep -n foo"), ("search", "foo"))
        self.assertEqual(kind("find . -name '*.rs'"), ("search", None))
        self.assertEqual(kind("cargo test 2>&1 | grep -E 'test result'"), (None, None))
        self.assertEqual(kind("grep -E 'a|b' src/ | head"), ("search", "a|b"))
        self.assertEqual(kind("git -C /x grep -n foo -- crates"), ("search", "foo"))
        self.assertEqual(kind("rg --max-count 3 foo src"), ("search", "foo"))
        self.assertEqual(kind("sed -En '1,5p' x"), ("read", None))
        self.assertEqual(kind("sed -n 10,20p src/lib.rs"), ("read", None))
        self.assertEqual(kind("cat -n src/lib.rs"), ("read", None))
        self.assertEqual(kind("sed -i 's/a/b/' x"), (None, None))
        self.assertEqual(kind("cargo build"), (None, None))
        self.assertEqual(kind(None), (None, None))

    def test_tool_names(self):
        self.assertEqual(usage.classify("mcp__plugin_kairos_kairos-code__callers", {}), ("index:callers", None))
        self.assertEqual(usage.classify("mcp__kairos__get_item", {}), ("work", None))
        self.assertEqual(usage.classify("Grep", {"pattern": "x"}), ("grep", "x"))
        self.assertEqual(usage.classify("Write", {}), ("edit", None))
        self.assertEqual(usage.classify("WebFetch", {}), ("other", None))

    def test_subagent_kinds(self):
        self.assertEqual(usage.subagent_kind("[Workflow harness — user request] ..."), "workflow")
        self.assertEqual(usage.subagent_kind("You review a change"), "agent")
        self.assertEqual(usage.subagent_kind(""), "other")
        self.assertEqual(usage.subagent_kind(None), "other")


if __name__ == "__main__":
    unittest.main()
