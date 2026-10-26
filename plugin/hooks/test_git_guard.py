#!/usr/bin/env python3
"""Unit tests for the git guard (KAIROS-T-0497, COLLIERY-I-0631): the
PreToolUse hook on Bash that asks before a push, a merge, a rebase, a tag, a
PR merge, a release and every history rewrite. Stdlib only; run with
`python3 -m unittest plugin/hooks/test_git_guard.py` (or `angreal test
unit`, which includes it).

The tests run `git_guard.py` as a subprocess, the way Claude Code runs it.
The push policy (KAIROS-T-0542) is tested with a fake ref lookup, and once
on a temporary repository with git. No test calls a server."""

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
SCRIPT = os.path.join(HERE, "git_guard.py")
SPEC = importlib.util.spec_from_file_location("git_guard", SCRIPT)
git_guard = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(git_guard)

ASKS = (
    "git push",
    "git push origin main",
    "git push --force",
    "git push --force-with-lease origin main",
    "git push origin --delete old-branch",
    "git push --delete origin old-branch",
    "git -C /work/repo push origin main",
    "git -c user.name=x push",
    "git --git-dir=/work/repo/.git push",
    "git merge feature",
    "git merge --no-ff origin/main",
    "git rebase main",
    "git rebase -i HEAD~3",
    "git tag v1.2.0",
    "git tag -a v1.2.0 -m 'release'",
    "git tag -d v1.2.0",
    "git reset --hard HEAD~1",
    "git reset --hard origin/main",
    "git commit --amend",
    "git commit --amend --no-edit",
    "git checkout --force main",
    "git branch -D origin/feature",
    "git branch -rD origin/feature",
    "git branch -r -D origin/feature",
    "gh pr merge 42",
    "gh pr merge --squash",
    "gh release create v1.2.0",
    "gh release edit v1.2.0 --draft=false",
    "gh release delete v1.2.0 -y",
    "git fetch origin && git push origin main",
    "git status; git merge feature",
    "git log | head; git rebase main",
    "git status || git push",
    "(git push origin main)",
    "echo $(git push)",
    "echo `git tag v1`",
    "git add -A\ngit commit -m x\ngit push",
    "cd /work/repo && git push",
    "env FOO=1 git push",
    "FOO=1 git push",
    "nohup git push > log 2>&1",
    # KAIROS-T-0548: a command inside a shell string or an eval.
    'bash -c "git push origin main"',
    "sh -c 'git merge feature'",
    'zsh -lc "git rebase main"',
    "bash -ec 'git fetch && git push'",
    "eval git push",
    'eval "git tag v1"',
    "exec bash -c 'git push'",
    # KAIROS-T-0548: a pull, a clean, a discard, a merge through the API.
    "git pull",
    "git pull --rebase origin main",
    "git pull --ff-only origin main",
    "git clean -fdx",
    "git clean -f",
    "git clean --force -d",
    "git checkout -- .",
    "git checkout -- src/main.rs",
    "git restore .",
    "git restore src/main.rs",
    "gh api repos/o/r/pulls/42/merge -X PUT",
    "gh api -X PUT /repos/o/r/pulls/42/merge/",
)

SILENT = (
    "git status",
    "git log --oneline -5",
    "git fetch origin main",
    "git fetch --all",
    "git diff origin/main",
    "git add -A && git commit -m 'KAIROS-T-0497 git guard'",
    "git checkout -b feature",
    "git branch feature",
    "git branch -d feature",
    "git branch -D feature",
    "git tag",
    "git tag -l",
    "git tag --list 'v*'",
    "git reset HEAD~1",
    "git reset --soft HEAD~1",
    "git worktree add /work/wt -b topic origin/main",
    "grep -rn 'push' plugin/hooks",
    "grep 'git push' README.md",
    "echo 'git push'",
    "cat docs/how-to-push.md",
    "gh pr create --draft --title x",
    "gh pr checks 42 --watch",
    "gh release list",
    "gh release view v1.2.0",
    "ls",
    "",
    # KAIROS-T-0548: the silent neighbours of the new rules.
    "git clean -n",
    "git clean --dry-run -d",
    "git restore --staged src/main.rs",
    "git restore -S .",
    "git checkout feature",
    "git checkout -b topic origin/main",
    "bash -c 'git status'",
    "sh -c ls",
    "eval echo hi",
    "gh api repos/o/r/pulls/42",
    "gh api repos/o/r/pulls/42/comments",
)


def payload(command, tool_name="Bash", cwd=None):
    data = {"session_id": "session-0001", "tool_name": tool_name, "tool_input": {"command": command}}
    if cwd:
        data["cwd"] = cwd
    return json.dumps(data)


class FakeRefs:
    """The refs of a repository with the default branch `trunk`, the
    branches `kairos-t-0542`, `feature/x` and `v1.0`, and the tag `v1.0`."""

    def __init__(self, default="trunk", heads=("trunk", "main", "kairos-t-0542", "feature/x", "v1.0"),
                 tags=("v1.0",), refspecs=()):
        self.default = default
        self.heads = set(heads)
        self.tags = set(tags)
        self.refspecs = list(refspecs)
        self.folders = []

    def default_branch(self, folder, remote):
        self.folders.append(folder)
        return self.default if remote == "origin" else None

    def push_refspecs(self, folder, remote):
        return self.refspecs

    def has_ref(self, folder, ref):
        kind, _, name = ref[len("refs/"):].partition("/")
        return name in (self.heads if kind == "heads" else self.tags)


# The push of a feature branch: silent (KAIROS-T-0542).
FEATURE_PUSHES = (
    "git push -u origin kairos-t-0542",
    "git push origin kairos-t-0542",
    "git push --set-upstream origin kairos-t-0542",
    "git push origin -u kairos-t-0542",
    "git -C /work/wt-kairos-t-0542 push -u origin kairos-t-0542",
    "git -C /work/wt-kairos-t-0542 push origin kairos-t-0542",
    "git push -u origin feature/x",
    "git add -A && git commit -m x && git push -u origin kairos-t-0542",
)

# Each other push still asks, also with the ref lookup.
REFUSED_PUSHES = (
    "git push",
    "git push origin",
    "git push -u origin trunk",
    "git push origin main",
    "git push origin master",
    "git push -u origin HEAD",
    "git push --force origin kairos-t-0542",
    "git push -f origin kairos-t-0542",
    "git push -uf origin kairos-t-0542",
    "git push --force-with-lease origin kairos-t-0542",
    "git push --force-with-lease=kairos-t-0542 origin kairos-t-0542",
    "git push origin +kairos-t-0542",
    "git push origin kairos-t-0542:trunk",
    "git push origin kairos-t-0542:kairos-t-0542",
    "git push origin v1.0",
    "git push origin refs/tags/v1.0",
    "git push --tags origin kairos-t-0542",
    "git push --follow-tags origin kairos-t-0542",
    "git push origin --delete kairos-t-0542",
    "git push -d origin kairos-t-0542",
    "git push origin :kairos-t-0542",
    "git push --all origin",
    "git push --mirror origin",
    "git push origin no-such-branch",
    "git push origin kairos-t-0542 feature/x",
    "git push upstream kairos-t-0542",
    "git push origin $BRANCH",
    "git push origin $(git branch --show-current)",
    "git -c remote.origin.url=x push origin kairos-t-0542",
    "git --git-dir=/x/.git push origin kairos-t-0542",
    "GIT_DIR=/x/.git git push origin kairos-t-0542",
    "cd /work/other && git push -u origin kairos-t-0542",
    "bash -c 'cd /work/other && git push -u origin kairos-t-0542'",
    "echo 'a && git push origin kairos-t-0542'",
)


class GitGuardPure(unittest.TestCase):
    def test_each_listed_shape_is_named(self):
        for command in ASKS:
            with self.subTest(command=command):
                self.assertIsNotNone(git_guard.guarded(command))

    def test_each_non_match_is_silent(self):
        for command in SILENT:
            with self.subTest(command=command):
                self.assertIsNone(git_guard.guarded(command))

    def test_the_reason_names_the_command(self):
        self.assertIn("git push", git_guard.guarded("git push origin main"))
        self.assertIn("git rebase", git_guard.guarded("git fetch && git rebase main"))
        self.assertIn("gh pr merge", git_guard.guarded("gh pr merge 42"))
        self.assertIn("--amend", git_guard.guarded("git commit --amend"))
        self.assertIn("--hard", git_guard.guarded("git reset --hard"))
        self.assertIn("--force", git_guard.guarded("git checkout --force main"))

    def test_a_command_that_shlex_cannot_split_is_scanned_by_words(self):
        self.assertIsNotNone(git_guard.guarded("git push 'unterminated"))
        self.assertIsNone(git_guard.guarded("echo 'unterminated"))


class PushPolicy(unittest.TestCase):
    """KAIROS-T-0542: the push of a feature branch to origin goes through;
    each other push asks."""

    def test_each_feature_push_is_silent(self):
        for command in FEATURE_PUSHES:
            with self.subTest(command=command):
                self.assertIsNone(git_guard.guarded(command, "/work/repo", FakeRefs()))

    def test_each_other_push_asks(self):
        for command in REFUSED_PUSHES:
            with self.subTest(command=command):
                reason = git_guard.guarded(command, "/work/repo", FakeRefs())
                self.assertIsNotNone(reason)
                self.assertIn("git push", reason)

    def test_with_no_ref_lookup_each_push_asks(self):
        for command in FEATURE_PUSHES:
            with self.subTest(command=command):
                self.assertIsNotNone(git_guard.guarded(command))

    def test_an_unknown_default_branch_asks(self):
        refs = FakeRefs(default=None)
        self.assertIsNotNone(git_guard.guarded("git push -u origin kairos-t-0542", "/work/repo", refs))

    def test_a_push_refspec_in_the_config_asks(self):
        refs = FakeRefs(refspecs=["refs/heads/kairos-t-0542:refs/heads/trunk"])
        self.assertIsNotNone(git_guard.guarded("git push origin kairos-t-0542", "/work/repo", refs))

    def test_a_branch_with_a_tag_of_the_same_name_asks(self):
        self.assertIsNotNone(git_guard.guarded("git push origin v1.0", "/work/repo", FakeRefs()))

    def test_the_lookup_runs_in_the_folder_of_dash_c(self):
        refs = FakeRefs()
        git_guard.guarded("git -C ../wt-kairos-t-0542 push origin kairos-t-0542", "/work/repo", refs)
        self.assertEqual(refs.folders, ["/work/repo/../wt-kairos-t-0542"])

    def test_a_merge_or_a_tag_still_asks_with_the_lookup(self):
        for command in ("git merge kairos-t-0542", "git tag v2.0", "gh pr merge 42",
                        "git push origin kairos-t-0542 && git tag v2.0"):
            with self.subTest(command=command):
                self.assertIsNotNone(git_guard.guarded(command, "/work/repo", FakeRefs()))


def git(folder, *args):
    subprocess.run(
        ["git", "-C", folder, "-c", "user.name=Robin", "-c", "user.email=robin@example.com", *args],
        check=True,
        capture_output=True,
    )


class GitGuardHook(unittest.TestCase):
    def run_hook(self, stdin):
        return subprocess.run(
            [sys.executable, SCRIPT],
            input=stdin,
            capture_output=True,
            text=True,
            check=False,
        )

    def test_a_push_gets_an_ask_with_a_one_line_reason(self):
        result = self.run_hook(payload("git push origin main"))
        self.assertEqual(result.returncode, 0, result.stderr)
        output = json.loads(result.stdout)["hookSpecificOutput"]
        self.assertEqual(output["hookEventName"], "PreToolUse")
        self.assertEqual(output["permissionDecision"], "ask")
        self.assertIn("git push", output["permissionDecisionReason"])
        self.assertNotIn("\n", output["permissionDecisionReason"])

    def test_a_feature_push_in_a_real_repository_gives_no_output(self):
        with tempfile.TemporaryDirectory() as folder:
            git(folder, "init", "-q", "-b", "main")
            git(folder, "commit", "-q", "--allow-empty", "-m", "first")
            git(folder, "update-ref", "refs/remotes/origin/main", "HEAD")
            git(folder, "symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main")
            git(folder, "branch", "kairos-t-0542")
            git(folder, "tag", "v1.0")
            git(folder, "branch", "v1.0")
            silent = self.run_hook(payload("git push -u origin kairos-t-0542", cwd=folder))
            self.assertEqual(silent.returncode, 0, silent.stderr)
            self.assertEqual(silent.stdout, "")
            silent = self.run_hook(payload(f"git -C {folder} push origin kairos-t-0542", cwd="/"))
            self.assertEqual(silent.stdout, "")
            for command in ("git push origin main", "git push origin v1.0",
                            "git push origin no-such-branch", "git push -f origin kairos-t-0542"):
                with self.subTest(command=command):
                    asked = self.run_hook(payload(command, cwd=folder))
                    output = json.loads(asked.stdout)["hookSpecificOutput"]
                    self.assertEqual(output["permissionDecision"], "ask")

    def test_a_push_outside_a_repository_asks(self):
        with tempfile.TemporaryDirectory() as folder:
            result = self.run_hook(payload("git push -u origin kairos-t-0542", cwd=folder))
            output = json.loads(result.stdout)["hookSpecificOutput"]
            self.assertEqual(output["permissionDecision"], "ask")

    def test_a_non_match_gives_no_output(self):
        result = self.run_hook(payload("git status"))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "")

    def test_another_tool_gives_no_output(self):
        result = self.run_hook(payload("git push", tool_name="Read"))
        self.assertEqual(result.stdout, "")

    def test_bad_input_gives_no_output_and_exit_0(self):
        for stdin in ("", "not json", "[]", json.dumps({"tool_name": "Bash"})):
            with self.subTest(stdin=stdin):
                result = self.run_hook(stdin)
                self.assertEqual(result.returncode, 0)
                self.assertEqual(result.stdout, "")

    def test_the_hook_is_registered_on_bash(self):
        with open(os.path.join(HERE, "hooks.json"), encoding="utf-8") as handle:
            hooks = json.load(handle)
        entries = [
            hook
            for entry in hooks["hooks"]["PreToolUse"]
            if entry.get("matcher") == "Bash"
            for hook in entry["hooks"]
        ]
        self.assertTrue(any("git_guard.py" in hook["command"] for hook in entries))


if __name__ == "__main__":
    unittest.main()
