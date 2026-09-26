#!/usr/bin/env bash
# render-references.sh — render plugin/references/*.md from their Kairos documents.
#
# The source of truth (KAIROS-A-0014) is three documents in the Kairos deployment this
# repository is wired to. They were Metis specifications until 2026-09-26, and the rendered
# marker names both codes so an older citation still resolves:
#
#   COLLIERY-D-0007  (was KAIROS-S-0007)  ->  plugin/references/architecture-review.md
#   COLLIERY-D-0008  (was KAIROS-S-0008)  ->  plugin/references/diataxis.md
#   COLLIERY-D-0009  (was KAIROS-S-0009)  ->  plugin/references/simplified-technical-english.md
#
# Rendering:
#   1. fetches the document's markdown over the REST API,
#   2. strips the migration footer the Metis import appended,
#   3. keeps the "# <Title>" H1 and everything after,
#   4. replaces the sentence naming the source of truth with a
#      "Rendered from <CODE> ... do not edit here." marker.
#
# Reproducible: output depends only on the documents. Needs the deployment reachable and a
# bearer that can read them:
#
#   KAIROS_URL   defaults to `deployment_url` in .claude/kairos.local.md
#   KAIROS_KEY   defaults to $KAIROS_MCP_KEY (what the Claude Code MCP client uses)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WIRING="$REPO_ROOT/.claude/kairos.local.md"

if [ -z "${KAIROS_URL:-}" ] && [ -f "$WIRING" ]; then
  KAIROS_URL="$(sed -n 's/^deployment_url:[[:space:]]*//p' "$WIRING" | head -1)"
fi
KAIROS_KEY="${KAIROS_KEY:-${KAIROS_MCP_KEY:-}}"
if [ -z "${KAIROS_URL:-}" ] || [ -z "$KAIROS_KEY" ]; then
  echo "render-references: set KAIROS_URL and KAIROS_KEY (or run /kairos:bootstrap and set KAIROS_MCP_KEY)" >&2
  exit 2
fi
export KAIROS_URL KAIROS_KEY

render() {
  local code="$1" dst="$2" was="$3"
  python3 - "$code" "$dst" "$was" <<'PY'
import json
import os
import re
import sys
import urllib.request

code, dst, was = sys.argv[1:4]
req = urllib.request.Request(
    os.environ["KAIROS_URL"].rstrip("/") + "/api/documents/" + code,
    headers={"authorization": "Bearer " + os.environ["KAIROS_KEY"], "accept": "application/json"},
)
with urllib.request.urlopen(req, timeout=30) as resp:
    text = json.load(resp)["content"]

# 2. Strip the migration footer, if this document came from Metis.
text = re.sub(r"\n---\n\n\*Migrated from Metis `[^`]+`.*\Z", "\n", text, flags=re.DOTALL)

# 3. Keep the H1 and everything after (drop leading blank lines).
text = text.lstrip("\n").rstrip("\n") + "\n"

# 4. Replace the source-of-truth sentence with the rendered-artifact marker.
text = re.sub(
    r"It renders into the skills plugin as .*?source of truth\.",
    f"Rendered from {code} (source of truth; was Metis {was}) — do not edit here.",
    text,
    count=1,
    flags=re.DOTALL,
)

with open(dst, "w", encoding="utf-8") as f:
    f.write(text)
print(f"rendered {dst} from {code}")
PY
}

mkdir -p "$REPO_ROOT/plugin/references"

render "COLLIERY-D-0007" "$REPO_ROOT/plugin/references/architecture-review.md" "KAIROS-S-0007"
render "COLLIERY-D-0008" "$REPO_ROOT/plugin/references/diataxis.md" "KAIROS-S-0008"
render "COLLIERY-D-0009" "$REPO_ROOT/plugin/references/simplified-technical-english.md" "KAIROS-S-0009"
