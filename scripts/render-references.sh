#!/usr/bin/env bash
# render-references.sh — render plugin/references/*.md from their Metis specification sources.
#
# The Metis specs (KAIROS-S-0007, KAIROS-S-0008) are the source of truth (KAIROS-A-0014).
# Rendering:
#   1. strips the YAML frontmatter block,
#   2. strips a leading "## KAIROS-S-000x: ..." header line if present,
#   3. keeps the "# <Title>" H1 and everything after,
#   4. replaces the sentence referencing rendering/Metis-source with a
#      "Rendered from <CODE> (source of truth) — do not edit here." marker.
#
# Idempotent and reproducible: output depends only on the source specs. Run from anywhere.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

render() {
  local src="$1" dst="$2" code="$3"
  python3 - "$src" "$dst" "$code" <<'PY'
import re
import sys

src, dst, code = sys.argv[1:4]
with open(src, encoding="utf-8") as f:
    text = f.read()

# 1. Strip the YAML frontmatter block.
m = re.match(r"\A---\n.*?\n---\n", text, re.DOTALL)
if m:
    text = text[m.end():]

# 2. Strip a "## KAIROS-S-000x: ..." Metis header line if present.
text = re.sub(r"^## KAIROS-S-\d+:.*\n", "", text, count=1, flags=re.MULTILINE)

# 3. Keep the H1 and everything after (drop leading blank lines).
text = text.lstrip("\n")

# 4. Replace the rendering/Metis-source sentence with the rendered-artifact marker.
text = re.sub(
    r"It renders into the skills plugin as .*?source of truth\.",
    f"Rendered from {code} (source of truth) — do not edit here.",
    text,
    count=1,
)

with open(dst, "w", encoding="utf-8") as f:
    f.write(text)
PY
  echo "rendered $dst from $src"
}

mkdir -p "$REPO_ROOT/plugin/references"

render "$REPO_ROOT/.metis/specifications/KAIROS-S-0007/specification.md" \
       "$REPO_ROOT/plugin/references/architecture-review.md" \
       "KAIROS-S-0007"

render "$REPO_ROOT/.metis/specifications/KAIROS-S-0008/specification.md" \
       "$REPO_ROOT/plugin/references/diataxis.md" \
       "KAIROS-S-0008"
