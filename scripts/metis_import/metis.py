"""Read a Metis archive: one document per markdown file with YAML frontmatter.

Standard library only. The frontmatter parser reads the flat keys that Metis
writes (`key: value`) and the list of tags; it is not a general YAML parser.
"""

import os
import re

LEVELS = ("vision", "initiative", "task", "adr", "specification")
CODE_IN_FIELD = re.compile(r"[A-Z][A-Z0-9]*-[A-Z]-\d{4}")


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
        if kv and kv.group(1) != "tags":
            meta[kv.group(1)] = kv.group(2).strip().strip('"')
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
    duplicate is an error (the keep-the-numbers mode needs that)."""
    docs = {}
    issues = {"duplicates": [], "unreadable": [], "unknown_level": []}
    for path, doc in walk(metis_dir):
        if doc is None:
            with open(path, encoding="utf-8") as f:
                if f.read(4) == "---\n":
                    issues["unreadable"].append(path)
            continue
        if doc["level"] not in LEVELS:
            issues["unknown_level"].append("%s (%s)" % (doc["short_code"], doc["level"]))
            continue
        if doc["short_code"] in docs:
            if strict:
                raise ValueError("duplicate Metis short code %s" % doc["short_code"])
            issues["duplicates"].append(doc["short_code"])
            continue
        docs[doc["short_code"]] = doc
    return docs, issues


def number(code):
    return int(code.rsplit("-", 1)[1])


def letter(code):
    return code.split("-")[-2]
