#!/usr/bin/env python3
"""Unit tests for the once-per-session rules hook (KAIROS-T-0498,
COLLIERY-I-0631): on the first Kairos tool call of a session it gives the
Kairos rules as additional context, once. Stdlib only; run with
`python3 -m unittest plugin/hooks/test_kairos_rules.py` (or `angreal test
unit`, which includes it).

The tests run `kairos_rules.py` as a subprocess, the way Claude Code runs
it, with KAIROS_RULES_STATE_DIR and CLAUDE_PROJECT_DIR pointing at a
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
SCRIPT = os.path.join(HERE, "kairos_rules.py")
SPEC = importlib.util.spec_from_file_location("kairos_rules", SCRIPT)
kairos_rules = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(kairos_rules)

SESSION = "session-0001"
URL = "https://kairos.example.com"

WIRING = (
    "---\n"
    "deployment_url: {url}\n"
    "tenant: acme\n"
    "repository: payments-api\n"
    "team_board: platform-delivery\n"
    "adr_board: platform-adrs\n"
    "---\n\n# Kairos wiring\n"
)


def payload(tool_name, tool_input=None, session=SESSION):
    return json.dumps(
        {"session_id": session, "tool_name": tool_name, "tool_input": tool_input or {}}
    )


class KairosRulesCase(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.project = os.path.join(self.tmp.name, "project")
        os.makedirs(os.path.join(self.project, ".claude"))
        self.state_dir = os.path.join(self.tmp.name, "rules")
        self.env = dict(
            os.environ,
            KAIROS_RULES_STATE_DIR=self.state_dir,
            CLAUDE_PROJECT_DIR=self.project,
        )

    def tearDown(self):
        self.tmp.cleanup()

    def wire(self, url=URL):
        with open(os.path.join(self.project, ".claude", "kairos.local.md"), "w") as handle:
            handle.write(WIRING.format(url=url))

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
        self.assertNotIn("permissionDecision", output)
        return output["additionalContext"]


class FiresOnce(KairosRulesCase):
    def test_the_first_kairos_tool_call_gets_the_rules_and_the_second_does_not(self):
        self.wire()
        first = self.context(self.run_hook(payload("mcp__plugin_kairos_kairos__get_item")))
        self.assertIsNotNone(first)
        self.assertIn("Kairos rules", first)
        second = self.context(self.run_hook(payload("mcp__plugin_kairos_kairos__search")))
        self.assertIsNone(second)

    def test_a_different_session_gets_the_rules_again(self):
        self.wire()
        self.context(self.run_hook(payload("mcp__kairos__get_item")))
        other = self.context(self.run_hook(payload("mcp__kairos__get_item", session="session-0002")))
        self.assertIsNotNone(other)

    def test_a_version_bump_fires_again(self):
        self.wire()
        self.context(self.run_hook(payload("mcp__plugin_kairos_kairos__get_item")))
        env = dict(self.env, KAIROS_RULES_VERSION_OVERRIDE=str(kairos_rules.RULES_VERSION + 1))
        again = self.context(self.run_hook(payload("mcp__plugin_kairos_kairos__get_item"), env=env))
        self.assertIsNotNone(again)

    def test_the_marker_is_keyed_by_session_and_version(self):
        self.wire()
        self.context(self.run_hook(payload("mcp__plugin_kairos_kairos__get_item")))
        self.assertTrue(
            os.path.isfile(os.path.join(self.state_dir, f"{SESSION}-{kairos_rules.RULES_VERSION}"))
        )


class WhichToolsFire(KairosRulesCase):
    def test_each_kairos_tool_shape_fires(self):
        shapes = (
            payload("mcp__plugin_kairos_kairos__get_item"),
            payload("mcp__kairos__board_items"),
            payload("mcp__plugin_kairos_kairos-code__code_search"),
            payload("Skill", {"skill": "kairos:implement"}),
            payload("Skill", {"skill": "kairos-vocabulary"}),
            payload("Bash", {"command": "kairos item get ACME-T-0001"}),
            payload("Bash", {"command": "env -u KAIROS_URL kairos --version"}),
            payload("Bash", {"command": "git status && kairos index update"}),
        )
        for index, shape in enumerate(shapes):
            with self.subTest(shape=shape):
                self.wire()
                context = self.context(self.run_hook(shape.replace(SESSION, f"s-{index}")))
                self.assertIsNotNone(context)

    def test_other_tools_do_not_fire(self):
        self.wire()
        shapes = (
            payload("Read", {"file_path": "/x"}),
            payload("mcp__github__get_pr"),
            payload("Skill", {"skill": "tdd"}),
            payload("Bash", {"command": "git status"}),
            payload("Bash", {"command": "echo kairos"}),
            payload("Bash", {"command": "grep kairos README.md"}),
        )
        for shape in shapes:
            with self.subTest(shape=shape):
                result = self.run_hook(shape)
                self.assertEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")
        self.assertFalse(os.path.exists(self.state_dir))

    def test_bad_input_gives_no_output_and_exit_0(self):
        for stdin in ("", "not json", "[]", json.dumps({"tool_name": "Skill"})):
            with self.subTest(stdin=stdin):
                result = self.run_hook(stdin)
                self.assertEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")


class TheRulesText(KairosRulesCase):
    def test_with_a_wired_url_every_short_code_is_a_link(self):
        self.wire()
        text = self.context(self.run_hook(payload("mcp__plugin_kairos_kairos__get_item")))
        self.assertIn(f"{URL}/items/COLLIERY-I-0627", text)
        self.assertIn(f"{URL}/items/COLLIERY-I-0628", text)
        self.assertIn(f"`{URL}/items/<CODE>`", text)
        self.assertNotRegex(text, r"(?<![/\w-])COLLIERY-I-06")

    def test_with_no_wiring_the_codes_stay_bare(self):
        text = self.context(self.run_hook(payload("mcp__plugin_kairos_kairos__get_item")))
        self.assertIsNotNone(text)
        self.assertIn("COLLIERY-I-0627", text)
        self.assertNotIn("/items/", text)
        self.assertIn("bare short code", text)

    def test_the_rules_name_each_line_of_the_initiatives(self):
        self.wire()
        text = self.context(self.run_hook(payload("mcp__plugin_kairos_kairos__get_item")))
        for phrase in (
            "Backlog, Todo, Active, Completed",
            "Ground before you plan",
            "ADR",
            # KAIROS-T-0599: the two routes to the ADRs and the documents.
            "`get_repository <slug>`",
            "the ADRs of the owner team",
            "`team_pages <team>`",
            "local memory",
            '"not checked"',
        ):
            self.assertIn(phrase, text)
        self.assertNotIn("KAIROS_MCP_KEY", text)
        self.assertLess(len(text), 2000)

    def test_linkify_is_pure(self):
        self.assertEqual(
            kairos_rules.linkify("see ACME-T-0042.", URL), f"see {URL}/items/ACME-T-0042."
        )
        self.assertEqual(kairos_rules.linkify("see ACME-T-0042.", ""), "see ACME-T-0042.")
        already = f"{URL}/items/ACME-T-0042"
        self.assertEqual(kairos_rules.linkify(already, URL), already)


class Registered(unittest.TestCase):
    def test_the_hook_is_registered_for_the_kairos_tools_skill_and_bash(self):
        with open(os.path.join(HERE, "hooks.json"), encoding="utf-8") as handle:
            hooks = json.load(handle)
        import re

        entries = [
            entry
            for entry in hooks["hooks"]["PreToolUse"]
            if any("kairos_rules.py" in hook["command"] for hook in entry["hooks"])
        ]
        self.assertEqual(len(entries), 1)
        matcher = re.compile(entries[0]["matcher"])
        for name in ("mcp__plugin_kairos_kairos__get_item", "mcp__kairos__search", "Skill", "Bash"):
            self.assertTrue(matcher.search(name), name)
        self.assertFalse(matcher.search("Read"))


if __name__ == "__main__":
    unittest.main()
