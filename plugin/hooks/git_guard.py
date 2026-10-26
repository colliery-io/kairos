#!/usr/bin/env python3
"""The git guard (KAIROS-T-0497, COLLIERY-I-0631): a PreToolUse hook on Bash
that asks before a command that pushes, merges, rebases, tags, releases or
rewrites history. It asks every time (Dylan, 2026-10-15); it does not look
for a "go" in the conversation.

The command is split into its simple commands (`&&`, `||`, `;`, `|`, a new
line, a subshell, `$(...)` and backticks), each simple command into words,
and the leading `VAR=value` words and wrappers (`env`, `nohup`, `sudo`,
`time`, `command`) are dropped. Then:

    git push ...                         a push, but for a feature branch (below)
    git pull ...                         a merge or a rebase (KAIROS-T-0548)
    git merge ...                        into any branch; the user decides
    git rebase ...
    git tag <name> | -a | -d | -f        a list (`git tag`, `-l`) is silent
    git reset --hard ...
    git commit --amend ...
    git clean -f | --force               deletes files git does not track
    git checkout -- <path>               discards changes
    git restore <path>                   discards changes (`--staged` is silent)
    git branch -D|-d of a remote-tracking branch (`-r`, or `origin/x`)
    git <anything> --force | --force-with-lease | -f on push
    gh pr merge ...
    gh release create | edit | delete ...
    gh api .../merge                     a merge through the API

A command inside `sh -c`, `bash -c`, `zsh -c` or `eval` is scanned the same
way (KAIROS-T-0548): the string is split into its simple commands too.

The push policy (KAIROS-T-0542): an agent pushes the branch of its task and
opens a draft PR; a merge, a release and a tag need the user's go. So one
push form is silent:

    git [-C <path>] push [-u | --set-upstream] origin <branch>

with exactly one `<branch>`, which is a local branch (`refs/heads/<branch>`),
not a tag of the same name, and not the default branch (`origin/HEAD`; `main`
and `master` always count as default). Any other option, a force flag, a
`+` or `:` refspec, `HEAD`, `--delete`, `--tags`, `--all`, `--mirror`, a
`git -c` or `--git-dir`, a `VAR=value` or wrapper before `git`, a
`remote.origin.push` refspec in the config, or a `cd` in the line asks.
When the guard cannot read the refs (no repository, no `origin/HEAD`, a git
error), it asks.

A match gives `permissionDecision: "ask"` with a one-line reason that names
the command. Anything else gives no output. Each error gives no output and
exit 0: the hook never blocks a tool call by accident. The hook holds no
credentials and reads no file; for a push it runs read-only git commands
(`symbolic-ref`, `show-ref`, `config --get-all`) in the folder of the
command. Python stdlib only.
"""

import json
import os
import re
import shlex
import subprocess
import sys

SEPARATORS = re.compile(r"&&|\|\||[;|\n()`]|\$\(")
ASSIGNMENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")
WRAPPERS = ("env", "nohup", "sudo", "time", "command", "exec")
SHELLS = ("sh", "bash", "zsh")
GIT_OPTIONS_WITH_VALUE = ("-C", "-c", "--git-dir", "--work-tree", "--namespace", "--exec-path")
GH_RELEASE_WRITES = ("create", "edit", "delete", "upload", "delete-asset")
FORCE_FLAGS = ("--force", "--force-with-lease", "--force-if-includes")
PUSH_REMOTE = "origin"
UPSTREAM_FLAGS = ("-u", "--set-upstream")
ALWAYS_DEFAULT = ("main", "master")
CHANGES_FOLDER = re.compile(r"(?:^|[\s;&|(`'\"])(?:cd|pushd|popd)(?:\s|$|[;&|)`'\"])")
PUSH_ASK = "`git push`: a push needs the user's go, each time."


class GitRefs:
    """Read-only lookups of the refs of a repository, with git. The tests
    give a fake with the same three methods."""

    def __init__(self, timeout=2):
        # Four calls at most: 8 seconds, under the hook timeout of 10.
        self.timeout = timeout

    def git(self, folder, *args):
        try:
            result = subprocess.run(
                ["git", "-C", folder, *args],
                capture_output=True,
                text=True,
                timeout=self.timeout,
                check=False,
            )
        except (OSError, subprocess.SubprocessError):
            return None
        return result

    def default_branch(self, folder, remote):
        """The default branch of `remote` (`origin/HEAD`), or None."""
        result = self.git(folder, "symbolic-ref", "--quiet", "--short", f"refs/remotes/{remote}/HEAD")
        if result is None or result.returncode != 0:
            return None
        name = result.stdout.strip()
        prefix = f"{remote}/"
        return name[len(prefix):] if name.startswith(prefix) else None

    def push_refspecs(self, folder, remote):
        """The `remote.<remote>.push` refspecs (a list), or None when git
        cannot tell. A refspec there can map a branch to the default."""
        result = self.git(folder, "config", "--get-all", f"remote.{remote}.push")
        if result is None or result.returncode not in (0, 1):
            return None
        return result.stdout.split()

    def has_ref(self, folder, ref):
        """True when `ref` exists; None when git cannot tell."""
        result = self.git(folder, "show-ref", "--verify", "--quiet", ref)
        if result is None:
            return None
        if result.returncode == 0:
            return True
        if result.returncode == 1:
            return False
        return None


def simple_commands(command):
    """The simple commands of a shell command line."""
    return [part.strip() for part in SEPARATORS.split(command) if part and part.strip()]


def plain_git(simple):
    """True when the simple command is `git ...` with balanced quotes and no
    `VAR=value` word or wrapper before it: `GIT_DIR=...` can point git at
    another repository, and a cut quote hides the real command."""
    try:
        words = shlex.split(simple, comments=False)
    except ValueError:
        return False
    return bool(words) and words[0] == "git"


def words_of(simple):
    """The words of one simple command, without the leading assignments and
    wrappers. A quote that shlex cannot read falls back to a split on
    spaces, so a guarded command is still seen."""
    try:
        words = shlex.split(simple, comments=False)
    except ValueError:
        # A quote that the split on separators cut open (`bash -c 'git fetch
        # && git push'`): the words keep their quote marks, so drop them.
        words = [word.strip("'\"") for word in simple.split()]
    while words and (ASSIGNMENT.match(words[0]) or words[0] in WRAPPERS):
        words = words[1:]
    return words


def git_subcommand(words):
    """(subcommand, its arguments) of a `git` word list, after the global
    options of git; (None, []) when there is none."""
    subcommand, args, _ = git_parts(words)
    return subcommand, args


def git_parts(words):
    """(subcommand, its arguments, the global options) of a `git` word list."""
    rest = words[1:]
    options = []
    while rest:
        word = rest[0]
        if word in GIT_OPTIONS_WITH_VALUE:
            options.append(tuple(rest[:2]))
            rest = rest[2:]
        elif word.startswith("-"):
            options.append((word,))
            rest = rest[1:]
        else:
            return word, rest[1:], options
    return None, [], options


def push_folder(options, cwd):
    """The folder of a `git push`: the `-C` paths on `cwd`. None when a
    global option other than `-C` is there (`-c`, `--git-dir`, ...)."""
    folder = cwd or os.getcwd()
    for option in options:
        if len(option) != 2 or option[0] != "-C":
            return None
        folder = os.path.join(folder, option[1])
    return folder


def feature_push(words, cwd, refs):
    """True when one `git push` is the push of a feature branch that the
    policy lets through (KAIROS-T-0542); False for each other push."""
    if refs is None:
        return False
    _, args, options = git_parts(words)
    folder = push_folder(options, cwd)
    if folder is None:
        return False
    flags = [arg for arg in args if arg.startswith("-")]
    names = [arg for arg in args if not arg.startswith("-")]
    if any(flag not in UPSTREAM_FLAGS for flag in flags) or len(names) != 2:
        return False
    remote, branch = names
    if remote != PUSH_REMOTE or branch == "HEAD" or branch in ALWAYS_DEFAULT:
        return False
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._/-]*", branch) or branch.endswith((".lock", "/", ".")) or ".." in branch:
        return False
    if refs.push_refspecs(folder, remote) != []:
        return False
    default = refs.default_branch(folder, remote)
    if not default or branch == default:
        return False
    if refs.has_ref(folder, f"refs/heads/{branch}") is not True:
        return False
    return refs.has_ref(folder, f"refs/tags/{branch}") is False


def has_short_flag(args, letter):
    """True when a `-xyz` cluster in `args` holds `letter`."""
    for arg in args:
        if arg.startswith("-") and not arg.startswith("--") and letter in arg[1:]:
            return True
    return False


def guarded_git(words, cwd=None, refs=None):
    """The reason to ask for one `git` command, or None. `refs` reads the
    refs for a push (None: each push asks)."""
    subcommand, args = git_subcommand(words)
    if subcommand is None:
        return None
    if any(arg in FORCE_FLAGS or arg.startswith("--force-with-lease=") for arg in args):
        return f"`git {subcommand} --force`: a history rewrite needs the user's go, each time."
    if subcommand == "push":
        if feature_push(words, cwd, refs):
            return None
        return PUSH_ASK
    if subcommand == "pull":
        return "`git pull`: a pull merges or rebases; it needs the user's go, each time."
    if subcommand == "merge":
        return "`git merge`: a merge needs the user's go, each time."
    if subcommand == "rebase":
        return "`git rebase`: a rebase rewrites history; it needs the user's go, each time."
    if subcommand == "tag":
        listing = not args or any(arg in ("-l", "--list", "-n") or arg.startswith("-n") for arg in args)
        if listing:
            return None
        return "`git tag`: a tag starts a release; it needs the user's go, each time."
    if subcommand == "reset" and "--hard" in args:
        return "`git reset --hard`: a history rewrite needs the user's go, each time."
    if subcommand == "commit" and "--amend" in args:
        return "`git commit --amend`: a history rewrite needs the user's go, each time."
    if subcommand == "clean" and (has_short_flag(args, "f") or "--force" in args):
        return "`git clean -f`: it deletes files that git does not track; it needs the user's go, each time."
    if subcommand == "checkout" and "--" in args:
        return "`git checkout -- <path>`: it discards changes; it needs the user's go, each time."
    if subcommand == "restore" and not (has_short_flag(args, "S") or "--staged" in args):
        return "`git restore`: it discards changes; it needs the user's go, each time."
    if subcommand == "branch":
        deleting = has_short_flag(args, "D") or has_short_flag(args, "d") or "--delete" in args
        remote = has_short_flag(args, "r") or "--remotes" in args
        remote = remote or any("/" in arg for arg in args if not arg.startswith("-"))
        if deleting and remote:
            return "`git branch -D` of a remote-tracking branch needs the user's go, each time."
    return None


def guarded_gh(words):
    """The reason to ask for one `gh` command, or None."""
    args = [word for word in words[1:] if not word.startswith("-")]
    if args[:2] == ["pr", "merge"]:
        return "`gh pr merge`: a merge needs the user's go, each time."
    if len(args) >= 2 and args[0] == "release" and args[1] in GH_RELEASE_WRITES:
        return f"`gh release {args[1]}`: a release needs the user's go, each time."
    if args[:1] == ["api"] and any(arg.rstrip("/").endswith("/merge") for arg in words[1:]):
        return "`gh api .../merge`: a merge needs the user's go, each time."
    return None


def inner_command(words):
    """The command line inside `sh -c <string>`, `bash -c`, `zsh -c` or
    `eval ...`, or None when `words` is not one of those."""
    if not words:
        return None
    if words[0] == "eval":
        return " ".join(words[1:]) or None
    if words[0] in SHELLS:
        for index, word in enumerate(words[1:], start=1):
            if word.startswith("-") and not word.startswith("--") and "c" in word[1:]:
                return words[index + 1] if index + 1 < len(words) else None
    return None


def guarded(command, cwd=None, refs=None):
    """The reason to ask for a Bash command, or None when nothing in it is
    guarded. `cwd` is the folder of the command; `refs` reads the refs for
    a push (None: each push asks, so the function is pure)."""
    if not isinstance(command, str):
        return None
    if CHANGES_FOLDER.search(command):
        refs = None  # a `cd` in the line: the folder of the push is unknown
    for simple in simple_commands(command):
        words = words_of(simple)
        if not words:
            continue
        reason = None
        if words[0] == "git":
            reason = guarded_git(words, cwd, refs if plain_git(simple) else None)
        elif words[0] == "gh":
            reason = guarded_gh(words)
        else:
            inner = inner_command(words)
            if inner is not None and inner != command:
                reason = guarded(inner, cwd, refs)
        if reason:
            return reason
    return None


def main():
    try:
        payload = json.loads(sys.stdin.read() or "null")
        if not isinstance(payload, dict) or payload.get("tool_name") != "Bash":
            return
        tool_input = payload.get("tool_input")
        if not isinstance(tool_input, dict):
            return
        cwd = payload.get("cwd")
        cwd = cwd if isinstance(cwd, str) and cwd else None
        reason = guarded(tool_input.get("command"), cwd, GitRefs())
        if not reason:
            return
        json.dump(
            {
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "ask",
                    "permissionDecisionReason": reason,
                }
            },
            sys.stdout,
        )
    except Exception as error:  # never block a tool call by accident (KAIROS-T-0708: the reason goes to the log)
        sys.stderr.write(f"kairos git guard: the hook failed: {error}\n")
        return


if __name__ == "__main__":
    main()
