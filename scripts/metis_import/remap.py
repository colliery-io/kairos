"""The pure parts of `--codes remap`: no network, no files other than the
staged text. Unit tests: test_remap.py.

Decisions (COLLIERY-I-0019, COLLIERY-I-0021):

- Each item gets a new code from the organization sequence. So the type
  letter follows the Kairos type: a Metis specification `X-S-n` and the
  Metis vision `X-V-0001` become documents, `<PREFIX>-D-m`.
- References in title and content change from the old code to the new code
  after every item exists. Whole codes only. A Metis wiki link
  `[[X-T-0042]]` becomes the plain new code (the web view of Kairos renders
  plain text and links no code, so a wiki link would show its brackets).
- The provenance footer keeps the old code. The rewrite never touches it.
"""

import re

from .metis import letter, number

# Metis level -> Kairos type, in remap mode. There is no strategy target.
KIND = {
    "vision": "document",
    "initiative": "initiative",
    "task": "task",
    "adr": "adr",
    "specification": "document",
}
LETTER = {"strategy": "S", "initiative": "I", "task": "T", "document": "D", "adr": "A"}
ROUTE = {
    "strategy": "strategies",
    "initiative": "initiatives",
    "task": "tasks",
    "document": "documents",
    "adr": "adrs",
}
# Creation order of the types: the ORDER of the keep mode without strategy.
ORDER = ["initiative", "adr", "document", "task"]
# Within the documents, the vision comes before the specifications.
LEVEL_RANK = {"vision": 0, "specification": 1}

# Metis phase -> Kairos column name, per type that has a column.
COLUMN = {
    "initiative": {
        "discovery": "Discovery",
        "design": "Design",
        "ready": "Ready",
        "decompose": "Decompose",
        "active": "Active",
        "completed": "Completed",
    },
    "task": {
        "backlog": "Backlog",
        "todo": "Todo",
        "active": "Active",
        "blocked": "Blocked",
        "completed": "Completed",
    },
    "adr": {
        "draft": "Draft",
        "discussion": "Discussion",
        "decided": "Decided",
        "superseded": "Superseded",
    },
}
# Metis document phase -> Kairos document lifecycle.
LIFECYCLE = {
    "draft": "draft",
    "discovery": "draft",
    "drafting": "draft",
    "review": "review",
    "published": "published",
}
TASK_TYPE = {"#bug": "bug", "#tech-debt": "tech_debt", "#feature": "task"}

# A short code: prefix, type letter, four digits. Not a part of a longer
# word or number: `FIDIUS-T-0042` does not match in `FIDIUS-T-00421` or in
# `XFIDIUS-T-0042`.
CODE = r"[A-Z][A-Z0-9]*-[A-Z]-\d{4}"
BARE = r"(?<![A-Za-z0-9_])(?P<bare>" + CODE + r")(?![A-Za-z0-9_])"
WIKI = r"\[\[\s*(?P<wiki>" + CODE + r")\s*(?:\|(?P<label>[^\]\n]*))?\]\]"
REFERENCE = re.compile(WIKI + "|" + BARE)

FOOTER_RULE = "\n\n---\n\n"
FOOTER_LEAD = "This item came from the Metis record of the repository "


def kind_of(doc):
    return KIND[doc["level"]]


def creation_order(docs):
    """The documents in creation order: the types in ORDER, and within a type
    ascending old number (the vision before the specifications)."""
    out = []
    for kind in ORDER:
        group = [d for d in docs.values() if KIND[d["level"]] == kind]
        group.sort(key=lambda d: (LEVEL_RANK.get(d["level"], 0), number(d["short_code"])))
        out.extend(group)
    return out


def footer(repository, doc):
    """The provenance footer. Plain ASD-STE100 text. It holds the old code, so
    a full-text search for the old code finds the item."""
    created = (doc.get("created_at") or "")[:10]
    parts = [
        "This item came from the Metis record of the repository %s." % repository,
        "Its Metis code was %s." % doc["short_code"],
        ("Metis created it on %s." % created) if created
        else "Metis did not record the date when it was created.",
        ("Its Metis phase was %s." % doc["phase"]) if doc.get("phase")
        else "Metis did not record its phase.",
    ]
    if doc.get("archived"):
        parts.append("Metis archived it.")
    return FOOTER_RULE + " ".join(parts) + "\n"


def footer_marker(doc):
    """The words of the footer that identify the item: used to adopt an item
    that a crash left out of the state file."""
    return "Its Metis code was %s." % doc["short_code"]


def split_footer(content):
    """Split content into (body, footer). The footer starts at the LAST rule
    that is followed by the footer lead. No footer: (content, "")."""
    at = content.rfind(FOOTER_RULE + FOOTER_LEAD)
    if at < 0:
        return content, ""
    return content[:at], content[at:]


def rewrite_text(text, mapping, wiki="plain"):
    """Replace each whole code that `mapping` knows with its new code.

    A wiki link `[[CODE]]` becomes the new code (wiki="plain") or a markdown
    link to the item page (wiki="link"). `[[CODE|label]]` becomes
    `label (NEW)`. A code that `mapping` does not know stays as it is, wiki
    brackets included.

    Returns (new_text, unknown) where unknown lists the codes not in mapping,
    in order of appearance."""
    unknown = []

    def sub(m):
        code = m.group("wiki") or m.group("bare")
        new = mapping.get(code)
        if new is None:
            unknown.append(code)
            return m.group(0)
        if m.group("wiki"):
            shown = new if wiki == "plain" else "[%s](/items/%s)" % (new, new)
            label = (m.group("label") or "").strip()
            return "%s (%s)" % (label, shown) if label else shown
        return new

    return REFERENCE.sub(sub, text), unknown


def rewrite_content(content, mapping, wiki="plain"):
    """rewrite_text on the body only. The footer stays as it is."""
    body, foot = split_footer(content)
    new_body, unknown = rewrite_text(body, mapping, wiki)
    return new_body + foot, unknown


def codes_in(text):
    """Every code in text, wiki or bare, in order of appearance."""
    return [m.group("wiki") or m.group("bare") for m in REFERENCE.finditer(text)]


def parse_staged(text):
    """A staged file: its first line is `# <title>`, the rest is the body.
    Returns (title, body). Raises ValueError for a file with no title line."""
    lines = text.split("\n", 1)
    m = re.match(r"^#\s+(.+?)\s*$", lines[0].lstrip("﻿"))
    if not m:
        raise ValueError("the first line is not '# <title>'")
    body = lines[1] if len(lines) > 1 else ""
    return m.group(1), body.strip("\n") + "\n"


def new_code_ok(new_code, prefix, kind):
    """The code that the server gave has the organization prefix and the
    letter of the type."""
    return bool(re.fullmatch(re.escape(prefix) + r"-" + LETTER[kind] + r"-\d{4,}", new_code or ""))


def owner(doc, docs):
    """The owner of a document: ("parent", old parent code) when its Metis
    parent is an initiative or a task, else ("board", reason-or-None)."""
    parent = doc.get("parent")
    if parent and parent in docs:
        level = docs[parent]["level"]
        if level in ("initiative", "task"):
            return "parent", parent
        return "board", "its Metis parent %s is a %s, and a document cannot support a document" % (
            parent, level)
    if parent:
        return "board", "its Metis parent %s is not in the record" % parent
    return "board", None


def relations(docs):
    """The edges that pass 2 writes, and the Metis links that do not map.

    Returns (edges, skipped). edges: (relationship, source old code, target
    old code). skipped: (old code, reason). A document that supports its
    parent gets that edge at its create, so it is not in edges."""
    edges, skipped = [], []
    for code in sorted(docs):
        doc = docs[code]
        kind = KIND[doc["level"]]
        parent = doc.get("parent")
        if parent:
            plevel = docs[parent]["level"] if parent in docs else None
            if plevel is None:
                skipped.append((code, "its Metis parent %s is not in the record" % parent))
            elif kind == "task":
                if plevel == "initiative":
                    edges.append(("parent", parent, code))
                else:
                    skipped.append((code, "its Metis parent %s is a %s" % (parent, plevel)))
            elif kind == "initiative":
                if plevel == "initiative":
                    edges.append(("parent", parent, code))
                elif plevel == "vision":
                    skipped.append((code, "its Metis parent is the vision, and there is no strategy"))
                else:
                    skipped.append((code, "its Metis parent %s is a %s" % (parent, plevel)))
            elif kind == "adr":
                if plevel in ("initiative", "task"):
                    edges.append(("supports", parent, code))
                else:
                    skipped.append((code, "its Metis parent %s is a %s, and a supports edge "
                                          "cannot go from a document to an ADR" % (parent, plevel)))
            elif kind == "document" and doc["level"] == "specification":
                how, reason = owner(doc, docs)
                if how == "board" and reason:
                    skipped.append((code, reason + ". The document names the delivery board"))
        for blocker in doc.get("blocked_by", []):
            if blocker in docs:
                edges.append(("blocks", blocker, code))
            else:
                skipped.append((code, "its blocker %s is not in the record" % blocker))
    return edges, skipped


def gaps(docs):
    """Per old prefix and letter: the numbers between 1 and the highest that
    no document has."""
    seen = {}
    for code in docs:
        key = code.rsplit("-", 1)[0]
        seen.setdefault(key, set()).add(number(code))
    out = {}
    for key, nums in sorted(seen.items()):
        missing = sorted(set(range(1, max(nums) + 1)) - nums)
        if missing:
            out[key] = missing
    return out


def letter_mismatch(docs):
    """Documents whose code letter does not agree with their Metis level."""
    want = {"vision": "V", "initiative": "I", "task": "T", "adr": "A", "specification": "S"}
    return sorted(c for c, d in docs.items() if letter(c) != want[d["level"]])
