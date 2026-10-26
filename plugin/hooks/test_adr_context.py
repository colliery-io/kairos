#!/usr/bin/env python3
"""Unit tests for the ADRs of a file before an edit (KAIROS-T-0702): a
PreToolUse hook on Write, Edit and MultiEdit that gives, once per session
per file, the ADRs that the file cites, from `kairos adrs check --json`. It
fails open and never blocks. Stdlib only; run with `python3 -m unittest
plugin/hooks/test_adr_context.py` (or `angreal test unit`, which includes
it).

The tests run `adr_context.py` as a subprocess, the way Claude Code runs
it. A stub `kairos` on PATH answers `adrs check` with a canned report and
logs each call; it refuses each other command, so the hooks read the
wiring file themselves. No test calls a server."""

import importlib.util
import json
import os
import stat
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "adr_context.py")
SPEC = importlib.util.spec_from_file_location("adr_context", SCRIPT)
adr_context = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(adr_context)

SESSION = "session-0001"
WIRING = "---\ndeployment_url: https://kairos.example.com\ntenant: acme\n---\n"

STUB = """#!/bin/sh
# An older kairos with no `config` verb: the hooks read the wiring file.
if [ "$1" = "config" ]; then
  exit 64
fi
echo "$*" >> "$STUB_LOG"
if [ "$1" = "adrs" ] && [ "$2" = "check" ]; then
  cat "$STUB_REPORT"
  exit "${STUB_EXIT:-0}"
fi
exit 1
"""


def state(code, title, status="Decided", **extra):
    value = {
        "code": code,
        "found": True,
        "current_code": code,
        "title": title,
        "status": status,
        "superseded_by": [],
        "amended_by": [],
    }
    value.update(extra)
    return value


REPORT = {
    "source": "deployment",
    "skipped": False,
    "ok": False,
    "summary": {"citations": 3, "adrs": 3, "fail": 1, "warn": 1, "info": 0},
    "citations": [],
    "adrs": [
        state("ACME-A-0001", "Postgres is the store"),
        state(
            "ACME-A-0002",
            "Refunds are async",
            "Superseded",
            superseded_by=[{"code": "ACME-A-0007", "title": "Refunds are sync"}],
            amended_by=[{"code": "ACME-A-0008", "title": "Refund limits"}],
        ),
        {"code": "ACME-A-0003", "found": False, "superseded_by": [], "amended_by": []},
    ],
}


class AdrContextCase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = self.tmp.name
        self.project = os.path.join(root, "project")
        os.makedirs(os.path.join(self.project, ".git"))
        os.makedirs(os.path.join(self.project, ".claude"))
        with open(os.path.join(self.project, ".claude", "kairos.local.md"), "w") as handle:
            handle.write(WIRING)
        self.bin = os.path.join(root, "bin")
        os.makedirs(self.bin)
        stub = os.path.join(self.bin, "kairos")
        with open(stub, "w") as handle:
            handle.write(STUB)
        os.chmod(stub, os.stat(stub).st_mode | stat.S_IXUSR)
        self.log = os.path.join(root, "calls.log")
        self.report = os.path.join(root, "report.json")
        self.set_report(REPORT)
        self.state_dir = os.path.join(root, "adr-state")
        self.env = {
            "PATH": f"{self.bin}:/usr/bin:/bin",
            "HOME": root,
            "CLAUDE_PROJECT_DIR": self.project,
            "KAIROS_ADR_STATE_DIR": self.state_dir,
            "STUB_LOG": self.log,
            "STUB_REPORT": self.report,
        }
        self.write("src/refund.rs", "// ACME-A-0001, ACME-A-0002\n// ACME-A-0003 and ACME-A-0001\n")

    def tearDown(self):
        self.tmp.cleanup()

    def set_report(self, report):
        with open(self.report, "w") as handle:
            handle.write(report if isinstance(report, str) else json.dumps(report))

    def write(self, name, text, folder=None):
        path = os.path.join(folder or self.project, name)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w") as handle:
            handle.write(text)
        return path

    def payload(self, file_path="src/refund.rs", tool_name="Edit", session=SESSION):
        return json.dumps(
            {
                "session_id": session,
                "tool_name": tool_name,
                "tool_input": {"file_path": file_path, "old_string": "a", "new_string": "b"},
            }
        )

    def run_hook(self, stdin, env=None):
        return subprocess.run(
            [sys.executable, "-I", SCRIPT],
            input=stdin,
            capture_output=True,
            text=True,
            env=env or self.env,
            cwd=self.project,
            check=False,
        )

    def context(self, result):
        self.assertEqual(result.returncode, 0, result.stderr)
        if not result.stdout:
            return None
        output = json.loads(result.stdout)["hookSpecificOutput"]
        self.assertEqual(output["hookEventName"], "PreToolUse")
        self.assertNotIn("permissionDecision", output)
        return output["additionalContext"]

    def calls(self):
        try:
            with open(self.log) as handle:
                return [line.strip() for line in handle if line.strip()]
        except OSError:
            return []


class GivesTheAdrs(AdrContextCase):
    def test_the_block_names_each_cited_adr_once(self):
        text = self.context(self.run_hook(self.payload()))
        self.assertIsNotNone(text)
        self.assertIn("ADRs that src/refund.rs cites", text)
        self.assertIn("- ACME-A-0001 [Decided]: Postgres is the store", text)
        self.assertIn("- ACME-A-0002 [Superseded]: Refunds are async", text)
        self.assertIn("Superseded by ACME-A-0007 (Refunds are sync).", text)
        self.assertIn("Amended by ACME-A-0008 (Refund limits).", text)
        self.assertIn("- ACME-A-0003: no ADR has this code.", text)
        self.assertEqual(text.count("ACME-A-0001"), 1)
        calls = self.calls()
        self.assertEqual(len(calls), 1, calls)
        self.assertTrue(calls[0].startswith("adrs check "), calls)
        self.assertTrue(calls[0].endswith("src/refund.rs --json"), calls)

    def test_each_write_tool_fires(self):
        for index, tool in enumerate(("Write", "Edit", "MultiEdit")):
            with self.subTest(tool=tool):
                text = self.context(self.run_hook(self.payload(tool_name=tool, session=f"s-{index}")))
                self.assertIsNotNone(text)

    def test_an_absolute_path_fires(self):
        path = os.path.join(self.project, "src", "refund.rs")
        self.assertIsNotNone(self.context(self.run_hook(self.payload(file_path=path))))

    def test_a_retired_code_and_an_archived_adr_are_named(self):
        self.write("a.md", "OLD-A-0001\n")
        self.set_report(
            {
                "source": "deployment",
                "adrs": [
                    state(
                        "OLD-A-0001",
                        "Old",
                        current_code="ACME-A-0009",
                        archived_at="2026-10-01T00:00:00Z",
                    )
                ],
            }
        )
        text = self.context(self.run_hook(self.payload(file_path="a.md")))
        self.assertIn("- OLD-A-0001 (now ACME-A-0009) [Decided]: Old ARCHIVED.", text)

    def test_the_revisit_condition_is_shown(self):
        self.write("r.md", "ACME-A-0001\n")
        self.set_report(
            {
                "adrs": [
                    state(
                        "ACME-A-0001",
                        "T",
                        revisit_when="a second forge",
                        revisit_on="2026-12-01",
                    )
                ]
            }
        )
        text = self.context(self.run_hook(self.payload(file_path="r.md")))
        self.assertIn("  Revisit when: a second forge (on 2026-12-01).", text)

    def test_a_long_list_is_cut(self):
        codes = [f"ACME-A-{n:04d}" for n in range(1, 16)]
        self.write("many.md", " ".join(codes))
        self.set_report({"adrs": [state(code, "T") for code in codes]})
        text = self.context(self.run_hook(self.payload(file_path="many.md")))
        self.assertIn("- and 3 more.", text)
        self.assertNotIn("ACME-A-0013 [", text)


class OncePerSessionPerFile(AdrContextCase):
    def test_the_second_edit_of_the_file_is_silent(self):
        self.assertIsNotNone(self.context(self.run_hook(self.payload())))
        self.assertIsNone(self.context(self.run_hook(self.payload())))
        self.assertEqual(len(self.calls()), 1)

    def test_another_file_with_known_adrs_needs_no_check(self):
        self.context(self.run_hook(self.payload()))
        self.write("docs/b.md", "ACME-A-0002\n")
        text = self.context(self.run_hook(self.payload(file_path="docs/b.md")))
        self.assertIn("Superseded by ACME-A-0007", text)
        self.assertEqual(len(self.calls()), 1, "the states of the session are the cache")

    def test_a_different_session_fires_again(self):
        self.context(self.run_hook(self.payload()))
        self.assertIsNotNone(self.context(self.run_hook(self.payload(session="session-0002"))))


class FailsOpen(AdrContextCase):
    def assert_silent(self, result):
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "")

    def test_a_file_that_cites_nothing_is_silent_and_runs_no_check(self):
        self.write("plain.rs", "fn main() {} // KAIROS-T-0702 is a task\n")
        self.assert_silent(self.run_hook(self.payload(file_path="plain.rs")))
        self.assertEqual(self.calls(), [])

    def test_a_new_file_is_silent(self):
        self.assert_silent(self.run_hook(self.payload(file_path="new.rs", tool_name="Write")))

    def test_no_kairos_on_path_is_silent(self):
        env = dict(self.env, PATH="/usr/bin:/bin")
        self.assert_silent(self.run_hook(self.payload(), env=env))

    def test_a_file_outside_a_wired_checkout_is_silent(self):
        other = os.path.join(self.tmp.name, "other")
        os.makedirs(os.path.join(other, ".git"))
        path = self.write("a.rs", "ACME-A-0001\n", folder=other)
        self.assert_silent(self.run_hook(self.payload(file_path=path)))
        loose = self.write("loose.md", "ACME-A-0001\n", folder=self.tmp.name)
        self.assert_silent(self.run_hook(self.payload(file_path=loose)))
        self.assertEqual([c for c in self.calls() if c.startswith("adrs ")], [])

    def test_a_failed_or_skipped_check_is_silent_and_is_tried_again(self):
        for report, code in (
            (REPORT, "2"),
            (REPORT, "64"),
            ("not json", "0"),
            ({"skipped": True, "source": "none", "adrs": []}, "0"),
        ):
            with self.subTest(report=str(report)[:20], code=code):
                self.set_report(report)
                env = dict(self.env, STUB_EXIT=code)
                self.assert_silent(self.run_hook(self.payload(), env=env))
        self.set_report(REPORT)
        self.assertIsNotNone(self.context(self.run_hook(self.payload())))

    def test_a_failed_citation_still_gives_the_block(self):
        env = dict(self.env, STUB_EXIT="1")
        self.assertIsNotNone(self.context(self.run_hook(self.payload(), env=env)))

    def test_other_tools_and_bad_input_are_silent(self):
        for stdin in (
            "",
            "not json",
            "[]",
            json.dumps({"tool_name": "Edit"}),
            self.payload(tool_name="Read"),
            self.payload(session="bad session/id"),
        ):
            with self.subTest(stdin=stdin):
                self.assert_silent(self.run_hook(stdin))


class Pure(unittest.TestCase):
    def test_cited_codes_match_the_scanner_of_the_cli(self):
        self.assertEqual(
            adr_context.cited_codes(
                "KAIROS-A-0031, (ACME-A-0002) KAIROS-A-0031 K8S-A-0001 K-A-0031"
            ),
            ["KAIROS-A-0031", "ACME-A-0002", "K8S-A-0001", "K-A-0031"],
        )
        for text in (
            "KAIROS-T-0031",
            "kairos-a-0031",
            "KAIROS-A-031",
            "KAIROS-A-0031x",
            "xKAIROS-A-0031",
            "_KAIROS-A-0031",
            "x-KAIROS-A-0031",
            "1KAIROS-A-0031",
            None,
        ):
            self.assertEqual(adr_context.cited_codes(text), [], text)

    def test_cited_codes_match_the_shared_examples(self):
        # The same examples as the Rust rule (kairos_index::codes_in).
        with open(os.path.join(HERE, "adr_code_examples.json"), encoding="utf-8") as f:
            examples = json.load(f)["examples"]
        self.assertTrue(examples)
        for example in examples:
            text = example["text"]
            self.assertEqual(
                [m.group(1) for m in adr_context.ADR_CODE.finditer(text)],
                example["codes"],
                text,
            )

    def test_refs_text(self):
        self.assertEqual(
            adr_context.refs_text([{"code": "A-A-0001", "title": "T"}, {"code": "A-A-0002"}, "x"]),
            "A-A-0001 (T), A-A-0002",
        )
        self.assertEqual(adr_context.refs_text(None), "")


class Registered(unittest.TestCase):
    def test_the_hook_is_registered_on_the_write_tools(self):
        import re

        with open(os.path.join(HERE, "hooks.json"), encoding="utf-8") as handle:
            hooks = json.load(handle)
        entries = [
            entry
            for entry in hooks["hooks"]["PreToolUse"]
            if any("adr_context.py" in hook["command"] for hook in entry["hooks"])
        ]
        self.assertEqual(len(entries), 1)
        matcher = re.compile(entries[0]["matcher"])
        for name in ("Write", "Edit", "MultiEdit"):
            self.assertTrue(matcher.search(name), name)
        for name in ("Read", "Bash", "NotebookEdit"):
            self.assertFalse(matcher.search(name), name)
        timeout = [hook for hook in entries[0]["hooks"] if "adr_context.py" in hook["command"]]
        self.assertGreater(timeout[0]["timeout"], adr_context.CHECK_SECONDS)


if __name__ == "__main__":
    unittest.main()
