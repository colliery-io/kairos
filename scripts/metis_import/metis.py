"""Read a Metis archive: one document per markdown file with YAML frontmatter.

Standard library only. The frontmatter parser reads the flat keys that Metis
writes (`key: value`) and the list of tags; it is not a general YAML parser.
"""

import os
import re

LEVELS = ("vision", "initiative", "task", "adr", "specification")
CODE_IN_FIELD = re.compile(r"[A-Z][A-Z0-9]*-[A-Z]-\d{4}")
CODE = re.compile(r"\A[A-Z][A-Z0-9]*-[A-Z]-\d{4}\Z")
# The level a short code's type letter names, for a file that has no level.
LEVEL_OF_LETTER = {"V": "vision", "I": "initiative", "T": "task", "A": "adr", "S": "specification"}


def parse(path):
    """Return the document of one file as a dict, or None when the file is
    not a Metis document (no frontmatter, no short code or no level)."""
    with open(path, encoding="utf-8") as f:
        text = f.read()
    m = re.match(r"\A---\n(.*?)\n---\n?", text, re.DOTALL)
    if not m:
        return None
    front, body = m.group(1), text[m.end():]
    meta = {"tags": []}
    for line in front.splitlines():
        tag = re.match(r'^\s+-\s+"?(#[^"\s]+)"?\s*$', line)
        if tag:
            meta["tags"].append(tag.group(1))
            continue
        kv = re.match(r"^([a-z_]+):\s*(.*)$", line)
        if kv and kv.group(1) == "tags":
            # An inline list: tags: ["#task", "#phase/completed"]
            meta["tags"].extend(re.findall(r'"?(#[^",\]\s]+)"?', kv.group(2)))
        elif kv:
            meta[kv.group(1)] = kv.group(2).strip().strip('"')
    # Some documents that agents wrote by hand name their code `id` and leave
    # out `level`. The code and the level tag still say what the file is.
    if "short_code" not in meta and CODE.match(meta.get("id", "")):
        meta["short_code"] = meta["id"]
    if "level" not in meta and "short_code" in meta:
        level = next((t[1:] for t in meta["tags"] if t[1:] in LEVELS), None)
        letter = meta["short_code"].split("-")[1] if CODE.match(meta["short_code"]) else ""
        level = level or LEVEL_OF_LETTER.get(letter)
        if level:
            meta["level"] = level
    if "short_code" not in meta or "level" not in meta:
        return None
    meta["body"] = body.strip("\n") + "\n"
    meta["phase"] = next((t.split("/", 1)[1] for t in meta["tags"] if t.startswith("#phase/")), "")
    meta["blocked_by"] = CODE_IN_FIELD.findall(meta.get("blocked_by", ""))
    meta["parent"] = (CODE_IN_FIELD.findall(meta.get("parent", "")) or [None])[0]
    meta["archived"] = meta.get("archived", "false") == "true"
    meta["path"] = path
    return meta


def walk(metis_dir):
    """Yield (path, document-or-None) for each markdown file of the archive."""
    for root, dirs, files in os.walk(metis_dir):
        dirs.sort()
        for name in sorted(files):
            if not name.endswith(".md") or name == "code-index.md":
                continue
            path = os.path.join(root, name)
            yield path, parse(path)


def inventory(metis_dir, strict=True):
    """Return (docs, issues). docs maps a short code to its document.

    issues lists what does not map cleanly: files with frontmatter but no
    short code, unknown levels, and duplicate short codes. With strict=True a
    duplicate is an error (the keep-the-numbers mode needs that).

    With strict=False each document of a duplicated code is kept: the first
    under the code, each later one under `<code>~<n>`. Each of them gets
    `metis_code` (the code in the file) and `metis_path` (its path in the
    archive), and the code is listed in issues["ambiguous"]: a reference to it
    cannot say which document it means, so the import leaves it as written."""
    docs = {}
    issues = {"duplicates": [], "unreadable": [], "unknown_level": [], "ambiguous": []}
    for path, doc in walk(metis_dir):
        if doc is None:
            with open(path, encoding="utf-8") as f:
                if f.read(4) == "---\n":
                    issues["unreadable"].append(path)
            continue
        if doc["level"] not in LEVELS:
            issues["unknown_level"].append("%s (%s)" % (doc["short_code"], doc["level"]))
            continue
        code = doc["short_code"]
        if code in docs:
            if strict:
                raise ValueError("duplicate Metis short code %s" % code)
            if code not in issues["ambiguous"]:
                issues["ambiguous"].append(code)
                first = docs[code]
                first["metis_code"] = code
                first["metis_path"] = os.path.relpath(first["path"], metis_dir)
            n = 2
            while "%s~%d" % (code, n) in docs:
                n += 1
            doc["metis_code"] = code
            doc["metis_path"] = os.path.relpath(path, metis_dir)
            doc["short_code"] = "%s~%d" % (code, n)
        docs[doc["short_code"]] = doc
    return docs, issues


def number(code):
    """The number of a short code. A `~<n>` suffix (a duplicated code) is not
    part of it."""
    return int(code.split("~", 1)[0].rsplit("-", 1)[1])


def letter(code):
    return code.split("-")[-2]
