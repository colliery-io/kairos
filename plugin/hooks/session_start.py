#!/usr/bin/env python3
"""SessionStart hook: inject the repo's Kairos wiring as session context.

Reads `.claude/kairos.local.md` (written by /kairos:bootstrap per
KAIROS-A-0014) and emits its contents — deployment, tenant, stream, boards —
as `additionalContext`, plus an instruction to pull live board state.

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


def main():
    sys.stdin.read()  # consume the hook input; nothing in it is needed
    project_dir = os.environ.get("CLAUDE_PROJECT_DIR") or os.getcwd()
    config_path = os.path.join(project_dir, ".claude", "kairos.local.md")
    if not os.path.isfile(config_path):
        return  # not a Kairos-bootstrapped repo: stay silent

    values = read_frontmatter(config_path)
    deployment = values.get("deployment_url", "")
    status = probe(deployment) if deployment else "no deployment_url configured"

    lines = [
        "Kairos wiring for this repo (.claude/kairos.local.md; "
        "re-run /kairos:bootstrap to change):",
    ]
    for key in FRONTMATTER_KEYS:
        lines.append(f"- {key}: {values.get(key) or '(not set)'}")
    lines.append(f"- status: {status}")
    if "NOT reachable" not in status and deployment:
        lines.append(
            "For live board state, call the kairos MCP tools now: `my_boards`, "
            f"then `board_items` for the team board "
            f"({values.get('team_board') or 'unset'}) to see active/todo items "
            "and standing buckets. The MCP client holds the authenticated "
            "session; this hook intentionally carries no credentials."
        )

    json.dump(
        {
            "hookSpecificOutput": {
                "hookEventName": "SessionStart",
                "additionalContext": "\n".join(lines),
            }
        },
        sys.stdout,
    )


if __name__ == "__main__":
    main()
