#!/usr/bin/env python3
"""ste-parity.py — the expected output of scripts/ste-check.py for the Rust port.

KAIROS-T-0727 ports the rules of ste-check.py into the crate kairos-plain. The
crate's test `tests/parity.rs` holds the Rust to the output of the Python. This
script writes that output; it imports ste-check.py and changes nothing in it.

  ste-parity.py                 read crates/kairos-plain/tests/fixtures/inputs.json,
                                write .../fixtures/expected.json
  ste-parity.py --corpus OUT    write each docs page in scope and each text of the
                                code, with its expected output, to OUT (for the
                                ignored test `corpus_parity`)
"""

import importlib.util
import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
FIXTURES = REPO_ROOT / "crates" / "kairos-plain" / "tests" / "fixtures"

spec = importlib.util.spec_from_file_location("ste_check", REPO_ROOT / "scripts" / "ste-check.py")
ste = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ste)


def expected(found):
    """(rule, what) pairs. The Rust message of STE-V3 has a colon, not a dash."""
    out = []
    for rule, what, _where in found:
        if rule == "STE-V3":
            what = what.replace(" — ", ": ") + "."
        out.append([rule, what])
    return out


def prose(text):
    found = []
    for block in ste.prose_blocks(text):
        found.extend(ste.violations_in_block(block))
    return expected(found)


def code(text):
    return expected(ste.violations_in_code_text(text))


def main(argv):
    if argv[:1] == ["--corpus"]:
        out = {"prose": [], "code": []}
        for path in ste.scoped_files():
            text = path.read_text(encoding="utf-8")
            out["prose"].append({"text": text, "expected": prose(text)})
        for path in ste.code_files():
            rel = str(path.relative_to(REPO_ROOT))
            source = path.read_text(encoding="utf-8")
            cut = source.find("#[cfg(test)]\nmod tests")
            if cut >= 0:
                source = source[:cut]
            for _line, text in ste.code_texts(rel, source):
                out["code"].append({"text": text, "expected": code(text)})
        Path(argv[1]).write_text(json.dumps(out, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        print(f"corpus: {len(out['prose'])} pages, {len(out['code'])} code texts")
        return 0

    inputs = json.loads((FIXTURES / "inputs.json").read_text(encoding="utf-8"))
    out = {
        "prose": [{"text": t, "expected": prose(t)} for t in inputs["prose"]],
        "code": [{"text": t, "expected": code(t)} for t in inputs["code"]],
    }
    (FIXTURES / "expected.json").write_text(
        json.dumps(out, ensure_ascii=False, indent=1) + "\n", encoding="utf-8"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
