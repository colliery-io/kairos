#!/usr/bin/env python3
"""SessionStart hook: inject the repo's Kairos wiring as session context.

Reads `.claude/kairos.local.md` (written by /kairos:bootstrap per
KAIROS-A-0014) and emits its contents — deployment, tenant, repository,
stream, boards — as `additionalContext`, plus an instruction to pull live
state. With a `repository` (KAIROS-A-0019, T-0108) the hint is
repo-scoped: read the repository (its "how to work here" description and
in-flight PRs), then the team board narrowed to that repository — the
session's queue. Without one it stays board-scoped as before. The team
decides the board and the repository is a link (COLLIERY-A-0023), so the
hint also says to name a board at create and how to send a request.

v1 design decision (KAIROS-T-0034): hooks have no OAuth token broker —
tokens live with the MCP client (A-0014), and a shell hook cannot drive the
client's browser OAuth flow (A-0011). So this hook NEVER authenticates and
holds no secrets. It performs one unauthenticated reachability probe
(`GET <deployment_url>/healthz`, short timeout) and then instructs the
agent to call `my_boards` / `board_items` over the already-authenticated
kairos MCP connection for live items. Offline degrades to a note, never a
failure.

In a worktree the wiring is the file of its clone (KAIROS-T-0520,
`wiring_path`). In a board folder the hook lists the clones there
(KAIROS-T-0521). When the `kairos` CLI has `kairos repos local`
(KAIROS-T-0646), the CLI gives the list, with the repository of each clone:
the CLI uses its own credentials (a key in the environment or its login),
and the hook still holds none. An older CLI, or a CLI that fails, gives the
listing of the hook. Not configured (no kairos.local.md) -> silent exit 0.
Python stdlib only.
"""

import json
import os
import shutil
import subprocess
import time
import sys
import urllib.request

FRONTMATTER_KEYS = (
    "deployment_url",
    "tenant",
    "repository",
    "delivery_stream",
    "team_board",
    "initiative_board",
    "adr_board",
)


WIRING_FILE = os.path.join(".claude", "kairos.local.md")


def wiring_path(project_dir=None):
    """The path of the wiring of a session, or None (KAIROS-T-0520).

    First `<project>/.claude/kairos.local.md`. The file is git-excluded, so
    a worktree has none: then the file of the main clone, the folder above
    `git rev-parse --git-common-dir`. Each hook reads the wiring with this."""
    project = project_dir or os.environ.get("CLAUDE_PROJECT_DIR") or os.getcwd()
    path = os.path.join(project, WIRING_FILE)
    if os.path.isfile(path):
        return path
    if not os.path.exists(os.path.join(project, ".git")):
        return None  # not a checkout: no git call, no clone to look in
    try:
        out = subprocess.run(
            ["git", "-C", project, "rev-parse", "--git-common-dir"],
            capture_output=True,
            text=True,
            timeout=2,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    common = out.stdout.strip()
    if out.returncode != 0 or not common:
        return None
    clone = os.path.dirname(os.path.realpath(os.path.join(project, common)))
    path = os.path.join(clone, WIRING_FILE)
    return path if os.path.isfile(path) else None


def git_out(folder, *args, timeout=1.0):
    """The stripped stdout of `git -C folder args`, or None on an error."""
    try:
        out = subprocess.run(
            ["git", "-C", folder, *args],
            capture_output=True,
            text=True,
            timeout=timeout,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    return out.stdout.strip() if out.returncode == 0 else None


def normalized_remote(url):
    """A remote URL as `host/path`, in lower case, with no scheme, user,
    `.git` or `/` at the end (the rule of `normalized_remote` in the CLI):
    `git@github.com:a/b.git` and `https://github.com/a/b` give
    `github.com/a/b`. A user or a token in the URL is never printed."""
    url = url.strip()
    if "://" in url:
        rest = url.split("://", 1)[1]
    else:
        rest = url.replace(":", "/", 1)  # scp form: user@host:path
    rest = rest.rsplit("@", 1)[-1]
    rest = rest.rstrip("/")
    if rest.endswith(".git"):
        rest = rest[: -len(".git")]
    return rest.lower()


def default_branch(folder):
    """The default branch of origin from the local refs: `origin/HEAD`, else
    `main`, else `master`. None when origin has no such ref."""
    head = git_out(folder, "symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD")
    if head and head.startswith("origin/"):
        return head[len("origin/"):]
    for name in ("main", "master"):
        if git_out(folder, "rev-parse", "--verify", "--quiet", f"refs/remotes/origin/{name}"):
            return name
    return None


MAX_CHECKOUTS = 40


def board_checkouts(folder, seconds=2.0, clock=time.monotonic):
    """The git checkouts in the immediate subfolders of a board folder
    (KAIROS-T-0521), sorted by name. Each is a dict: `name`, `worktree`
    (True when `.git` is a file), `clone` (the folder name of the clone of a
    worktree), `branch`, `remote` (normalized, or None), `default` and
    `behind` (commits behind `origin/<default>`, from the local refs: the
    hook runs no fetch). Empty when the folder is itself a checkout. The
    scan stops after MAX_CHECKOUTS checkouts or `seconds`: then the last
    entry is `{"stopped": True}`."""
    deadline = clock() + seconds
    if os.path.exists(os.path.join(folder, ".git")):
        return []
    try:
        names = sorted(os.listdir(folder))
    except OSError:
        return []
    checkouts = []
    for name in names:
        path = os.path.join(folder, name)
        dot_git = os.path.join(path, ".git")
        if name.startswith(".") or not os.path.exists(dot_git):
            continue
        if len(checkouts) == MAX_CHECKOUTS or clock() > deadline:
            checkouts.append({"stopped": True})
            break
        entry = {"name": name, "worktree": os.path.isfile(dot_git), "clone": None}
        if entry["worktree"]:
            common = git_out(path, "rev-parse", "--git-common-dir")
            if common:
                clone = os.path.dirname(os.path.realpath(os.path.join(path, common)))
                entry["clone"] = os.path.basename(clone)
        branch = git_out(path, "rev-parse", "--abbrev-ref", "HEAD")
        entry["branch"] = "(detached HEAD)" if branch == "HEAD" else branch
        url = git_out(path, "config", "--get", "remote.origin.url")
        entry["remote"] = normalized_remote(url) if url else None
        entry["default"] = default_branch(path) if url else None
        entry["behind"] = None
        if entry["default"]:
            count = git_out(path, "rev-list", "--count", f"HEAD..origin/{entry['default']}")
            if count and count.isdigit():
                entry["behind"] = int(count)
        checkouts.append(entry)
    return checkouts


CLI_LOCAL_SECONDS = 3.0


def cli_checkouts(folder, deployment, timeout=CLI_LOCAL_SECONDS):
    """The checkouts of a board folder from `kairos repos local --json`
    (KAIROS-T-0646), in the form of `board_checkouts`, each with its
    `repository` (the slug, or None when no repository of Kairos has the
    remote). None when `kairos` is not on PATH, when the CLI has no such
    verb (an older CLI), or when it fails: then the hook lists the clones
    itself. The CLI runs no fetch and no daily version check."""
    binary = shutil.which("kairos")
    if not binary or os.path.exists(os.path.join(folder, ".git")):
        return None
    env = dict(os.environ, KAIROS_UPDATE_CHECK="0", KAIROS_QUIET="1")
    try:
        out = subprocess.run(
            [binary, "repos", "local", "--json", "--url", deployment, folder],
            capture_output=True,
            text=True,
            timeout=timeout,
            env=env,
        )
        items = json.loads(out.stdout)["items"] if out.returncode == 0 else None
    except (OSError, subprocess.SubprocessError, ValueError, KeyError, TypeError):
        return None
    if not isinstance(items, list):
        return None
    checkouts = []
    for item in items:
        if not isinstance(item, dict) or not isinstance(item.get("folder"), str):
            return None
        if len(checkouts) == MAX_CHECKOUTS:
            checkouts.append({"stopped": True})
            break
        behind = item.get("behind")
        checkouts.append(
            {
                "name": item["folder"],
                "worktree": bool(item.get("worktree")),
                "clone": item.get("clone"),
                "branch": item.get("branch") or "(detached HEAD)",
                "remote": item.get("remote"),
                "default": item.get("default_branch"),
                "behind": behind if isinstance(behind, int) else None,
                "repository": item.get("repository"),
            }
        )
    return checkouts


def checkouts_text(checkouts):
    """The lines of the board folder for the context (pure; testable)."""
    if not checkouts:
        return None
    lines = [
        "Clones here (this is a board folder; the counts are from the local "
        "refs, as of the last fetch):"
    ]
    for entry in checkouts:
        if entry.get("stopped"):
            lines.append("- (the list stops here: more checkouts are not listed)")
            continue
        parts = []
        if "repository" in entry:  # from `kairos repos local`
            parts.append(
                f"repository {entry['repository']}"
                if entry["repository"]
                else "no repository in Kairos"
            )
        if entry["worktree"]:
            parts.append(
                f"worktree of {entry['clone']}/" if entry["clone"] else "worktree"
            )
        parts.append(
            f"remote {entry['remote']}" if entry["remote"] else "no origin remote"
        )
        if entry["branch"]:
            parts.append(f"branch {entry['branch']}")
        if entry["default"] is not None and entry["behind"] is not None:
            parts.append(f"{entry['behind']} behind origin/{entry['default']}")
        elif entry["remote"]:
            parts.append("no origin default branch in the local refs")
        lines.append(f"- {entry['name']}/: " + ", ".join(parts))
    if any("repository" in entry for entry in checkouts):
        lines.append(
            "`kairos repos local` matched each remote with the repositories of "
            "Kairos. Open the clone of the repository of a task. Work in a "
            "worktree of that clone, not in the clone (/kairos:implement)."
        )
    else:
        lines.append(
            "Match each remote with the full name of a repository in "
            "`list_repositories` to find the clone of the repository of a task. "
            "Work in a worktree of that clone, not in the clone (/kairos:implement)."
        )
    return "\n".join(lines)


CONFIG_SHOW_SECONDS = 3.0


def config_binding(project, timeout=CONFIG_SHOW_SECONDS):
    """The binding of the folder from `kairos config show --json` (#270):
    the `[[project]]` of its origin remote in the config of the person,
    then `.claude/kairos.local.md`, then `[default]`, in the order of the
    CLI. None when `kairos` is not on PATH, when it has no such verb (an
    older CLI), or when it fails: then the hook reads the file itself."""
    binary = shutil.which("kairos")
    if not binary:
        return None
    env = dict(os.environ, KAIROS_UPDATE_CHECK="0", KAIROS_QUIET="1")
    try:
        out = subprocess.run(
            [binary, "config", "show", "--json", project],
            capture_output=True,
            text=True,
            timeout=timeout,
            env=env,
        )
        data = json.loads(out.stdout) if out.returncode == 0 else None
    except (OSError, subprocess.SubprocessError, ValueError):
        return None
    return data if isinstance(data, dict) else None


def wiring(project_dir=None):
    """The wiring of the project, with the keys of the front matter, or None
    when nothing binds it to Kairos (#270). The CLI gives the binding: a
    `[[project]]` of the config of the person, else the file. KAIROS_URL
    comes first for `deployment_url`, as in the CLI. With no CLI that has
    `kairos config`, the hook reads `.claude/kairos.local.md` itself."""
    project = project_dir or os.environ.get("CLAUDE_PROJECT_DIR") or os.getcwd()
    data = config_binding(project)
    if data is None:
        path = wiring_path(project_dir)
        return read_frontmatter(path) if path else None
    if not data.get("bound"):
        return None
    values = {}
    for key, item in (data.get("values") or {}).items():
        name = "deployment_url" if key == "deployment" else key
        value = item.get("value") if isinstance(item, dict) else None
        if name in FRONTMATTER_KEYS and isinstance(value, str) and value:
            values[name] = value
    env_url = data.get("env_url")
    if isinstance(env_url, str) and env_url:
        values["deployment_url"] = env_url
    return values


def read_frontmatter(path):
    """Parse the YAML frontmatter's simple `key: value` pairs (no external
    YAML dependency; the /bootstrap contract writes flat scalar keys)."""
    with open(path, encoding="utf-8") as f:
        lines = f.read().splitlines()
    if not lines or lines[0].strip() != "---":
        return {}
    values = {}
    for line in lines[1:]:
        if line.strip() == "---":
            break
        if ":" not in line or line.lstrip() != line:
            continue
        key, _, raw = line.partition(":")
        value = raw.strip().strip("'\"")
        if key.strip() in FRONTMATTER_KEYS and value:
            values[key.strip()] = value
    return values


class Budget:
    """One time budget for every probe of a session start (KAIROS-T-0525):
    each probe gets what is left of it, never less than `floor`, so three
    slow probes cannot add up past the timeout of the hook."""

    def __init__(self, seconds=3.0, floor=0.2, clock=time.monotonic):
        self.deadline = clock() + seconds
        self.floor = floor
        self.clock = clock

    def left(self):
        return max(self.floor, self.deadline - self.clock())


def probe(url, timeout=3):
    """Unauthenticated reachability check; returns a short status note."""
    try:
        with urllib.request.urlopen(url + "/healthz", timeout=timeout) as response:
            if response.status == 200:
                return "deployment reachable"
            return f"deployment answered /healthz with HTTP {response.status}"
    except Exception:
        return (
            "deployment NOT reachable right now (offline?) — "
            "local wiring below is still valid; live board state unavailable"
        )


def live_state_hint(values, board_folder=False):
    """The instruction that points the agent at live state over MCP. With a
    `repository` the session is repo-scoped (KAIROS-A-0019); without one it
    is board-scoped, exactly as before T-0108. In a board folder (clones,
    no checkout of its own: KAIROS-T-0521) there is no bootstrap advice."""
    repository = values.get("repository")
    # `team_board` is a board SLUG (what `board_items` resolves), per the
    # bootstrap contract.
    team_board = values.get("team_board") or "unset"
    # `adr_board` (COLLIERY-T-3102): the ADR board of the team, or the ADR
    # board of the organization when the team has none.
    adr_board = values.get("adr_board") or "unset"
    if repository:
        return (
            f"This checkout is repository `{repository}`. For live state, call the "
            f"kairos MCP tools now: `get_repository` for `{repository}` (read its "
            "\"How to work here\" description and in-flight PRs first). The result "
            "lists the documents and the ADRs that impact this repository (its "
            "vision, its architecture and the decisions about it) and the other "
            "ADRs of the owner team. Read them with "
            "`get_item` before you plan work. Then call "
            f"`board_items` for the board of your team ({team_board}) with "
            f"`repository={repository}`. That is your queue. The board without "
            "the filter shows all the work of your team. The work of other teams "
            "in this repository is not in your queue. Find it with `search` and "
            f"the filter `repository={repository}`. When you create a task, name "
            "a board in `board` (the CLI and REST also accept a team). The board "
            "decides the team. The repository is an optional link: "
            f"`repository={repository}` says where the code is and does not "
            "choose the board. Work for a different team is a request: name the "
            "delivery board of that team in `board`. The request goes to the "
            "entry column, in the support lane. An ADR about the work of your "
            "team goes on the ADR board of your team: `create_item` with "
            f"`item_type: adr` and `board={adr_board}`. The "
            "MCP client holds the authenticated session; this hook "
            "intentionally carries no credentials."
        )
    if board_folder:
        scope = (
            "No `repository` is wired: this is a board folder. Pick the task, "
            "then open the clone of its repository (the list above)."
        )
    else:
        scope = (
            "No `repository` is wired: re-run /kairos:bootstrap to detect it "
            "from the git remote and get a repo-scoped queue."
        )
    return (
        "For live board state, call the kairos MCP tools now: `my_boards`, "
        f"then `board_items` for the team board ({team_board}) to see "
        f"active/todo items and standing buckets. {scope} The MCP client "
        "holds the authenticated session; this hook intentionally carries no "
        "credentials."
    )


def server_version(url, timeout=3):
    """The version of the server (`GET /version`, open), or None."""
    try:
        with urllib.request.urlopen(url + "/version", timeout=timeout) as response:
            return json.load(response).get("version")
    except Exception:
        return None


def cli_version(timeout=3):
    """The version of the `kairos` CLI on PATH, or None."""
    binary = shutil.which("kairos")
    if not binary:
        return None
    try:
        out = subprocess.run(
            [binary, "--version"], capture_output=True, text=True, timeout=timeout
        ).stdout
    except Exception:
        return None
    parts = out.split()
    return parts[-1] if parts else None


def version_tuple(version):
    parts = []
    for part in version.lstrip("v").replace("-", ".").split(".")[:3]:
        parts.append(int(part) if part.isdigit() else 0)
    return tuple(parts)


def plugin_version():
    """The version of the plugin: `.claude-plugin/plugin.json` at
    `CLAUDE_PLUGIN_ROOT`, else two folders above this hook. None when it
    cannot be read."""
    root = os.environ.get("CLAUDE_PLUGIN_ROOT")
    if not root:
        here = os.path.dirname(os.path.abspath(__file__))
        root = os.path.dirname(os.path.dirname(here))
    try:
        with open(os.path.join(root, ".claude-plugin", "plugin.json"), encoding="utf-8") as f:
            version = json.load(f).get("version")
    except (OSError, ValueError, AttributeError):
        return None
    return version if isinstance(version, str) and version else None


def cli_note(cli, server, plugin=None):
    """One line when the CLI is older than the server (KAIROS-T-0432) or
    than the plugin (KAIROS-T-0513), else None (pure; testable). An old CLI
    reads an old index schema, so the code tools of the plugin are off."""
    if not cli or not any(c.isdigit() for c in cli):
        return None
    behind = []
    if server and version_tuple(cli) < version_tuple(server):
        behind.append(f"the server is {server}")
    if plugin and version_tuple(cli) < version_tuple(plugin):
        behind.append(f"the plugin is {plugin}")
    if not behind:
        return None
    return (
        f"- cli: the kairos CLI is {cli}; {' and '.join(behind)}. "
        "Tell the user. Update the CLI: `kairos self-update` installs the "
        "version of the server. `kairos-code` runs the old binary until the "
        "session starts again: restart the session after the update "
        "(KAIROS-T-0526)."
    )


def build_context(values, status, note=None, checkouts=None):
    """The full additionalContext text (pure; testable). `checkouts`: the
    list of `board_checkouts` in a board folder."""
    lines = [
        "Kairos wiring for this repo (.claude/kairos.local.md; "
        "re-run /kairos:bootstrap to change):",
    ]
    for key in FRONTMATTER_KEYS:
        lines.append(f"- {key}: {values.get(key) or '(not set)'}")
    lines.append(f"- status: {status}")
    if note:
        lines.append(note)
    clones = checkouts_text(checkouts or [])
    if clones:
        lines.append(clones)
    if "NOT reachable" not in status and values.get("deployment_url"):
        lines.append(live_state_hint(values, board_folder=bool(clones)))
    return "\n".join(lines)


def rules_at_start(values, session):
    """The Kairos rules for the start of a wired session, and the marker
    that keeps the PreToolUse hook silent after (KAIROS-T-0525). Empty when
    the marker was there already or the session id is not usable."""
    import kairos_rules  # here, not at the top: kairos_rules imports this module

    if not kairos_rules.mark(session):
        return ""
    return kairos_rules.build_context(values.get("deployment_url", ""))


def main():
    try:
        payload = json.loads(sys.stdin.read() or "null")
    except ValueError:
        payload = None
    session = payload.get("session_id") if isinstance(payload, dict) else None
    values = wiring()
    if values is None:
        return  # not a Kairos-bootstrapped repo: stay silent

    deployment = values.get("deployment_url", "")
    budget = Budget()
    status = probe(deployment, budget.left()) if deployment else "no deployment_url configured"
    server = None
    if deployment and "NOT reachable" not in status:
        server = server_version(deployment, budget.left())
    note = cli_note(cli_version(budget.left()), server, plugin_version())

    project_dir = os.environ.get("CLAUDE_PROJECT_DIR") or os.getcwd()
    checkouts = None
    if deployment and "NOT reachable" not in status:
        checkouts = cli_checkouts(project_dir, deployment)
    if checkouts is None:
        checkouts = board_checkouts(project_dir)
    context = build_context(values, status, note, checkouts)
    rules = rules_at_start(values, session)
    if rules:
        context = context + "\n\n" + rules
    json.dump(
        {
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": context,
            }
        },
        sys.stdout,
    )


if __name__ == "__main__":
    main()
