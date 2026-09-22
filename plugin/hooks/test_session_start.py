#!/usr/bin/env python3
"""Unit tests for the SessionStart hook's pure parts (KAIROS-T-0108):
frontmatter parsing and the repo-scoped vs board-scoped hint. Stdlib only;
run with `python3 -m unittest plugin/hooks/test_session_start.py` (or
`angreal test unit`, which includes it)."""

import importlib.util
import os
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SPEC = importlib.util.spec_from_file_location(
    "session_start", os.path.join(HERE, "session_start.py")
)
session_start = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(session_start)


def write(text):
    handle = tempfile.NamedTemporaryFile("w", suffix=".md", delete=False)
    handle.write(text)
    handle.close()
    return handle.name


class ReadFrontmatter(unittest.TestCase):
    def test_reads_every_known_key_including_repository(self):
        path = write(
            "---\n"
            "deployment_url: https://acme.kairos.example\n"
            "tenant: acme\n"
            "repository: payments-api\n"
            "delivery_stream: platform\n"
            "team_board: platform-delivery\n"
            "initiative_board: initiatives\n"
            "---\n\n# prose\n"
        )
        values = session_start.read_frontmatter(path)
        self.assertEqual(values["repository"], "payments-api")
        self.assertEqual(values["team_board"], "platform-delivery")
        self.assertEqual(set(values), set(session_start.FRONTMATTER_KEYS))

    def test_empty_values_and_unknown_keys_are_dropped(self):
        path = write(
            "---\nrepository:\nteam_board: web-delivery\nunknown: x\n---\n"
        )
        values = session_start.read_frontmatter(path)
        self.assertNotIn("repository", values)
        self.assertNotIn("unknown", values)
        self.assertEqual(values["team_board"], "web-delivery")


class LiveStateHint(unittest.TestCase):
    def test_repo_scoped_when_repository_is_set(self):
        hint = session_start.live_state_hint(
            {"repository": "payments-api", "team_board": "platform-delivery"}
        )
        self.assertIn("repository `payments-api`", hint)
        self.assertIn("`get_repository`", hint)
        self.assertIn("`repository=payments-api`", hint)
        self.assertIn("platform-delivery", hint)
        self.assertIn("ANOTHER repository", hint)
        self.assertNotIn("re-run /kairos:bootstrap to detect", hint)

    def test_board_scoped_when_repository_is_unset(self):
        hint = session_start.live_state_hint({"team_board": "platform-delivery"})
        self.assertIn("`my_boards`", hint)
        self.assertIn("platform-delivery", hint)
        self.assertIn("No `repository` is wired", hint)
        self.assertNotIn("get_repository", hint)


class BuildContext(unittest.TestCase):
    def test_lists_every_key_and_adds_the_hint_when_reachable(self):
        text = session_start.build_context(
            {"deployment_url": "http://localhost:41080", "repository": "kairos"},
            "deployment reachable",
        )
        for key in session_start.FRONTMATTER_KEYS:
            self.assertIn(f"- {key}:", text)
        self.assertIn("- repository: kairos", text)
        self.assertIn("- tenant: (not set)", text)
        self.assertIn("`get_repository` for `kairos`", text)

    def test_no_hint_when_offline(self):
        text = session_start.build_context(
            {"deployment_url": "http://localhost:41080", "repository": "kairos"},
            "deployment NOT reachable right now (offline?)",
        )
        self.assertIn("- status: deployment NOT reachable", text)
        self.assertNotIn("get_repository", text)


if __name__ == "__main__":
    unittest.main()
