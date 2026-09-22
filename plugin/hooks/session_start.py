#!/usr/bin/env python3
"""SessionStart hook: inject the repo's Kairos wiring as session context.

Reads `.claude/kairos.local.md` (written by /kairos:bootstrap per
KAIROS-A-0014) and emits its contents — deployment, tenant, repository,
stream, boards — as `additionalContext`, plus an instruction to pull live
state. With a `repository` (KAIROS-A-0019, T-0108) the hint is
repo-scoped: read the repository (its "how to work here" description and
in-flight PRs), then the team board narrowed to that repository — the
session's queue. Without one it stays board-scoped as before.

v1 design decision (KAIROS-T-0034): hooks have no OAuth token broker —
tokens live with the MCP client (A-0014), and a shell hook cannot drive the
client's browser OAuth flow (A-0011). So this hook NEVER authenticates and
holds no secrets. It performs one unauthenticated reachability probe
(`GET <deployment_url>/healthz`, short timeout) and then instructs the
agent to call `my_boards` / `board_items` over the already-authenticated
kairos MCP connection for live items. Offline degrades to a note, never a
failure.

Not configured (no kairos.local.md) -> silent exit 0. Python stdlib only.
"""

import json
import os
import sys
import urllib.request

FRONTMATTER_KEYS = (
    "deployment_url",
    "tenant",
    "repository",
    "delivery_stream",
    "team_board",
    "initiative_board",
)


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


def probe(url):
    """Unauthenticated reachability check; returns a short status note."""
    try:
        with urllib.request.urlopen(url + "/healthz", timeout=3) as response:
            if response.status == 200:
                return "deployment reachable"
            return f"deployment answered /healthz with HTTP {response.status}"
    except Exception:
        return (
            "deployment NOT reachable right now (offline?) — "
            "local wiring below is still valid; live board state unavailable"
        )


def live_state_hint(values):
    """The instruction that points the agent at live state over MCP. With a
    `repository` the session is repo-scoped (KAIROS-A-0019); without one it
    is board-scoped, exactly as before T-0108."""
    repository = values.get("repository")
    # `team_board` is a board SLUG (what `board_items` resolves), per the
    # bootstrap contract.
    team_board = values.get("team_board") or "unset"
    if repository:
        return (
            f"This checkout is repository `{repository}`. For live state, call the "
            f"kairos MCP tools now: `get_repository` for `{repository}` (read its "
            "\"How to work here\" description and in-flight PRs first), then "
            f"`board_items` for the team board ({team_board}) with "
            f"`repository={repository}` — that is your queue; the unfiltered "
            "board is the wider team lens. Tasks you create for this codebase "
            f"take `repository={repository}`. Work bound to ANOTHER repository "
            "belongs in that checkout: file it there with `create_item` "
            "(`repository` + `parent`/`blocks`), do not implement it here. The "
            "MCP client holds the authenticated session; this hook "
            "intentionally carries no credentials."
        )
    return (
        "For live board state, call the kairos MCP tools now: `my_boards`, "
        f"then `board_items` for the team board ({team_board}) to see "
        "active/todo items and standing buckets. No `repository` is wired: "
        "re-run /kairos:bootstrap to detect it from the git remote and get a "
        "repo-scoped queue. The MCP client holds the authenticated session; "
        "this hook intentionally carries no credentials."
    )


def build_context(values, status):
    """The full additionalContext text (pure; testable)."""
    lines = [
        "Kairos wiring for this repo (.claude/kairos.local.md; "
        "re-run /kairos:bootstrap to change):",
    ]
    for key in FRONTMATTER_KEYS:
        lines.append(f"- {key}: {values.get(key) or '(not set)'}")
    lines.append(f"- status: {status}")
    if "NOT reachable" not in status and values.get("deployment_url"):
        lines.append(live_state_hint(values))
    return "\n".join(lines)


def main():
    sys.stdin.read()  # consume the hook input; nothing in it is needed
    project_dir = os.environ.get("CLAUDE_PROJECT_DIR") or os.getcwd()
    config_path = os.path.join(project_dir, ".claude", "kairos.local.md")
    if not os.path.isfile(config_path):
        return  # not a Kairos-bootstrapped repo: stay silent

    values = read_frontmatter(config_path)
    deployment = values.get("deployment_url", "")
    status = probe(deployment) if deployment else "no deployment_url configured"

    json.dump(
        {
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": build_context(values, status),
            }
        },
        sys.stdout,
    )


if __name__ == "__main__":
    main()
