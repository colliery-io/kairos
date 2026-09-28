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

`--code` (COLLIERY-T-0258) applies the same rules to the texts of errors in the Rust
code, in place of the book. It reads the source as text and needs no Rust build. The
texts come from two places:

  * each `#[error("...")]` attribute in `CODE_SCOPE`, and
  * each string literal that is a direct argument of an `ApiError::...(` or
    `Self::...(` constructor, or the format string of a `format!(` in it.

WHAT `--code` CANNOT SEE is as important. A text that the code puts in a variable
first, a text that a helper function makes from its arguments, the result texts of the
MCP tools, the CLI texts and the GUI texts are all outside it. `CODE_HELPERS` names
the helper functions that take a text directly.

  ste-check.py --code             gate the texts of the code against their baseline
  ste-check.py --code --list      print every violation, with file and line
  ste-check.py --code --baseline  rewrite scripts/ste-code-baseline.json
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


def violations_in_block(block, max_words=MAX_SENTENCE_WORDS):
    """Every violation in one block of prose, as (rule, what, where).

    `max_words` is a number, or a function from a sentence to its limit.
    """
    found = []
    sents = sentences(block)
    if len(sents) > MAX_PARAGRAPH_SENTENCES:
        found.append(("STE-S4", f"paragraph of {len(sents)} sentences", block[:60]))
    for sentence in sents:
        words = [w for w in re.findall(r"[A-Za-z0-9'’/._-]+", sentence)]
        limit = max_words(sentence) if callable(max_words) else max_words
        if len(words) > limit:
            rule = "STE-S2" if limit == MAX_SENTENCE_WORDS else "STE-S3"
            found.append((rule, f"{len(words)} words", sentence[:80]))
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


def violations_in(path):
    """Every violation in one file, as (rule, line-ish, message)."""
    found = []
    text = path.read_text(encoding="utf-8")
    for block in prose_blocks(text):
        found.extend(violations_in_block(block))
    return found


# --- `--code`: the texts of errors in the Rust code (COLLIERY-T-0258) -----------------

CODE_BASELINE_PATH = REPO_ROOT / "scripts" / "ste-code-baseline.json"
# Where a text of an error that a user can see starts. `#[error]` attributes are read
# in each of these. Constructor calls are read in the server only.
CODE_SCOPE = [
    "crates/kairos-core/src",
    "crates/kairos-db/src",
    "crates/kairos-server/src/api",
    "crates/kairos-server/src/mcp",
    "crates/kairos-server/src/middleware",
    "crates/kairos-server/src/service_accounts",
    "crates/kairos-server/src/scim/tokens.rs",
    "crates/kairos-server/src/body.rs",
    "crates/kairos-server/src/error.rs",
    "crates/kairos-server/src/input.rs",
    "crates/kairos-server/src/local_auth.rs",
    "crates/kairos-server/src/login.rs",
    "crates/kairos-server/src/web.rs",
]
# A call of one of these has a text as a direct argument.
CODE_CONSTRUCTOR = re.compile(r"\b(?:ApiError|Self)::([a-z_]+)\s*\(")
# The text of `ApiError::internal` goes to the log. The user gets a fixed text.
NOT_FOR_THE_USER = {"internal"}
# `Self::` is the constructor of ApiError only in this file.
SELF_IS_API_ERROR = "crates/kairos-server/src/error.rs"
# Helper functions that take the text of an error directly.
CODE_HELPERS = re.compile(r"\b(field_invalid|validation|idp_unreachable)\s*\(")
# The code of an error (`VALIDATION`) and the name of a field (`team_id`) are not texts.
NOT_A_TEXT = re.compile(r"^[A-Za-z0-9_.:-]*$")
# STE-S2 and STE-S3 for a text of the code: 20 words for an instruction, 25 for a
# description. An instruction starts with a verb in the imperative.
MAX_DESCRIPTION_WORDS = 25
IMPERATIVE = {
    "add", "ask", "change", "close", "delete", "do", "get", "give", "make", "move",
    "open", "put", "read", "remove", "restore", "run", "send", "set", "sign", "start",
    "stop", "try", "use", "wait", "write",
}
CONTRACTION = re.compile(
    r"\b(?:\w+n't|\w+'(?:re|ll|ve|d)|(?:it|that|there|what|let|here|who)'s)\b",
    re.IGNORECASE,
)
LATIN = re.compile(r"\b(?:e\.g\.|i\.e\.|etc\.?|viz\.|vs\.?|via)(?=\W|$)", re.IGNORECASE)
DASH = re.compile(r"[—–]|\s-{1,2}\s")


def rust_literals(source):
    """Each string literal of a Rust source, as (start, end, value).

    A small scanner, not a parser: it knows comments, char literals, raw strings and
    the escapes of a normal string, and that is enough to find where a literal starts
    and ends.
    """
    out = []
    i, n = 0, len(source)
    while i < n:
        c = source[i]
        if source.startswith("//", i):
            j = source.find("\n", i)
            i = n if j < 0 else j
        elif source.startswith("/*", i):
            j = source.find("*/", i + 2)
            i = n if j < 0 else j + 2
        elif c == "r" and re.match(r'r#*"', source[i:i + 8]) and (
            i == 0 or not (source[i - 1].isalnum() or source[i - 1] == "_")
        ):
            hashes = len(re.match(r"r(#*)", source[i:]).group(1))
            start = i + 2 + hashes
            closer = '"' + "#" * hashes
            j = source.find(closer, start)
            j = n if j < 0 else j
            out.append((i, j + len(closer), source[start:j]))
            i = j + len(closer)
        elif c == '"':
            j, value = i + 1, []
            while j < n and source[j] != '"':
                if source[j] == "\\" and j + 1 < n:
                    nxt = source[j + 1]
                    if nxt == "\n":
                        # A line continuation: the newline and the indent go.
                        j += 2
                        while j < n and source[j] in " \t\n":
                            j += 1
                        continue
                    value.append({"n": "\n", "t": " "}.get(nxt, nxt))
                    j += 2
                else:
                    value.append(source[j])
                    j += 1
            out.append((i, j + 1, "".join(value)))
            i = j + 1
        elif c == "'":
            # A char literal ('"', '\'') or a lifetime ('a). Skip a char literal.
            m = re.match(r"'(?:\\.[^']*|[^'\\])'", source[i:])
            i += len(m.group(0)) if m else 1
        else:
            i += 1
    return out


def call_end(source, open_paren, literals):
    """The index of the `)` that closes the `(` at `open_paren`."""
    skip = {start: end for start, end, _ in literals}
    depth, i, n = 0, open_paren, len(source)
    while i < n:
        if i in skip:
            i = skip[i]
            continue
        if source[i] in "([{":
            depth += 1
        elif source[i] in ")]}":
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return n


def code_texts(rel, source):
    """The texts of errors in one file, as (line, text)."""
    literals = rust_literals(source)
    ends = {start: end for start, end, _ in literals}
    values = {start: value for start, _, value in literals}
    found = {}

    def first_literal_after(position):
        for start, _, value in literals:
            if start >= position:
                return start, value
        return None

    for match in re.finditer(r"#\[error\(\s*", source):
        hit = first_literal_after(match.end())
        if hit and hit[0] == match.end():
            found[hit[0]] = hit[1]

    if rel.startswith("crates/kairos-server/"):
        calls = [
            m for m in CODE_CONSTRUCTOR.finditer(source)
            if m.group(0).startswith("ApiError") or rel == SELF_IS_API_ERROR
        ]
        calls += list(CODE_HELPERS.finditer(source))
        for match in calls:
            if match.group(1) in NOT_FOR_THE_USER:
                continue
            if re.search(r"\bfn\s+$", source[:match.start()][-8:]):
                continue
            open_paren = match.end() - 1
            close = call_end(source, open_paren, literals)
            # One frame for each open bracket. A frame is True for a `format!(` that
            # still waits for its format string, and False for each other bracket.
            frames, i = [], open_paren
            while i <= close:
                if i in ends:
                    direct = len(frames) == 1
                    if direct or frames[-1]:
                        found[i] = values[i]
                        frames[-1] = False
                    i = ends[i]
                    continue
                if source.startswith("format!(", i):
                    frames.append(True)
                    i += len("format!(")
                    continue
                if source[i] in "([{":
                    frames.append(False)
                elif source[i] in ")]}" and frames:
                    frames.pop()
                i += 1

    texts = []
    for position in sorted(found):
        value = found[position]
        if NOT_A_TEXT.match(value):
            continue
        line = source.count("\n", 0, position) + 1
        texts.append((line, value))
    return texts


def code_prose(text):
    """The text as prose: a placeholder or a quoted name is one technical noun."""
    text = text.replace("{{", "(").replace("}}", ")")
    text = re.sub(r"\{[^{}]*\}", "CODE", text)
    text = re.sub(r"`[^`]*`", "CODE", text)
    text = re.sub(r'"[^"]*"', "CODE", text)
    return re.sub(r"\s+", " ", text).strip()


def sentence_limit(sentence):
    first = re.match(r"[A-Za-z]+", sentence)
    if first and first.group(0).lower() in IMPERATIVE:
        return MAX_SENTENCE_WORDS
    return MAX_DESCRIPTION_WORDS


def violations_in_code_text(text):
    """Every violation in one text of the code, as (rule, what, where)."""
    prose = code_prose(text)
    found = violations_in_block(prose, sentence_limit)
    where = prose[:80]
    first = prose[:1]
    first_word = prose.split(" ", 1)[0]
    technical = first_word.startswith("CODE") or "_" in first_word
    if not technical and not first.isupper():
        found.append(("STE-C1", "the text does not start with a capital", where))
    # A placeholder after the end of a sentence is a sentence that the code made.
    if not prose.endswith((".", ".)", ". CODE", ".CODE")):
        found.append(("STE-C2", "the text does not end with a period", where))
    for match in CONTRACTION.finditer(prose):
        found.append(("STE-C3", f"contraction: {match.group(0)}", where))
    if ";" in prose:
        found.append(("STE-C4", "semicolon", where))
    if DASH.search(prose) or "->" in prose:
        found.append(("STE-C5", "dash or arrow as punctuation", where))
    for match in LATIN.finditer(prose):
        found.append(("STE-C6", f"Latin abbreviation: {match.group(0)}", where))
    if re.search(r"\w\(s\)", prose):
        found.append(("STE-C7", 'plural in parentheses: write the plural', where))
    return found


def code_files():
    for entry in CODE_SCOPE:
        root = REPO_ROOT / entry
        if root.is_file():
            yield root
        elif root.is_dir():
            for path in sorted(root.rglob("*.rs")):
                yield path


def code_main(list_all, rewrite, texts_only):
    counts, detail, total_texts = {}, {}, 0
    for path in code_files():
        rel = str(path.relative_to(REPO_ROOT))
        source = path.read_text(encoding="utf-8")
        # The tests of a module quote texts, and they are not texts of the product.
        cut = source.find("#[cfg(test)]\nmod tests")
        if cut >= 0:
            source = source[:cut]
        for line, text in code_texts(rel, source):
            total_texts += 1
            if texts_only:
                print(f"{rel}:{line}: {text}")
                continue
            found = violations_in_code_text(text)
            if found:
                counts[rel] = counts.get(rel, 0) + len(found)
                detail.setdefault(rel, []).extend(
                    (rule, f"line {line}: {what}", where) for rule, what, where in found
                )
    if texts_only:
        print(f"TOTAL {total_texts}")
        return 0

    if rewrite:
        CODE_BASELINE_PATH.write_text(
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
    if CODE_BASELINE_PATH.exists():
        baseline = json.loads(CODE_BASELINE_PATH.read_text(encoding="utf-8"))
    worse, improved = [], []
    for rel in sorted(set(counts) | set(baseline)):
        now, was = counts.get(rel, 0), baseline.get(rel, 0)
        if now > was:
            worse.append((rel, was, now))
        elif now < was:
            improved.append((rel, was, now))

    print(
        f"STE for the texts of the code (COLLIERY-T-0258): {total_texts} texts, "
        f"{sum(counts.values())} mechanical violations in {len(counts)} file(s); "
        f"baseline allows {sum(baseline.values())}."
    )
    print(
        "  NOT CHECKED: STE-V1, the approved vocabulary, and each text that is not an\n"
        "  `#[error]` attribute or a direct argument of an ApiError constructor."
    )
    if improved:
        print("\nbetter than baseline (run --code --baseline to lock it in):")
        for rel, was, now in improved:
            print(f"  {rel}: {was} -> {now}")
    if worse:
        print("\nWORSE than baseline:", file=sys.stderr)
        for rel, was, now in worse:
            print(f"  {rel}: {was} -> {now}", file=sys.stderr)
        print(
            "\nA text of an error follows KAIROS-S-0009 (plugin/references/"
            "simplified-technical-english.md).\n"
            "Run `angreal docs ste --code --list` to see each violation with its rule "
            "ID.\nThe baseline is a ceiling that only goes down: fix the new "
            "violations rather than raising it.",
            file=sys.stderr,
        )
        return 1
    return 0


def main(argv):
    list_all = "--list" in argv
    rewrite = "--baseline" in argv
    if "--code" in argv:
        return code_main(list_all, rewrite, "--texts" in argv)

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
