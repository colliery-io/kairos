#!/usr/bin/env python3
"""ste-check.py — the mechanical half of KAIROS-S-0009 (ASD-STE100).

Reads `docs/src/{tutorials,how-to,reference}` and reports the rules a script can
actually check. What it does NOT check is as important as what it does, and
KAIROS-S-0009 section 6 is the contract:

  * STE-V1, the approved vocabulary, is NOT checked. It needs ASD's word list as
    data, and ASD owns its copyright, so this repository ships no copy. Section 2
    of the spec says so. A green run here is not STE conformance.
  * `explanation/`, ADRs, Status Updates, commit messages and code comments are out
    of scope entirely (section 1.2). STE is hostile to argument, and those files
    exist to argue. This script must never read them.

BASELINED, not big-bang. `scripts/ste-baseline.json` records the violation count per
file, and the gate fails only when a file gets WORSE. A file with no baseline entry
must be clean. That is how KAIROS-T-0207 applies the rule from today forward without a
rewrite nobody reviewed — and the baseline only ever goes down.

  ste-check.py             report and gate against the baseline
  ste-check.py --list      print every violation, with rule IDs
  ste-check.py --baseline  rewrite the baseline from the current tree
"""

import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DOCS = REPO_ROOT / "docs" / "src"
# Section 1.1. `explanation/` is deliberately absent.
SCOPE = ["tutorials", "how-to", "reference"]
BASELINE_PATH = REPO_ROOT / "scripts" / "ste-baseline.json"

# STE-S2: a procedural sentence is 20 words or fewer. Applied to every sentence in
# these directories, because every sentence in them is procedural or referential.
MAX_SENTENCE_WORDS = 20
# STE-S4.
MAX_PARAGRAPH_SENTENCES = 6

# STE-G1. A form of "to be" followed by a past participle. Deliberately narrow: it
# wants the obvious passives ("is set", "was created", "can be found") and would
# rather miss a clever one than cry wolf on "is available".
BE = r"(?:is|are|was|were|be|been|being|get|gets)"
PASSIVE = re.compile(
    rf"\b{BE}\s+(?:not\s+|also\s+|then\s+|only\s+|already\s+)?"
    r"([a-z]+(?:ed|en))\b",
    re.IGNORECASE,
)
# Past participles that are really adjectives or nouns here, so the narrow rule above
# does not fire on them. Each one earns its place by having been a false positive.
PASSIVE_ALLOW = {
    "based", "advanced", "detailed", "limited", "related", "unauthenticated",
    "authenticated", "needed", "intended", "supported", "unchanged", "used",
    "shared", "fixed", "named", "expected", "required", "combined", "connected",
    "provided", "deleted", "archived", "enabled", "disabled", "empty",
}

# STE-G4: two or more gerunds in one sentence. `-ing` words that are ordinary nouns
# are excluded, or every mention of a "setting" is a violation.
GERUND = re.compile(r"\b([a-z]{4,}ing)\b")
GERUND_ALLOW = {
    "setting", "settings", "string", "strings", "during", "nothing", "something",
    "anything", "everything", "thing", "things", "ring", "bring", "spring",
    "morning", "warning", "warnings", "meaning", "engineering", "onboarding",
    "housekeeping", "planning", "reporting", "tooling", "logging", "tracing",
    "missing", "existing", "following", "remaining", "according", "including",
    "leading", "trailing", "working", "running",
}

# STE-V2 / STE-V3: the "do not write" column of section 4.1. Only the unambiguous
# ones — "code" and "key" are too common in other meanings to flag, and a checker
# that cries wolf gets switched off.
BANNED_TERMS = {
    "org admin": "organization admin",
    "orgs": "organizations",
    "swimlane": "lane",
    "kanban": "board",
    "epic": "initiative",
    "ticket": "task",
    "squad": "team",
    "bot": "service account",
    "machine user": "service account",
    "idp": "issuer",
    "superuser": "deployment admin",
    "sysadmin": "deployment admin",
    "installation": "deployment",
}


def scoped_files():
    for directory in SCOPE:
        root = DOCS / directory
        if not root.is_dir():
            continue
        for path in sorted(root.rglob("*.md")):
            yield path


def prose_blocks(text):
    """Paragraphs of prose, with everything a rule cannot sensibly apply to removed.

    Fenced code, tables, headings, list markers, link targets, inline code and HTML
    comments all go: a rule about sentence length has no opinion about a `curl`
    invocation or a table row, and pretending otherwise produces noise rather than
    findings.
    """
    text = re.sub(r"```.*?```", "", text, flags=re.DOTALL)
    text = re.sub(r"<!--.*?-->", "", text, flags=re.DOTALL)
    def clean(chunk):
        chunk = re.sub(r"`[^`]*`", "CODE", chunk)
        chunk = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", chunk)
        chunk = re.sub(r"\*\*?([^*]*)\*\*?", r"\1", chunk)
        return chunk

    out = []
    for block in text.split("\n\n"):
        # A LIST ITEM IS ITS OWN BLOCK. Joining them made a six-link "Related"
        # section read as one 28-word sentence, which is a checker defect rather
        # than a finding — and a checker that cries wolf gets switched off. Each
        # item is a unit under STE-S1 too.
        paragraph = []
        for line in block.splitlines():
            stripped = line.strip()
            if not stripped or stripped.startswith(("#", "|", ">", "---", "    ")):
                continue
            item = re.sub(r"^(?:[-*+]|\d+\.)\s+", "", stripped)
            if item != stripped:
                # It started a list item: flush the paragraph, emit the item alone.
                if paragraph:
                    out.append(clean(" ".join(paragraph)))
                    paragraph = []
                out.append(clean(item))
            else:
                paragraph.append(stripped)
        if paragraph:
            out.append(clean(" ".join(paragraph)))
    return [b for b in out if b]


def sentences(block):
    # Abbreviations that would otherwise split a sentence.
    guarded = block.replace("e.g.", "eg").replace("i.e.", "ie")
    parts = re.split(r"(?<=[.!?])\s+(?=[A-Z(])", guarded)
    return [p.strip() for p in parts if p.strip()]


def violations_in(path):
    """Every violation in one file, as (rule, line-ish, message)."""
    found = []
    text = path.read_text(encoding="utf-8")
    for block in prose_blocks(text):
        sents = sentences(block)
        if len(sents) > MAX_PARAGRAPH_SENTENCES:
            found.append(("STE-S4", f"paragraph of {len(sents)} sentences", block[:60]))
        for sentence in sents:
            words = [w for w in re.findall(r"[A-Za-z0-9'’/._-]+", sentence)]
            if len(words) > MAX_SENTENCE_WORDS:
                found.append(("STE-S2", f"{len(words)} words", sentence[:80]))
            for match in PASSIVE.finditer(sentence):
                if match.group(1).lower() not in PASSIVE_ALLOW:
                    found.append(("STE-G1", f"passive: {match.group(0)}", sentence[:80]))
            gerunds = [
                g for g in GERUND.findall(sentence.lower())
                if g not in GERUND_ALLOW
            ]
            if len(gerunds) >= 2:
                found.append(("STE-G4", f"gerunds: {', '.join(gerunds[:3])}", sentence[:80]))
            lowered = sentence.lower()
            for banned, instead in BANNED_TERMS.items():
                if re.search(rf"\b{re.escape(banned)}\b", lowered):
                    found.append(
                        ("STE-V3", f'"{banned}" — write "{instead}"', sentence[:80])
                    )
    return found


def main(argv):
    list_all = "--list" in argv
    rewrite = "--baseline" in argv

    counts = {}
    detail = {}
    for path in scoped_files():
        rel = str(path.relative_to(REPO_ROOT))
        found = violations_in(path)
        if found:
            counts[rel] = len(found)
            detail[rel] = found

    if rewrite:
        BASELINE_PATH.write_text(
            json.dumps(dict(sorted(counts.items())), indent=2) + "\n", encoding="utf-8"
        )
        print(f"baseline rewritten: {len(counts)} files, {sum(counts.values())} violations")
        return 0

    if list_all:
        for rel in sorted(detail):
            print(f"\n{rel}")
            for rule, what, where in detail[rel]:
                print(f"  {rule}  {what}\n        {where}")

    baseline = {}
    if BASELINE_PATH.exists():
        baseline = json.loads(BASELINE_PATH.read_text(encoding="utf-8"))

    worse, improved = [], []
    for rel in sorted(set(counts) | set(baseline)):
        now, was = counts.get(rel, 0), baseline.get(rel, 0)
        if now > was:
            worse.append((rel, was, now))
        elif now < was:
            improved.append((rel, was, now))

    total = sum(counts.values())
    allowed = sum(baseline.values())
    print(
        f"STE (KAIROS-S-0009): {total} mechanical violations in "
        f"{len(counts)} file(s); baseline allows {allowed}."
    )
    print(
        "  NOT CHECKED: STE-V1, the approved vocabulary — ASD owns its copyright, so "
        "this repository ships no word list.\n"
        "  A clean run is not STE conformance. See KAIROS-S-0009 section 6."
    )

    if improved:
        print("\nbetter than baseline (run --baseline to lock it in):")
        for rel, was, now in improved:
            print(f"  {rel}: {was} -> {now}")

    if worse:
        print("\nWORSE than baseline:", file=sys.stderr)
        for rel, was, now in worse:
            print(f"  {rel}: {was} -> {now}", file=sys.stderr)
        print(
            "\nProcedural text follows KAIROS-S-0009 (plugin/references/"
            "simplified-technical-english.md).\n"
            "Run `angreal docs ste --list` to see each violation with its rule ID.\n"
            "The baseline is a ceiling that only goes down: fix the new violations "
            "rather than raising it.",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
