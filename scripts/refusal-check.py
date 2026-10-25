#!/usr/bin/env python3
"""refusal-check.py: the refusals of the server and of the CLI MCP servers
(KAIROS-T-0690, pass 3 of KAIROS-D-0016).

A refusal names the field, the expected value and the next call. This check
reads the Rust source as text, with the scanner of `ste-check.py --code`. It
needs no Rust build and no database.

It finds each refusal that the code makes:

  * each call of an `ApiError` constructor in the server (not `internal`: its
    text goes to the log), and of the helpers `validation(` and
    `field_invalid(`;
  * each `#[error("...")]` attribute in the scope.

The rules:

  LIB    The text comes from a library: a placeholder or an argument that is
         a foreign error (`{e}`, `{err}`, `{source}`, `{serde_message}`,
         `e.to_string()`), a field of a foreign error type (`std::io::Error`,
         `serde_json::Error`, `reqwest::Error`, diesel, sqlx), the words
         serde, sqlx, reqwest or diesel, or an HTML body.
  FIELD  A VALIDATION or NOT_FOUND refusal (`ApiError::validation`,
         `ApiError::not_found`, the helper `validation(`) has no
         `.with_details(...)` with `field` (or `parameter` for a query
         parameter, `header` for a header, KAIROS-T-0698).

BASELINED, as `ste-check.py`: `scripts/refusal-baseline.json` holds the
count per file and rule, and the check fails only when a count gets higher.
A new file must be clean. The baseline only goes down.

  NEXT   The text names no next call: no tool, no CLI command, no REST
         route, and no sentence that starts with a verb of an instruction.
         This rule is a list for review (`--next`), not a failure.

WHAT IT CANNOT SEE: a text that the code puts in a variable first, a text
that a helper makes from its arguments (except `validation` and
`field_invalid`), a `field` that a helper adds, and the result texts of the
tools.

  refusal-check.py             gate against the baseline
  refusal-check.py --list      print each LIB and FIELD finding, with the line
  refusal-check.py --next      print each refusal that names no next call
  refusal-check.py --all       print each refusal that the check finds
  refusal-check.py --baseline  rewrite scripts/refusal-baseline.json
"""

import importlib.util
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parent
BASELINE_PATH = HERE / "refusal-baseline.json"

_SPEC = importlib.util.spec_from_file_location("ste_check", HERE / "ste-check.py")
ste = importlib.util.module_from_spec(_SPEC)
_SPEC.loader.exec_module(ste)

# The server, and the two MCP servers of the CLI: the code index
# (`kairos-cli/src/index`, with `arguments.rs`) and the proxy.
SCOPE = [
    "crates/kairos-server/src",
    "crates/kairos-cli/src/index",
    "crates/kairos-cli/src/commands/mcp_proxy.rs",
]
CONSTRUCTOR = re.compile(r"\bApiError::([a-z_]+)\s*\(")
# The refusals of the code tools of the CLI (`kairos-cli/src/index`).
TOOL_ERROR = re.compile(r"\bToolError::([A-Z][A-Za-z]+)\s*[({]")
HELPER = re.compile(r"(?<![\w:.])(validation|field_invalid)\s*\(")
NOT_FOR_THE_USER = {"internal", "with_details"}
NEEDS_FIELD = {"validation", "not_found"}

# A name that holds the error or the text of a library.
FOREIGN_NAMES = r"(?:e|err|error|source|cause|serde_message|serde_error|rejection)"
# These names hold the text of a library wherever they are: serde, and the
# rejections of axum.
ALWAYS_FOREIGN = {"serde_message": "serde", "serde_error": "serde", "rejection": "axum"}
FOREIGN_PLACEHOLDER_NAME = re.compile(r"\{(" + FOREIGN_NAMES + r")(?::[^}]*)?\}")
FOREIGN_ARGUMENT = re.compile(
    r"(?:^|[,(]\s*)(" + FOREIGN_NAMES + r")(?:\.to_string\(\))?\s*(?:[,)]|$)"
)
FOREIGN_TO_STRING = re.compile(r"\b(" + FOREIGN_NAMES + r")\.to_string\(\)")
# A call whose error comes from a library: the expression before the
# `map_err(|e| ...)` or the `match` that binds the error.
FOREIGN_CALL = re.compile(
    r"(serde_json::|serde::|\bfrom_str\b|\bfrom_slice\b|\bfrom_value\b|parse_str\b"
    r"|\.parse\s*(?:::<[^>]*>)?\s*\(|chrono::|NaiveDate|DateTime::parse|reqwest|sqlx|diesel::"
    r"|\.send\(\)|\.json\b|\.text\(\)|std::fs|tokio::fs|std::io|base64|Url::parse|url::"
    r"|rusqlite|zip::|flate2|tar::|hex::|toml::|Regex::new|read_to_string|\.decode\()"
)


def foreign_binding(source, position, name):
    """The library call whose error the name `name` holds at `position`, or
    None: the nearest binder before the position (`|name|`, `Err(name)`),
    and the statement before it."""
    before = source[:position]
    binder = None
    for pattern in (r"\|\s*" + name + r"\s*\|", r"\bErr\(\s*" + name + r"\s*\)"):
        for found in re.finditer(pattern, before):
            if binder is None or found.start() > binder:
                binder = found.start()
    if binder is None or position - binder > 1500:
        return None
    start = max(before.rfind(";", 0, binder), before.rfind("{\n", 0, binder), binder - 600)
    found = FOREIGN_CALL.search(before[start:binder])
    return found.group(1).strip("\\.(") if found else None
FOREIGN_WORD = re.compile(r"\b(?:serde|sqlx|reqwest|diesel)\b", re.IGNORECASE)
HTML = re.compile(r"<(?:html|!doctype|body|head)\b", re.IGNORECASE)
FOREIGN_TYPE = re.compile(
    r"(?:std::io::Error|\bio::Error|serde_json::Error|serde::de::Error|reqwest::Error"
    r"|diesel::|sqlx::|jsonwebtoken::|axum::|rmcp::|rusqlite::|uuid::Error|ParseError"
    r"|Box<dyn\s+std::error::Error)"
)

VERBS = (
    "add ask call change choose close create delete give make move open pick put "
    "read rebase remove reopen restore retry run send set sign start stop try use "
    "wait write archive cancel claim link get"
).split()
INSTRUCTION = re.compile(r"(?:^|[.:]\s+|\bthen\s+)(?:" + "|".join(VERBS) + r")\b", re.IGNORECASE)
ROUTE = re.compile(r"\b(?:GET|POST|PUT|PATCH|DELETE) /|`kairos |\bkairos [a-z]+ ")


def tool_names():
    """The names of the MCP tools of the server and of the code index."""
    names = set()
    for rel in ("crates/kairos-server/src/mcp/tools.rs", "crates/kairos-cli/src/index/mcp.rs"):
        path = REPO_ROOT / rel
        if not path.exists():
            continue
        waiting = False
        for line in path.read_text(encoding="utf-8").splitlines():
            if re.match(r"^\s*#\[tool\(", line):
                waiting = True
            elif waiting:
                found = re.match(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+(\w+)", line)
                if found:
                    names.add(found.group(1))
                    waiting = False
    return names


def without_tests(source):
    """The source with each `#[cfg(test)]` module blanked, so that the line
    numbers stay."""
    literals = ste.rust_literals(source)
    out = source
    for match in re.finditer(r"#\[cfg\(test\)\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{", source):
        open_brace = match.end() - 1
        close = ste.call_end(source, open_brace, literals)
        blank = re.sub(r"[^\n]", " ", source[match.start():close + 1])
        out = out[:match.start()] + blank + out[close + 1:]
    return out


def literal_texts(literals, start, end):
    """The string literals between two positions, as one text."""
    return " ".join(value for s, _, value in literals if start <= s < end)


# The keys of `details` that name the input of a refusal, and the helpers
# that add one.
FIELD_DETAIL = re.compile(r'"(?:field|parameter|header)"|\bfault\.details\(')


def has_field_detail(source, close, literals):
    """Whether `.with_details(... "field" ...)` follows the call that ends at
    `close`."""
    rest = source[close + 1:]
    found = re.match(r"\s*\.with_details\s*\(", rest)
    if not found:
        return False
    open_paren = close + 1 + found.end() - 1
    end = ste.call_end(source, open_paren, literals)
    # KAIROS-T-0697: `fault.details(...)` of `body::Fault` gives `field`.
    # KAIROS-T-0698: a query parameter is `parameter` and a header is
    # `header` (the conventions of input.rs and of the Idempotency-Key).
    return re.search(FIELD_DETAIL, source[open_paren:end]) is not None


def refusals(rel, source):
    """Each refusal of one file, as a dict: line, kind, text, lib, field."""
    source = without_tests(source)
    literals = ste.rust_literals(source)
    in_literal = [(s, e) for s, e, _ in literals]

    def inside_literal(position):
        return any(s <= position < e for s, e in in_literal)

    out = []
    calls = [(m, m.group(1)) for m in CONSTRUCTOR.finditer(source)]
    calls += [(m, m.group(1)) for m in HELPER.finditer(source)]
    if rel.startswith("crates/kairos-cli/"):
        calls += [(m, m.group(1)) for m in TOOL_ERROR.finditer(source)]
    for match, name in calls:
        if name in NOT_FOR_THE_USER or inside_literal(match.start()):
            continue
        if re.search(r"\bfn\s+$", source[:match.start()][-8:]):
            continue
        if name in ("validation", "field_invalid") and not match.group(0).startswith("ApiError"):
            if not rel.startswith("crates/kairos-server/"):
                continue
        open_paren = match.end() - 1
        close = ste.call_end(source, open_paren, literals)
        span = source[open_paren + 1:close]
        if match.group(0).startswith("ToolError") and re.match(
            r"\s*(?:=>|\|)", source[close + 1:close + 8]
        ):
            continue
        text = literal_texts(literals, open_paren, close)
        code = re.sub(r'"(?:[^"\\]|\\.)*"', '""', span)
        lib = []
        names = {m.group(1) for m in FOREIGN_PLACEHOLDER_NAME.finditer(text)}
        names |= {m.group(1) for m in FOREIGN_ARGUMENT.finditer(code.strip())}
        names |= {m.group(1) for m in FOREIGN_TO_STRING.finditer(code)}
        for binder in sorted(names):
            source_of = ALWAYS_FOREIGN.get(binder) or foreign_binding(source, match.start(), binder)
            if source_of:
                lib.append(f"{binder} is the error of {source_of}")
        if FOREIGN_WORD.search(text):
            lib.append("the name of a library")
        if HTML.search(text):
            lib.append("HTML")
        needs_field = name in NEEDS_FIELD or (
            name == "new" and re.search(r'"(?:VALIDATION|NOT_FOUND)"', span) is not None
        )
        field = (not needs_field) or name == "field_invalid" or has_field_detail(
            source, close, literals
        )
        out.append({
            "line": source.count("\n", 0, match.start()) + 1,
            "kind": match.group(0).rstrip("({ ") if "::" in match.group(0) else name,
            "text": text,
            "lib": lib,
            "field": field,
        })

    for match in re.finditer(r"#\[error\(\s*", source):
        if inside_literal(match.start()):
            continue
        after = source[match.end():]
        line = source.count("\n", 0, match.start()) + 1
        # The variant or the struct after the attribute: to the next attribute.
        body_end = re.search(r"\n\s*#\[|\n\s*\}|\n\s*///|\n\n", after)
        body = after[: body_end.start() if body_end else 400]
        lib = []
        if after.startswith("transparent"):
            text = ""
            if FOREIGN_TYPE.search(body):
                lib.append("a transparent foreign error")
        else:
            hit = next(((s, v) for s, _, v in literals if s >= match.end()), None)
            if not hit or hit[0] != match.end():
                continue
            text = hit[1]
            if FOREIGN_WORD.search(text):
                lib.append("the name of a library")
            if HTML.search(text):
                lib.append("HTML")
            for placeholder in re.findall(r"\{([A-Za-z0-9_]+)(?::[^}]*)?\}", text):
                if placeholder.isdigit():
                    types = re.findall(r"\(([^()]*(?:\([^()]*\))?[^()]*)\)", body)
                    parts = types[0].split(",") if types else []
                    index = int(placeholder)
                    kind = parts[index] if index < len(parts) else ""
                else:
                    found = re.search(r"\b" + placeholder + r"\s*:\s*([^,\n}]+)", body)
                    kind = found.group(1) if found else ""
                if FOREIGN_TYPE.search(kind):
                    lib.append(f"{{{placeholder}}} is a foreign error")
        out.append({"line": line, "kind": "#[error]", "text": text, "lib": lib, "field": True})
    return sorted(out, key=lambda r: r["line"])


def names_next_call(text, tools):
    if not text.strip():
        return True
    if any(re.search(r"\b" + re.escape(tool) + r"\b", text) for tool in tools):
        return True
    return bool(INSTRUCTION.search(text) or ROUTE.search(text))


def files():
    for entry in SCOPE:
        root = REPO_ROOT / entry
        if root.is_file():
            yield root
        elif root.is_dir():
            yield from sorted(root.rglob("*.rs"))


def scan():
    """Each refusal of the scope, as (rel, refusal)."""
    for path in files():
        rel = str(path.relative_to(REPO_ROOT))
        for refusal in refusals(rel, path.read_text(encoding="utf-8")):
            yield rel, refusal


def counts_of(found):
    counts = {}
    for rel, refusal in found:
        entry = counts.setdefault(rel, {"LIB": 0, "FIELD": 0})
        if refusal["lib"]:
            entry["LIB"] += 1
        if not refusal["field"]:
            entry["FIELD"] += 1
    return {rel: c for rel, c in sorted(counts.items()) if c["LIB"] or c["FIELD"]}


def main(argv):
    found = list(scan())
    if "--all" in argv:
        for rel, r in found:
            flags = ",".join((["LIB"] if r["lib"] else []) + ([] if r["field"] else ["FIELD"]))
            print(f"{rel}:{r['line']}: {r['kind']} [{flags}] {r['text'][:120]}")
        print(f"TOTAL {len(found)} refusals")
        return 0
    if "--next" in argv:
        tools = tool_names()
        missing = [(rel, r) for rel, r in found if not names_next_call(r["text"], tools)]
        for rel, r in missing:
            print(f"{rel}:{r['line']}: {r['kind']} {r['text'][:160]}")
        print(f"{len(missing)} of {len(found)} refusals name no next call (for review).")
        return 0
    if "--list" in argv:
        for rel, r in found:
            if r["lib"]:
                print(f"{rel}:{r['line']}: LIB ({'; '.join(r['lib'])}) {r['text'][:120]}")
            if not r["field"]:
                print(f"{rel}:{r['line']}: FIELD {r['kind']} {r['text'][:120]}")
    counts = counts_of(found)
    if "--baseline" in argv:
        BASELINE_PATH.write_text(json.dumps(counts, indent=2) + "\n", encoding="utf-8")
        total = {rule: sum(c[rule] for c in counts.values()) for rule in ("LIB", "FIELD")}
        print(f"baseline rewritten: {len(counts)} files, LIB {total['LIB']}, FIELD {total['FIELD']}")
        return 0
    baseline = {}
    if BASELINE_PATH.exists():
        baseline = json.loads(BASELINE_PATH.read_text(encoding="utf-8"))
    worse, better = [], []
    for rel in sorted(set(counts) | set(baseline)):
        for rule in ("LIB", "FIELD"):
            now = counts.get(rel, {}).get(rule, 0)
            was = baseline.get(rel, {}).get(rule, 0)
            if now > was:
                worse.append(f"  {rel}: {rule} {was} -> {now}")
            elif now < was:
                better.append(f"  {rel}: {rule} {was} -> {now}")
    total = {rule: sum(c[rule] for c in counts.values()) for rule in ("LIB", "FIELD")}
    print(
        f"refusal-check: {len(found)} refusals; LIB {total['LIB']}, FIELD {total['FIELD']} "
        f"in {len(counts)} file(s)."
    )
    if better:
        print("\nbetter than baseline (run --baseline to lock it in):")
        print("\n".join(better))
    if worse:
        print("\nWORSE than baseline:", file=sys.stderr)
        print("\n".join(worse), file=sys.stderr)
        print(
            "\nRun `scripts/refusal-check.py --list` to see each finding. Build the text "
            "of a refusal from your own words, not from the error of a library, and give "
            "a VALIDATION or NOT_FOUND refusal `.with_details(json!({\"field\": ...}))`.",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
