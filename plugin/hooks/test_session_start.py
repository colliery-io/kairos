#!/usr/bin/env python3
"""Unit tests for the SessionStart hook's pure parts (KAIROS-T-0108):
frontmatter parsing and the repo-scoped vs board-scoped hint. Stdlib only;
run with `python3 -m unittest plugin/hooks/test_session_start.py` (or
`angreal test unit`, which includes it)."""

import importlib.util
import os
import tempfile
import unittest
import unittest.mock

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
            "adr_board: platform-adrs\n"
            "---\n\n# prose\n"
        )
        values = session_start.read_frontmatter(path)
        self.assertEqual(values["repository"], "payments-api")
        self.assertEqual(values["adr_board"], "platform-adrs")
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
    def test_an_adr_goes_on_the_adr_board_of_the_team(self):
        # COLLIERY-T-3102: a team can have an ADR board of its own.
        hint = session_start.live_state_hint(
            {
                "repository": "payments-api",
                "team_board": "platform-delivery",
                "adr_board": "platform-adrs",
            }
        )
        self.assertIn("`board=platform-adrs`", hint)
        self.assertIn("ADR board of your team", hint)

    def repo_hint(self):
        return session_start.live_state_hint(
            {"repository": "payments-api", "team_board": "platform-delivery"}
        )

    def test_repo_scoped_when_repository_is_set(self):
        hint = session_start.live_state_hint(
            {"repository": "payments-api", "team_board": "platform-delivery"}
        )
        self.assertIn("repository `payments-api`", hint)
        self.assertIn("`get_repository`", hint)
        self.assertIn("`repository=payments-api`", hint)
        self.assertIn("platform-delivery", hint)
        self.assertNotIn("re-run /kairos:bootstrap to detect", hint)

    def test_the_agent_reads_the_documents_that_impact_the_repository(self):
        # COLLIERY-T-0269: the vision of a repository is a document that
        # impacts it, and `get_repository` lists it.
        hint = self.repo_hint()
        self.assertIn(
            "the documents and the ADRs that impact this repository", hint
        )
        self.assertIn("the other ADRs of the owner team", hint)  # KAIROS-T-0599
        self.assertIn("Read them with `get_item` before you plan work.", hint)
        self.assertIn("Then call `board_items`", hint)

    def test_the_queue_is_the_board_of_the_team(self):
        hint = self.repo_hint()
        self.assertIn("the board of your team (platform-delivery)", hint)
        self.assertIn("That is your queue.", hint)

    def test_the_work_of_other_teams_is_found_with_search(self):
        hint = self.repo_hint()
        self.assertIn(
            "The work of other teams in this repository is not in your queue",
            hint,
        )
        self.assertIn("`search`", hint)

    def test_a_create_names_a_board_or_a_team(self):
        hint = self.repo_hint()
        self.assertIn("name a board in `board`", hint)
        self.assertIn("also accept a team", hint)
        self.assertIn("The board decides the team.", hint)

    def test_the_repository_is_a_link_and_chooses_no_board(self):
        hint = self.repo_hint()
        self.assertIn("The repository is an optional link", hint)
        self.assertIn("does not choose the board", hint)
        # COLLIERY-A-0023: the old rule must not come back in any wording.
        for old_rule in ("routes", "routed", "lands", "Backlog", "ANOTHER repository"):
            self.assertNotIn(old_rule, hint)

    def test_work_for_a_different_team_is_a_request(self):
        hint = self.repo_hint()
        self.assertIn("Work for a different team is a request", hint)
        self.assertIn("the delivery board of that team", hint)
        self.assertIn("entry column", hint)
        self.assertIn("support lane", hint)

    def test_the_hook_carries_no_credentials(self):
        self.assertIn("carries no credentials", self.repo_hint())

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


class CliNote(unittest.TestCase):
    """KAIROS-T-0432: the hook says when the CLI is behind the server."""

    def test_an_older_cli_gets_a_line(self):
        note = session_start.cli_note("0.8.0", "0.12.0")
        self.assertIn("0.8.0", note)
        self.assertIn("0.12.0", note)
        self.assertIn("kairos self-update", note)

    def test_the_line_says_kairos_code_needs_a_restart(self):
        # KAIROS-T-0526: the code tools run the old binary until a restart.
        note = session_start.cli_note("0.8.0", "0.12.0")
        self.assertIn("kairos-code", note)
        self.assertIn("restart the session", note)

    def test_versions_compare_by_number(self):
        self.assertIsNotNone(session_start.cli_note("0.9.0", "0.12.0"))
        self.assertIsNone(session_start.cli_note("0.12.0", "0.12.0"))
        self.assertIsNone(session_start.cli_note("0.13.0", "0.12.0"))

    def test_no_line_without_both_versions(self):
        self.assertIsNone(session_start.cli_note(None, "0.12.0"))
        self.assertIsNone(session_start.cli_note("0.12.0", None))

    def test_an_older_cli_than_the_plugin_gets_a_line(self):
        """KAIROS-T-0513: the plugin's code tools need the CLI at its version."""
        note = session_start.cli_note("0.13.0", None, "0.15.0")
        self.assertIn("kairos CLI is 0.13.0", note)
        self.assertIn("plugin is 0.15.0", note)
        self.assertIn("kairos self-update", note)
        self.assertIsNone(session_start.cli_note("0.15.0", None, "0.15.0"))
        self.assertIsNone(session_start.cli_note("0.16.0", None, "0.15.0"))
        self.assertIsNone(session_start.cli_note("dev", None, "0.15.0"))

    def test_server_and_plugin_both_newer_is_one_line(self):
        note = session_start.cli_note("0.13.0", "0.15.0", "0.15.0")
        self.assertEqual(note.count("\n"), 0)
        self.assertIn("server is 0.15.0", note)
        self.assertIn("plugin is 0.15.0", note)
        self.assertEqual(note.count("kairos self-update"), 1)

    def test_the_plugin_version_is_read_from_the_manifest(self):
        with tempfile.TemporaryDirectory() as root:
            os.makedirs(os.path.join(root, ".claude-plugin"))
            with open(os.path.join(root, ".claude-plugin", "plugin.json"), "w") as f:
                f.write('{"name": "kairos", "version": "0.15.0"}')
            with unittest.mock.patch.dict(os.environ, {"CLAUDE_PLUGIN_ROOT": root}):
                self.assertEqual(session_start.plugin_version(), "0.15.0")
            with unittest.mock.patch.dict(os.environ, {"CLAUDE_PLUGIN_ROOT": root + "/x"}):
                self.assertIsNone(session_start.plugin_version())
        with unittest.mock.patch.dict(os.environ, {}, clear=False):
            os.environ.pop("CLAUDE_PLUGIN_ROOT", None)
            self.assertRegex(session_start.plugin_version(), r"^\d+\.\d+\.\d+")

    def test_the_line_is_in_the_context(self):
        note = session_start.cli_note("0.8.0", "0.12.0")
        text = session_start.build_context(
            {"deployment_url": "http://localhost:41080"}, "deployment reachable", note
        )
        self.assertIn(note, text)


if __name__ == "__main__":
    unittest.main()


class RulesAtTheStart(unittest.TestCase):
    """KAIROS-T-0525: a wired session gets the Kairos rules at its start,
    the marker keeps the PreToolUse hook silent after, and the SessionStart
    hooks of the wiring run on startup and clear, not on a compaction."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.project = os.path.join(self.tmp.name, "project")
        os.makedirs(os.path.join(self.project, ".claude"))
        self.rules_dir = os.path.join(self.tmp.name, "rules")
        with open(os.path.join(self.project, ".claude", "kairos.local.md"), "w") as handle:
            handle.write(
                "---\ndeployment_url: http://127.0.0.1:9\ntenant: acme\n"
                "repository: payments-api\nteam_board: platform\n---\n"
            )
        self.env = dict(
            os.environ,
            CLAUDE_PROJECT_DIR=self.project,
            KAIROS_RULES_STATE_DIR=self.rules_dir,
            PATH="/nonexistent",  # no kairos on PATH: no CLI probe
        )

    def tearDown(self):
        self.tmp.cleanup()

    def run_hook(self, session="session-0525"):
        import json
        import subprocess
        import sys

        result = subprocess.run(
            [sys.executable, os.path.join(HERE, "session_start.py")],
            input=json.dumps({"session_id": session, "hook_event_name": "SessionStart", "source": "startup"}),
            capture_output=True,
            text=True,
            env=self.env,
            cwd=self.project,
            check=False,
            timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return json.loads(result.stdout)["hookSpecificOutput"]["additionalContext"]

    def test_the_rules_come_with_the_wiring_and_the_marker_is_written(self):
        context = self.run_hook()
        self.assertIn("Kairos wiring for this repo", context)
        self.assertIn("Kairos rules (once per session", context)
        self.assertIn("http://127.0.0.1:9/items/COLLIERY-I-0627", context)
        self.assertTrue(
            any(name.startswith("session-0525-") for name in os.listdir(self.rules_dir)),
            "the marker of the session is written",
        )
        # The second start of the same session (a resume) gives the wiring only.
        context = self.run_hook()
        self.assertIn("Kairos wiring for this repo", context)
        self.assertNotIn("Kairos rules (once per session", context)

    def test_the_budget_is_shared_and_never_below_the_floor(self):
        clock = [100.0]
        budget = session_start.Budget(seconds=3.0, floor=0.2, clock=lambda: clock[0])
        self.assertAlmostEqual(budget.left(), 3.0)
        clock[0] = 102.5
        self.assertAlmostEqual(budget.left(), 0.5)
        clock[0] = 110.0
        self.assertAlmostEqual(budget.left(), 0.2)

    def test_the_wiring_hooks_run_on_startup_and_clear_only(self):
        import json

        with open(os.path.join(os.path.dirname(HERE), "hooks", "hooks.json")) as handle:
            hooks = json.load(handle)["hooks"]["SessionStart"]
        by_matcher = {entry["matcher"]: [hook["command"] for hook in entry["hooks"]] for entry in hooks}
        self.assertIn("startup|clear", by_matcher)
        self.assertTrue(any("session_start.py" in command for command in by_matcher["startup|clear"]))
        self.assertTrue(any("index_update.py" in command for command in by_matcher["startup|clear"]))
        self.assertTrue(any("active_work.py" in command for command in by_matcher["compact"]))
        self.assertNotIn("*", by_matcher)


def git(*args, cwd):
    import subprocess

    subprocess.run(
        ["git", "-c", "user.name=Robin", "-c", "user.email=robin@example.com", *args],
        cwd=cwd,
        check=True,
        capture_output=True,
        env=dict(os.environ, GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1"),
    )


def make_clone(parent, name="kairos", origin=None):
    """A git checkout `parent/name` with one commit; with `origin`, that URL
    as its remote `origin`."""
    clone = os.path.join(parent, name)
    os.makedirs(clone)
    git("init", "-q", "-b", "main", cwd=clone)
    git("commit", "-q", "--allow-empty", "-m", "first", cwd=clone)
    if origin:
        git("remote", "add", "origin", origin, cwd=clone)
    return clone


class WorktreeWiring(unittest.TestCase):
    """KAIROS-T-0520: `.claude/kairos.local.md` is git-excluded, so a
    worktree has none. Each hook reads the wiring of the clone through
    `git rev-parse --git-common-dir`."""

    WIRING = (
        "---\ndeployment_url: http://127.0.0.1:9\ntenant: acme\n"
        "repository: payments-api\nteam_board: platform\nadr_board: platform-adrs\n---\n"
    )

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        board = os.path.realpath(self.tmp.name)
        self.clone = make_clone(board)
        os.makedirs(os.path.join(self.clone, ".claude"))
        self.wiring = os.path.join(self.clone, ".claude", "kairos.local.md")
        with open(self.wiring, "w") as handle:
            handle.write(self.WIRING)
        self.worktree = os.path.join(board, "wt-acme-t-0001")
        git("worktree", "add", "-q", "-b", "acme-t-0001", self.worktree, cwd=self.clone)

    def tearDown(self):
        self.tmp.cleanup()

    def test_the_worktree_has_no_wiring_of_its_own(self):
        self.assertFalse(
            os.path.exists(os.path.join(self.worktree, ".claude", "kairos.local.md"))
        )

    def test_the_clone_reads_its_own_file(self):
        self.assertEqual(session_start.wiring_path(self.clone), self.wiring)

    def test_a_worktree_reads_the_file_of_its_clone(self):
        self.assertEqual(session_start.wiring_path(self.worktree), self.wiring)

    def test_a_subfolder_of_a_worktree_is_not_a_checkout_root(self):
        # The hooks run with CLAUDE_PROJECT_DIR, the root of the session.
        os.makedirs(os.path.join(self.worktree, "src"))
        self.assertIsNone(session_start.wiring_path(os.path.join(self.worktree, "src")))

    def test_a_file_in_the_worktree_comes_first(self):
        os.makedirs(os.path.join(self.worktree, ".claude"))
        own = os.path.join(self.worktree, ".claude", "kairos.local.md")
        with open(own, "w") as handle:
            handle.write("---\nrepository: other\n---\n")
        self.assertEqual(session_start.wiring_path(self.worktree), own)

    def test_no_wiring_anywhere_is_none(self):
        os.remove(self.wiring)
        self.assertIsNone(session_start.wiring_path(self.worktree))
        self.assertIsNone(session_start.wiring_path(self.tmp.name))

    def test_the_project_dir_comes_from_the_environment(self):
        with unittest.mock.patch.dict(os.environ, {"CLAUDE_PROJECT_DIR": self.worktree}):
            self.assertEqual(session_start.wiring_path(), self.wiring)

    def run_script(self, script, *args, payload=None):
        import json
        import subprocess
        import sys

        env = dict(
            os.environ,
            CLAUDE_PROJECT_DIR=self.worktree,
            KAIROS_RULES_STATE_DIR=os.path.join(self.tmp.name, "rules"),
            KAIROS_WORK_STATE_DIR=os.path.join(self.tmp.name, "work"),
            KAIROS_RALPH_STATE_DIR=os.path.join(self.tmp.name, "ralph"),
            PATH="/usr/bin:/bin",
        )
        result = subprocess.run(
            [sys.executable, os.path.join(HERE, script), *args],
            input=json.dumps(payload or {"session_id": "session-0520", "source": "startup"}),
            capture_output=True,
            text=True,
            env=env,
            cwd=self.worktree,
            check=False,
            timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout

    def test_the_session_hook_in_a_worktree_gives_the_wiring_of_the_clone(self):
        import json

        out = self.run_script("session_start.py")
        context = json.loads(out)["hookSpecificOutput"]["additionalContext"]
        self.assertIn("- repository: payments-api", context)
        self.assertIn("- adr_board: platform-adrs", context)

    def test_the_compaction_hook_in_a_worktree_gives_the_wiring_of_the_clone(self):
        import json

        out = self.run_script(
            "active_work.py", "compact",
            payload={"session_id": "session-0520", "source": "compact"},
        )
        context = json.loads(out)["hookSpecificOutput"]["additionalContext"]
        self.assertIn("- repository: payments-api", context)

    def test_the_rules_and_the_reminder_in_a_worktree_read_the_clone(self):
        def load(name):
            spec = importlib.util.spec_from_file_location(name, os.path.join(HERE, name + ".py"))
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            return module

        with unittest.mock.patch.dict(os.environ, {"CLAUDE_PROJECT_DIR": self.worktree}):
            self.assertEqual(load("kairos_rules").deployment_url(), "http://127.0.0.1:9")
            self.assertEqual(load("memory_reminder").adr_board(), "platform-adrs")
            self.assertTrue(load("index_update").is_wired(self.worktree))


class BoardFolder(unittest.TestCase):
    """KAIROS-T-0521: in a board folder (clones, no `.git` of its own) the
    session hook lists each clone and worktree, its origin remote, its
    branch and how far it is behind the origin default branch, from the
    local refs."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = os.path.realpath(self.tmp.name)
        self.board = os.path.join(root, "board")
        os.makedirs(self.board)
        origin = os.path.join(root, "origin.git")
        os.makedirs(origin)
        git("init", "-q", "--bare", "-b", "main", cwd=origin)
        seed = make_clone(root, "seed", origin=origin)
        git("push", "-q", "origin", "main", cwd=seed)
        git("clone", "-q", origin, "payments", cwd=self.board)
        self.clone = os.path.join(self.board, "payments")
        for n in (2, 3):
            git("commit", "-q", "--allow-empty", "-m", f"c{n}", cwd=seed)
        git("push", "-q", "origin", "main", cwd=seed)
        git("fetch", "-q", "origin", cwd=self.clone)  # the clone is 2 behind
        git("worktree", "add", "-q", "-b", "acme-t-0001", "../wt-acme-t-0001", "origin/main", cwd=self.clone)
        # A user and a token in the URL: the hook never prints them.
        git("remote", "set-url", "origin", "https://robin:s3cret@GitHub.com/Acme/Payments.git", cwd=self.clone)
        os.makedirs(os.path.join(self.board, "notes"))  # not a checkout
        with open(os.path.join(self.board, "README.md"), "w") as handle:
            handle.write("x")

    def tearDown(self):
        self.tmp.cleanup()

    def by_name(self):
        return {e["name"]: e for e in session_start.board_checkouts(self.board)}

    def test_each_clone_and_worktree_is_listed_and_nothing_else(self):
        self.assertEqual(sorted(self.by_name()), ["payments", "wt-acme-t-0001"])

    def test_the_clone_has_its_branch_its_remote_and_its_count_behind(self):
        clone = self.by_name()["payments"]
        self.assertFalse(clone["worktree"])
        self.assertEqual(clone["branch"], "main")
        self.assertEqual(clone["remote"], "github.com/acme/payments")
        self.assertEqual(clone["default"], "main")
        self.assertEqual(clone["behind"], 2)

    def test_a_worktree_names_its_clone_and_its_branch(self):
        worktree = self.by_name()["wt-acme-t-0001"]
        self.assertTrue(worktree["worktree"])
        self.assertEqual(worktree["clone"], "payments")
        self.assertEqual(worktree["branch"], "acme-t-0001")
        self.assertEqual(worktree["behind"], 0)

    def test_without_origin_head_the_default_is_main(self):
        git("symbolic-ref", "--delete", "refs/remotes/origin/HEAD", cwd=self.clone)
        self.assertEqual(self.by_name()["payments"]["default"], "main")
        self.assertEqual(self.by_name()["payments"]["behind"], 2)

    def test_the_text_says_as_of_the_last_fetch_and_hides_the_credentials(self):
        text = session_start.checkouts_text(session_start.board_checkouts(self.board))
        self.assertIn("as of the last fetch", text)
        self.assertIn(
            "- payments/: remote github.com/acme/payments, branch main, 2 behind origin/main",
            text,
        )
        self.assertIn(
            "- wt-acme-t-0001/: worktree of payments/, remote github.com/acme/payments, "
            "branch acme-t-0001, 0 behind origin/main",
            text,
        )
        self.assertIn("`list_repositories`", text)
        self.assertNotIn("s3cret", text)
        self.assertNotIn("robin", text)

    def test_a_checkout_is_not_a_board_folder(self):
        self.assertEqual(session_start.board_checkouts(self.clone), [])
        self.assertIsNone(session_start.checkouts_text([]))

    def test_the_scan_stops_at_its_time_limit(self):
        ticks = iter([0.0, 0.0, 5.0, 5.0])
        checkouts = session_start.board_checkouts(self.board, seconds=1.0, clock=lambda: next(ticks))
        self.assertEqual([e.get("name") for e in checkouts], ["payments", None])
        self.assertIn("the list stops here", session_start.checkouts_text(checkouts))

    def test_remotes_are_normalized_like_the_cli(self):
        for url in (
            "git@github.com:Acme/Payments.git",
            "https://github.com/acme/payments",
            "https://user:token@github.com/acme/payments.git/",
            "ssh://git@github.com/acme/payments.git",
        ):
            self.assertEqual(session_start.normalized_remote(url), "github.com/acme/payments")

    def test_the_board_folder_gets_no_bootstrap_advice(self):
        checkouts = session_start.board_checkouts(self.board)
        text = session_start.build_context(
            {"deployment_url": "http://127.0.0.1:9", "team_board": "platform"},
            "deployment reachable",
            None,
            checkouts,
        )
        self.assertIn("Clones here", text)
        self.assertIn("board folder. Pick the task, then open the clone of its repository", text)
        self.assertNotIn("/kairos:bootstrap to detect", text)
        # Outside a board folder the advice stays.
        text = session_start.build_context(
            {"deployment_url": "http://127.0.0.1:9", "team_board": "platform"},
            "deployment reachable",
        )
        self.assertIn("/kairos:bootstrap to detect", text)
        self.assertNotIn("Clones here", text)

    def test_the_hook_in_a_board_folder_lists_the_clones(self):
        import json
        import subprocess
        import sys

        os.makedirs(os.path.join(self.board, ".claude"))
        with open(os.path.join(self.board, ".claude", "kairos.local.md"), "w") as handle:
            handle.write("---\ndeployment_url: http://127.0.0.1:9\nteam_board: platform\n---\n")
        env = dict(
            os.environ,
            CLAUDE_PROJECT_DIR=self.board,
            KAIROS_RULES_STATE_DIR=os.path.join(self.tmp.name, "rules"),
            PATH="/usr/bin:/bin",
        )
        result = subprocess.run(
            [sys.executable, os.path.join(HERE, "session_start.py")],
            input=json.dumps({"session_id": "session-0521", "source": "startup"}),
            capture_output=True, text=True, env=env, cwd=self.board, check=False, timeout=30,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        context = json.loads(result.stdout)["hookSpecificOutput"]["additionalContext"]
        self.assertIn("- payments/: remote github.com/acme/payments, branch main, 2 behind origin/main", context)
        self.assertIn("- wt-acme-t-0001/: worktree of payments/", context)
