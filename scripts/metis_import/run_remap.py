"""`--codes remap` and `--codes keep`: plan, apply and verify.

The two modes are one pipeline. In keep mode (COLLIERY-T-3104) an item
keeps its Metis number when the board that gives its code has the prefix
`--prefix` (remap.keep_codes). Before the create of such an item, the script
sets the sequence of the board (`PUT /api/boards/{id}/code-sequences/{type}`,
org admin) so that the next code has the Metis number. Then it checks the
code that the create gives. A refused number (the code is retired or in use,
or the sequence is past it) stops the run and names the code and the reason.

apply runs four passes. Each pass records what it did in the state file
after each step, so a new run continues where the last one stopped and does
no step two times.

1. create: each item, in creation order. The state gets old code -> new
   code and the item id after each create. Before each create the script
   searches the tenant for the footer of the item and adopts an item that
   has it: that closes the window between a create and the write of the
   state file.
2. edges: `parent`, `supports` (ADR below an initiative), `blocks`, the
   `impacts` link of each document and ADR, the lifecycle of the documents,
   and the metadata `document_type = vision` of the vision.
3. references: title and content of each item whose text changes, with the
   version that the server gives. The footer does not change.
4. archive: the items that Metis archived, children before parents. When
   the archive of an item cascades to an item that Metis did not archive,
   the script restores that item.
"""

import collections
import hashlib
import json
import os
import sys
import time

from . import remap
from .api import Api, Stop
from .metis import inventory, number

IMPLIED_BY_TEAM = ("manage_tasks", "manage_documents", "transition_items")


# --- files ----------------------------------------------------------------

def default_state_path(repository):
    return os.path.join(os.path.expanduser("~/kairos-import"), repository, "import-state.json")


def load_state(path):
    state = {"mode": "remap", "codes": {}, "ids": {}, "by": {}, "done": [],
             "refused": {}, "texts": [], "archived": [], "restored": []}
    if os.path.exists(path):
        with open(path, encoding="utf-8") as f:
            state.update(json.load(f))
    return state


def save_state(path, state):
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(state, f, indent=1, sort_keys=True)
        f.write("\n")
    os.replace(tmp, path)


def load_also_map(paths):
    """old code -> new code from the state files of earlier runs (or the
    ledger of the keep mode, which has the same `codes` key)."""
    mapping = {}
    for path in [p for p in (paths or "").split(",") if p.strip()]:
        path = os.path.expanduser(path.strip())
        with open(path, encoding="utf-8") as f:
            mapping.update(json.load(f).get("codes", {}))
    return mapping


def staged_texts(content_dir, docs):
    """old code -> (title, body) of the staged files. ADRs never get one.
    Returns (staged, ignored, bad)."""
    staged, ignored, bad = {}, [], []
    if not content_dir:
        return staged, ignored, bad
    content_dir = os.path.expanduser(content_dir)
    for code, doc in docs.items():
        path = os.path.join(content_dir, code + ".md")
        if not os.path.exists(path):
            continue
        if remap.kind_of(doc) == "adr":
            ignored.append(code)
            continue
        with open(path, encoding="utf-8") as f:
            try:
                staged[code] = remap.parse_staged(f.read())
            except ValueError as e:
                bad.append("%s: %s" % (code, e))
    return staged, ignored, bad


def source_text(doc, staged):
    """(title, body) as the item gets it at create: the staged file, or the
    Metis title and body."""
    if doc["short_code"] in staged:
        return staged[doc["short_code"]]
    return doc["title"], doc["body"]


def expected_text(doc, staged, repository, mapping, wiki):
    title, body = source_text(doc, staged)
    new_title, _ = remap.rewrite_text(title, mapping, wiki)
    new_body, _ = remap.rewrite_text(body.rstrip("\n"), mapping, wiki)
    return new_title, new_body + remap.footer(repository, doc)


def unknown_references(docs, known):
    """Codes that the text of the record names and that are not in `known`:
    Counter of prefixes, and old code -> list of unknown codes."""
    prefixes = collections.Counter()
    where = {}
    for code, doc in docs.items():
        found = [c for c in remap.codes_in(doc["title"] + "\n" + doc["body"]) if c not in known]
        if found:
            where[code] = found
            for c in found:
                prefixes[c.split("-")[0]] += 1
    return prefixes, where


# --- server context -------------------------------------------------------

class Context:
    """The boards, the repository and the rights of the principals."""

    def __init__(self, api, args):
        self.api = api
        self.boards = {}
        for role, slug in (("delivery", args.delivery_board), ("initiative", args.initiative_board),
                           ("adr", args.adr_board)):
            board = api.json("GET", "/api/boards/%s" % slug)
            board["by_name"] = {c["name"]: c["id"] for c in board["columns"] if not c.get("removed_at")}
            self.boards[role] = board
        # The code prefix of each board (COLLIERY-T-3099).
        self.prefixes = {role: board.get("code_prefix") for role, board in self.boards.items()}
        delivery = self.boards["delivery"]
        if delivery.get("code_prefix") != args.prefix:
            raise Stop("--prefix is %s, but the delivery board %s has the prefix %s. --prefix must "
                       "be the prefix of the delivery board."
                       % (args.prefix, delivery["slug"], delivery.get("code_prefix")))
        repos = api.json("GET", "/api/repositories")
        match = [r for r in repos if r["slug"] == args.repository]
        if not match:
            raise Stop("the tenant has no repository %r" % args.repository)
        self.repository = match[0]
        self.me = api.json("GET", "/api/whoami")

    def board_of(self, kind):
        return self.boards[{"task": "delivery", "initiative": "initiative", "adr": "adr"}[kind]]

    def prefix_of(self, doc, docs):
        """The prefix of the board that gives the code of doc."""
        return self.prefixes[remap.role_of(doc, docs)]

    def is_admin(self):
        return self.me["organization"].get("role") == "admin"

    def key_has(self, role, capability):
        if self.me["organization"].get("role") == "admin":
            return True
        board = self.boards[role]
        for row in self.me.get("capabilities", []):
            if row["board_id"] == board["id"]:
                for grant in row["grants"]:
                    if grant == capability or (grant.endswith("*") and capability.startswith(grant[:-1])):
                        return True
        teams = {t["id"] for t in self.me.get("teams", [])}
        return board.get("team_id") in teams and capability in IMPLIED_BY_TEAM


def create_class(doc, docs):
    """(role of the board, capability) that the create of doc needs."""
    kind = remap.kind_of(doc)
    if kind == "task":
        return "delivery", "manage_tasks"
    if kind == "initiative":
        return "initiative", "manage_initiatives"
    if kind == "adr":
        return "adr", "manage_adrs"
    # COLLIERY-T-3109: the create gate of a document is its owner board, the
    # delivery board, also for a document that supports a parent.
    return "delivery", "manage_documents"


def rights(ctx, docs, admin_token):
    """Which creates use the admin token. Stop before any write when the key
    lacks a right and no admin token is given."""
    need = sorted({create_class(d, docs) for d in docs.values()})
    use_admin = {}
    for role, cap in need:
        use_admin[(role, cap)] = not ctx.key_has(role, cap)
    missing = ["%s on %s" % (cap, ctx.boards[role]["slug"]) for (role, cap), a in use_admin.items() if a]
    if missing and not admin_token:
        raise Stop("KAIROS_KEY does not have: %s. Give the key these capabilities, "
                   "or set KAIROS_ADMIN_TOKEN." % ", ".join(missing))
    if missing:
        print("The admin token creates the items that need: %s. The same token then does the "
              "later writes on those items." % ", ".join(missing))
    else:
        print("KAIROS_KEY has each capability that the import needs. "
              "The script does not use the admin token.")
    return use_admin


# --- pass 1 ---------------------------------------------------------------

def find_existing(api, doc, repository):
    """An item of the tenant whose footer names this old code, or None."""
    kind = remap.kind_of(doc)
    body = {"q": '"%s"' % remap.footer_marker(doc).rstrip("."),
            "filter": {"entity_type": [kind], "include_deleted": True}, "limit": 25}
    found = api.json("POST", "/api/search", body)
    lead = remap.FOOTER_LEAD + repository + "."
    for rows in found.get("results", {}).values():
        for row in rows:
            _, foot = remap.split_footer(row.get("content", ""))
            if remap.footer_marker(doc) in foot and lead in foot:
                return row
    return None


def create_body(doc, docs, ctx, args, state, staged):
    kind = remap.kind_of(doc)
    title, text = source_text(doc, staged)
    body = {"title": title, "content": text.rstrip("\n") + remap.footer(args.repository, doc)}
    if kind in remap.COLUMN:
        board = ctx.board_of(kind)
        column = remap.COLUMN[kind].get(doc["phase"])
        if column is None or column not in board["by_name"]:
            raise Stop("%s: no column for the phase %r on the board %s"
                       % (doc["short_code"], doc["phase"], board["slug"]))
        body["board_id"] = board["id"]
        body["column_id"] = board["by_name"][column]
    if kind == "initiative":
        size = doc.get("estimated_complexity", "").lower()
        if size in ("xs", "s", "m", "l", "xl"):
            body["complexity"] = size
    elif kind == "adr":
        date = doc.get("decision_date", "")[:10]
        if len(date) == 10 and date[4] == "-" and date[7] == "-":
            body["decision_date"] = date
        if doc.get("decision_maker") and doc["decision_maker"] != "NULL":
            body["decision_maker"] = doc["decision_maker"]
    elif kind == "task":
        body["repository"] = args.repository
        for tag in doc["tags"]:
            if tag in remap.TASK_TYPE:
                body["task_type"] = remap.TASK_TYPE[tag]
    elif kind == "document":
        # COLLIERY-T-3109: each document has an owner board, also a document
        # that supports a parent. The owner is the delivery board.
        body["board"] = ctx.boards["delivery"]["id"]
        how, parent = remap.owner(doc, docs)
        if how == "parent":
            if parent not in state["codes"]:
                raise Stop("%s: its parent %s has no item yet" % (doc["short_code"], parent))
            body["parent_short_code"] = state["codes"][parent]
    return body


def create(api, doc, body, admin, args, before_post=None):
    """Create one item. Before each try, adopt an item that has the footer.
    before_post (keep mode) runs before each POST. Returns (row, adopted)."""
    route = "/api/" + remap.ROUTE[remap.kind_of(doc)]
    last = None
    for wait in (0,) + (1, 2, 4):
        if wait:
            time.sleep(wait)
        row = find_existing(api, doc, args.repository)
        if row is not None:
            return row, True
        if before_post is not None:
            before_post()
        try:
            status, raw, _ = api.call("POST", route, body, admin=admin, retry=False)
        except Stop as e:
            last = str(e)
            continue
        if status == 201:
            return json.loads(raw), False
        raise Stop("create of %s -> HTTP %s: %s" % (doc["short_code"], status, raw[:500]))
    raise Stop("create of %s failed 4 times (%s). The state file is correct. "
               "Run the command again to continue." % (doc["short_code"], last))


def set_next_number(api, ctx, doc, docs, code, admin):
    """Keep mode: set the sequence of the board of doc, so that its next
    code is `code`. A refusal stops the run and says why."""
    kind = remap.kind_of(doc)
    board = ctx.boards[remap.role_of(doc, docs)]
    path = "/api/boards/%s/code-sequences/%s" % (board["id"], kind)
    status, raw, _ = api.call("PUT", path, {"next_number": number(code)}, admin=admin)
    if status == 200:
        return
    try:
        error = json.loads(raw).get("error", {})
    except ValueError:
        error = {}
    raise Stop("%s cannot keep its Metis number: the server refused the code %s on the board %s "
               "(HTTP %s %s: %s). Nothing was created for %s. The state file is correct."
               % (doc["short_code"], code, board["slug"], status, error.get("code", "?"),
                  error.get("message", raw[:300]), doc["short_code"]))


def pass_create(api, ctx, docs, order, args, state, state_path, staged, use_admin,
                wanted=None, seq_admin=False):
    made = 0
    wanted = wanted or {}
    for doc in order:
        old = doc["short_code"]
        if old in state["codes"]:
            continue
        kind = remap.kind_of(doc)
        admin = use_admin[create_class(doc, docs)]
        body = create_body(doc, docs, ctx, args, state, staged)
        before_post = None
        if old in wanted:
            def before_post(doc=doc, code=wanted[old]):
                set_next_number(api, ctx, doc, docs, code, seq_admin)
        row, adopted = create(api, doc, body, admin, args, before_post)
        new = row.get("short_code")
        if old in wanted and new != wanted[old]:
            raise Stop("%s came back as %r, expected %s. Another create on the board took the "
                       "number. STOP: look at the item %r before the next run."
                       % (old, new, wanted[old], new))
        prefix = ctx.prefix_of(doc, docs)
        if not remap.new_code_ok(new, prefix, kind):
            raise Stop("%s came back as %r, not a %s code of the prefix %s"
                       % (old, new, kind, prefix))
        state["codes"][old] = new
        state["ids"][old] = row.get("id")
        state["by"][old] = "admin" if admin else "key"
        save_state(state_path, state)
        made += 1
        print("%s -> %s  [%s%s]%s" % (old, new, doc["phase"] or "-",
                                       ", archived" if doc["archived"] else "",
                                       "  (adopted)" if adopted else ""))
        if args.limit and made >= args.limit:
            print("Stopped at --limit %d." % args.limit)
            return False
    return True


# --- pass 2 ---------------------------------------------------------------

def mark(state, state_path, key):
    state["done"].append(key)
    save_state(state_path, state)


def pass_edges(api, docs, order, args, state, state_path):
    done = set(state["done"])
    edges, _ = remap.relations(docs)
    for rel, src, dst in edges:
        key = "%s %s %s" % (rel, src, dst)
        if key in done or key in state["refused"]:
            continue
        admin = state["by"].get(src) == "admin" and state["by"].get(dst) == "admin"
        ok, text = api.tool("link_items", {"source": state["codes"][src],
                                           "target": state["codes"][dst],
                                           "relationship": rel}, admin=admin)
        if ok or text.startswith("ALREADY_LINKED"):
            mark(state, state_path, key)
        elif text.startswith(("RELATIONSHIP_RULE", "CYCLE_DETECTED")):
            state["refused"][key] = text[:300]
            save_state(state_path, state)
            print("REFUSED %s: %s" % (key, text[:200]))
        else:
            raise Stop("link %s: %s" % (key, text[:500]))
    for doc in order:
        old = doc["short_code"]
        kind = remap.kind_of(doc)
        new = state["codes"][old]
        admin = state["by"].get(old) == "admin"
        if kind in ("document", "adr"):
            key = "impacts %s %s" % (old, args.repository)
            if key not in done:
                ok, text = api.tool("link_items", {"source": new, "target": args.repository,
                                                   "relationship": "impacts"}, admin=admin)
                if not (ok or text.startswith("ALREADY_LINKED")):
                    raise Stop("impacts %s: %s" % (new, text[:500]))
                mark(state, state_path, key)
        if kind == "document":
            value = remap.LIFECYCLE.get(doc["phase"], "draft")
            key = "lifecycle %s" % old
            if key not in done:
                if value != "draft":
                    api.json("PATCH", "/api/documents/%s/lifecycle" % new, {"lifecycle": value}, admin=admin)
                mark(state, state_path, key)
            key = "metadata %s" % old
            if doc["level"] == "vision" and key not in done:
                api.json("PATCH", "/api/documents/%s/metadata" % new,
                         {"values": {"document_type": "vision"}}, admin=admin)
                mark(state, state_path, key)


# --- pass 3 ---------------------------------------------------------------

def mapping_signature(mapping):
    return hashlib.sha256("\n".join(sorted(mapping)).encode("utf-8")).hexdigest()


def pass_references(api, docs, order, args, state, state_path, staged, mapping):
    # The pass is done for one set of loaded codes. A run with more codes
    # (a later --also-map) does the pass again: an item whose server text
    # does not change gets no write.
    signature = mapping_signature(mapping)
    if state.get("texts_signature") != signature:
        state["texts"] = []
        state["texts_signature"] = signature
        save_state(state_path, state)
    done = set(state["texts"])
    changed = 0
    for doc in order:
        old = doc["short_code"]
        if old in done:
            continue
        title, body = source_text(doc, staged)
        if (remap.rewrite_text(title, mapping, args.wiki_links)[0] == title
                and remap.rewrite_text(body, mapping, args.wiki_links)[0] == body):
            state["texts"].append(old)
            save_state(state_path, state)
            continue
        kind = remap.kind_of(doc)
        new = state["codes"][old]
        path = "/api/%s/%s" % (remap.ROUTE[kind], new)
        admin = state["by"].get(old) == "admin"
        archived_again = False
        for attempt in (1, 2, 3):
            item = api.json("GET", path, admin=admin)
            new_title = remap.rewrite_text(item["title"], mapping, args.wiki_links)[0]
            new_content = remap.rewrite_content(item["content"], mapping, args.wiki_links)[0]
            if new_title == item["title"] and new_content == item["content"]:
                break
            if item.get("archived_at"):
                # A later run with more codes: an archived item takes no
                # edit. Restore it, edit it, and archive it again. The state
                # says "not archived" until the archive is done, so a crash
                # leaves the archive to pass 4.
                if old in state["archived"]:
                    state["archived"].remove(old)
                    save_state(state_path, state)
                api.json("POST", "/api/%s/%s/restore" % (remap.ROUTE[kind], new), admin=admin)
                archived_again = True
                continue
            status, raw, _ = api.call("PATCH", path, {"title": new_title, "content": new_content,
                                                      "version": item["version"]}, admin=admin)
            if status == 200:
                changed += 1
                break
            if status == 409 and attempt < 3:
                continue
            raise Stop("rewrite of %s -> HTTP %s: %s" % (new, status, raw[:500]))
        if archived_again:
            archive_one(api, docs, state, state_path, doc)
        state["texts"].append(old)
        save_state(state_path, state)
    return changed


# --- pass 4 ---------------------------------------------------------------

def pass_archive(api, docs, args, state, state_path):
    rank = {k: i for i, k in enumerate(reversed(remap.ORDER))}
    todo = sorted((d for d in docs.values() if d["archived"]),
                  key=lambda d: (rank[remap.kind_of(d)], -number(d["short_code"])))
    for doc in todo:
        if doc["short_code"] not in state["archived"]:
            archive_one(api, docs, state, state_path, doc)


def archive_one(api, docs, state, state_path, doc):
    """Archive one item. When the archive cascades to an item that Metis did
    not archive, restore that item."""
    old = doc["short_code"]
    new = state["codes"][old]
    admin = state["by"].get(old) == "admin"
    path = "/api/%s/%s" % (remap.ROUTE[remap.kind_of(doc)], new)
    status, raw, _ = api.call("DELETE", path, admin=admin)
    if status == 404:
        item = api.json("GET", path, admin=admin)
        if not item.get("archived_at"):
            raise Stop("archive of %s -> HTTP 404: %s" % (new, raw[:300]))
        cascaded = []
    elif status in (200, 204):
        cascaded = (json.loads(raw) if raw.strip() else {}).get("cascaded_short_codes", [])
    else:
        raise Stop("archive of %s -> HTTP %s: %s" % (new, status, raw[:500]))
    state["archived"].append(old)
    save_state(state_path, state)
    new_to_old = {v: k for k, v in state["codes"].items()}
    for code in cascaded:
        src = new_to_old.get(code)
        if src is not None and docs.get(src, {}).get("archived"):
            continue
        if src is None:
            raise Stop("the archive of %s archived %s, which is not an item of this import. "
                       "Restore it by hand." % (new, code))
        api.json("POST", "/api/%s/%s/restore" % (remap.ROUTE[remap.kind_of(docs[src])], code),
                 admin=admin)
        state["restored"].append(src)
        save_state(state_path, state)
        print("RESTORED %s: the archive of %s archived it, and Metis did not archive it"
              % (code, new))


# --- plan -----------------------------------------------------------------

def plan_keep(docs, order, args, keep, ctx):
    """The keep part of plan: which items keep their Metis number."""
    wanted, displaced = keep
    if ctx is None:
        print("Keep mode with no --url: the plan supposes that only the delivery board has the "
              "prefix %s. Give --url to read the prefix of each board." % args.prefix)
    kept = collections.defaultdict(list)
    for doc in order:
        if doc["short_code"] in wanted:
            kept[remap.kind_of(doc)].append(number(wanted[doc["short_code"]]))
    print("Keep mode: %d items keep their Metis number with the prefix %s."
          % (len(wanted), args.prefix))
    for kind in remap.ORDER:
        if kept[kind]:
            print("  %-10s %4d  %s" % (kind, len(kept[kind]), compact(sorted(kept[kind]))))
    for old, (code, holder) in sorted(displaced.items()):
        print("  NEXT FREE %s: %s is the code of the item of %s. It gets the next free number."
              % (old, code, holder))
    others = collections.Counter(remap.kind_of(d) for d in order
                                 if d["short_code"] not in wanted and d["short_code"] not in displaced)
    if others:
        print("  New codes of their board: %s."
              % ", ".join("%d %s" % (n, k) for k, n in sorted(others.items())))


def plan(docs, issues, order, args, state, staged_info, also, ctx=None, keep=None):
    staged, ignored, bad = staged_info
    print("Metis record %s: %d documents. The state file has %d of them."
          % (args.metis, len(docs), len(state["codes"])))
    if keep is not None:
        plan_keep(docs, order, args, keep, ctx)
    counts = collections.Counter((remap.kind_of(d), d["level"], d["phase"] or "-", d["archived"])
                                 for d in docs.values())
    for (kind, level, phase, arch), n in sorted(counts.items()):
        print("%5d  %-10s from %-13s %-11s%s" % (n, kind, level, phase, "  archived" if arch else ""))
    print("Creation order:")
    for kind in remap.ORDER:
        group = [d["short_code"] for d in order if remap.kind_of(d) == kind]
        if group:
            print("  %-10s %4d  %s ... %s" % (kind, len(group), group[0], group[-1]))
    print("Archived in Metis: %d. They are created, then archived in the last pass."
          % sum(1 for d in docs.values() if d["archived"]))
    print("Staged files: %d%s." % (len(staged), (" (%s)" % args.content_dir) if args.content_dir else ""))
    for line in ignored:
        print("  IGNORED %s: an ADR does not get a staged text" % line)
    for line in bad:
        print("  BAD STAGED FILE %s" % line)
    problems = 0
    for name, rows in (("duplicate short code", issues["duplicates"]),
                       ("file with no short code or no level", issues["unreadable"]),
                       ("unknown Metis level", issues["unknown_level"]),
                       ("code letter does not agree with the level", remap.letter_mismatch(docs))):
        for row in rows:
            print("  PROBLEM %s: %s" % (name, row))
            problems += 1
    for code in issues.get("ambiguous", ()):
        files = sorted(d["metis_path"] for d in docs.values() if d.get("metis_code") == code)
        print("  AMBIGUOUS %s: %d documents have this code (%s). Each one is imported with its "
              "file in its footer. References to the code stay as written."
              % (code, len(files), ", ".join(files)))
    for key, missing in remap.gaps(docs).items():
        print("  Numbers with no document, %s: %s" % (key, compact(missing)))
    phases = collections.Counter()
    for d in docs.values():
        kind = remap.kind_of(d)
        if kind in remap.COLUMN:
            column = remap.COLUMN[kind].get(d["phase"])
            names = ctx.board_of(kind)["by_name"] if ctx else remap.COLUMN[kind].values()
            if column is None or column not in names:
                phases[(kind, d["phase"])] += 1
        elif d["phase"] not in remap.LIFECYCLE:
            phases[(kind, d["phase"])] += 1
    for (kind, phase), n in sorted(phases.items()):
        print("  PROBLEM %d %s items have the Metis phase %r, which has no column" % (n, kind, phase))
        problems += n
    edges, skipped = remap.relations(docs)
    missing_parents = [(c, r) for c, r in skipped if "not in the record" in r]
    for code, reason in missing_parents:
        print("  PROBLEM %s: %s" % (code, reason))
        problems += 1
    quiet = collections.Counter(r for c, r in skipped if r.endswith("there is no strategy"))
    for reason, n in quiet.items():
        print("  %d initiatives: %s. They get no parent." % (n, reason))
    for code, reason in skipped:
        if "not in the record" not in reason and not reason.endswith("there is no strategy"):
            print("  NOT MAPPED %s: %s" % (code, reason))
    rels = collections.Counter(e[0] for e in edges)
    supports_at_create = sum(1 for d in docs.values() if remap.kind_of(d) == "document"
                             and remap.owner(d, docs)[0] == "parent")
    impacts = sum(1 for d in docs.values() if remap.kind_of(d) in ("document", "adr"))
    print("Edges: %s. %d documents support their parent (the create writes that edge). "
          "%d impacts links to %s."
          % (", ".join("%d %s" % (n, r) for r, n in sorted(rels.items())) or "none",
             supports_at_create, impacts, args.repository))
    known = set(docs) | set(also)
    prefixes, where = unknown_references(docs, known)
    own = sorted({c for codes in where.values() for c in codes if c.split("-")[0] in
                  {k.split("-")[0] for k in docs}})
    if prefixes:
        print("References to codes that are not in this record and not in --also-map "
              "(they stay as they are):")
        for prefix, n in prefixes.most_common():
            distinct = len({c for codes in where.values() for c in codes if c.split("-")[0] == prefix})
            print("  %-10s %4d references, %d distinct codes" % (prefix, n, distinct))
        if own:
            print("  Codes of this record that no document has: %s" % ", ".join(own))
    else:
        print("References: each code in the text is in this record or in --also-map.")
    mapping = dict(also)
    mapping.update({c: "X" for c in docs})
    rewrites = sum(1 for d in docs.values()
                   if remap.rewrite_text(source_text(d, staged)[0] + "\n" + source_text(d, staged)[1],
                                         mapping)[0] != source_text(d, staged)[0] + "\n" + source_text(d, staged)[1])
    n = len(docs)
    requests = (2 * n + sum(rels.values()) + impacts
                + sum(1 for d in docs.values() if remap.kind_of(d) == "document"
                      and remap.LIFECYCLE.get(d["phase"], "draft") != "draft")
                + sum(1 for d in docs.values() if d["level"] == "vision")
                + 2 * rewrites + sum(1 for d in docs.values() if d["archived"]) + 8)
    print("Items whose text changes in pass 3: %d. Estimate of requests for a full apply: %d."
          % (rewrites, requests))
    return problems


def compact(nums):
    out, start, prev = [], None, None
    for n in nums + [None]:
        if start is None:
            start = prev = n
        elif n is not None and n == prev + 1:
            prev = n
        else:
            out.append(str(start) if start == prev else "%d-%d" % (start, prev))
            start = prev = n
    return ", ".join(out)


# --- verify ---------------------------------------------------------------

def verify(api, ctx, docs, order, args, state, staged, mapping, wanted=None):
    bad = []
    wanted = wanted or {}
    edges, _ = remap.relations(docs)
    incoming = collections.defaultdict(set)
    for rel, src, dst in edges:
        if "%s %s %s" % (rel, src, dst) not in state["refused"]:
            incoming[dst].add((rel, src))
    for d in docs.values():
        if remap.kind_of(d) == "document" and remap.owner(d, docs)[0] == "parent":
            incoming[d["short_code"]].add(("supports", d["parent"]))
    new_to_old = {v: k for k, v in state["codes"].items()}
    for doc in order:
        old = doc["short_code"]
        kind = remap.kind_of(doc)
        new = state["codes"].get(old)
        if not new:
            bad.append("MISSING  %s has no item" % old)
            continue
        if not remap.new_code_ok(new, ctx.prefix_of(doc, docs), kind):
            bad.append("TYPE     %s (%s) is not a %s code of its board" % (new, old, kind))
        if old in wanted and new != wanted[old]:
            bad.append("KEPT     %s (%s) does not have its Metis number: expected %s"
                       % (new, old, wanted[old]))
        status, raw, _ = api.call("GET", "/api/%s/%s" % (remap.ROUTE[kind], new))
        if status != 200:
            bad.append("MISSING  %s (%s): HTTP %s" % (new, old, status))
            continue
        item = json.loads(raw)
        want_title, want_content = expected_text(doc, staged, args.repository, mapping, args.wiki_links)
        if item["title"] != want_title:
            bad.append("TITLE    %s: %r, expected %r" % (new, item["title"], want_title))
        if item["content"] != want_content:
            bad.append("CONTENT  %s differs from the expected text" % new)
        body, foot = remap.split_footer(item["content"])
        if remap.footer_marker(doc) not in foot:
            bad.append("FOOTER   %s has no footer with %s" % (new, old))
        left = remap.old_codes_left(item["title"] + "\n" + body, mapping)
        if left:
            bad.append("OLD CODE %s still names %s" % (new, ", ".join(left)))
        if kind in remap.COLUMN:
            board = ctx.board_of(kind)
            want = board["by_name"].get(remap.COLUMN[kind].get(doc["phase"]))
            if item.get("board_id") != board["id"] or item.get("column_id") != want:
                bad.append("COLUMN   %s is not in %s of %s" % (new, remap.COLUMN[kind].get(doc["phase"]),
                                                             board["slug"]))
        if kind == "task":
            repo = (item.get("repository") or {}).get("slug") or (
                args.repository if item.get("repository_id") == ctx.repository["id"] else None)
            if repo != args.repository:
                bad.append("REPO     %s links to %r" % (new, repo))
        if kind in ("document", "adr"):
            slugs = [i.get("repository", {}).get("slug") for i in item.get("impacts", [])]
            if args.repository not in slugs:
                bad.append("IMPACTS  %s does not impact %s" % (new, args.repository))
        if kind == "document":
            want_board = ctx.boards["delivery"]["id"]
            if item.get("board_id") != want_board:
                bad.append("OWNER    %s names the board %r" % (new, item.get("board_id")))
            if doc["level"] == "vision":
                meta = api.json("GET", "/api/documents/%s/metadata" % new)
                if not any(v["slug"] == "document_type" and v["value"] == "vision" for v in meta["values"]):
                    bad.append("METADATA %s has no document_type = vision" % new)
            want_life = remap.LIFECYCLE.get(doc["phase"], "draft")
            if item.get("lifecycle") != want_life:
                bad.append("LIFECYCLE %s is %r, expected %r" % (new, item.get("lifecycle"), want_life))
        if bool(item.get("archived_at")) != doc["archived"]:
            bad.append("ARCHIVE  %s archived=%s, Metis archived=%s"
                       % (new, bool(item.get("archived_at")), doc["archived"]))
        rels = api.json("GET", "/api/%s/%s/relationships" % (remap.ROUTE[kind], new))
        got = set()
        for group in rels.get("incoming", []):
            if group["relationship"] in ("parent", "supports", "blocks"):
                for row in group["items"]:
                    if row["short_code"] in new_to_old:
                        got.add((group["relationship"], new_to_old[row["short_code"]]))
        if got != incoming.get(old, set()):
            bad.append("EDGES    %s: missing %s, extra %s" % (
                new, sorted(incoming.get(old, set()) - got), sorted(got - incoming.get(old, set()))))
    for line in bad:
        print(line)
    for key, text in sorted(state["refused"].items()):
        print("refused edge (not a difference): %s: %s" % (key, text[:160]))
    prefixes, _ = unknown_references(docs, set(docs) | set(mapping))
    if prefixes:
        print("Codes in no loaded mapping (they stay as they are): %s"
              % ", ".join("%s %d" % p for p in prefixes.most_common()))
    print("Verified %d items: %d differences. %d requests." % (len(order), len(bad), api.requests))
    return len(bad)


# --- entry ----------------------------------------------------------------

def rewrite_codes(state, issues):
    """The old -> new codes that the text rewrite uses. A code that 2 Metis
    documents used is left out, so a reference to it stays as written: the
    reference cannot say which of the 2 items it means. The `<code>~<n>` keys
    are left out too; no text names them."""
    ambiguous = set(issues.get("ambiguous", ()))
    return {old: new for old, new in state["codes"].items()
            if old not in ambiguous and "~" not in old}


def keep_order(docs, order, args, prefixes):
    """remap.keep_codes, and the footer sentence of each item that does not
    get its Metis number. Returns (wanted, displaced, order)."""
    wanted, displaced, order = remap.keep_codes(docs, order, args.prefix, prefixes)
    for old, (code, holder) in displaced.items():
        docs[old]["keep_note"] = remap.keep_note(code, holder, docs)
    return wanted, displaced, order


def main(args):
    docs, issues = inventory(args.metis, strict=False)
    order = remap.creation_order(docs)
    state_path = os.path.expanduser(args.state or default_state_path(args.repository))
    state = load_state(state_path)
    also = load_also_map(args.also_map)
    staged_info = staged_texts(args.content_dir, docs)
    staged = staged_info[0]
    key = os.environ.get("KAIROS_KEY")
    admin_token = os.environ.get("KAIROS_ADMIN_TOKEN")

    if state["codes"] and state.get("mode", "remap") != args.codes:
        raise Stop("the state file %s is for --codes %s, not --codes %s. Use --state to name a "
                   "different state file." % (state_path, state.get("mode", "remap"), args.codes))
    state["mode"] = args.codes

    if args.mode == "plan":
        ctx = None
        if args.url:
            ctx = Context(Api(args.url, key, admin_token, args.tenant), args)
        keep = None
        if args.codes == "keep":
            prefixes = ctx.prefixes if ctx else {"delivery": args.prefix}
            wanted, displaced, order = keep_order(docs, order, args, prefixes)
            keep = (wanted, displaced)
        problems = plan(docs, issues, order, args, state, staged_info, also, ctx, keep)
        return 1 if problems else 0

    if not args.url:
        raise Stop("--url is required for %s" % args.mode)
    url = args.url.rstrip("/")
    if state["codes"] and state.get("url", url) != url:
        raise Stop("the state file %s is for %s, not %s. Use --state to name the state file "
                   "of this deployment." % (state_path, state["url"], url))

    api = Api(args.url, key, admin_token, args.tenant)
    ctx = Context(api, args)
    mapping = dict(also)
    wanted = {}
    if args.codes == "keep":
        wanted, _, order = keep_order(docs, order, args, ctx.prefixes)

    if args.mode == "verify":
        mapping.update(rewrite_codes(state, issues))
        return 1 if verify(api, ctx, docs, order, args, state, staged, mapping, wanted) else 0

    if issues["duplicates"] or issues["unknown_level"]:
        raise Stop("the record has duplicate short codes or unknown levels. Run plan.")
    if staged_info[2]:
        raise Stop("bad staged files: %s" % " | ".join(staged_info[2]))
    os.makedirs(os.path.dirname(state_path), exist_ok=True)
    for field, value in (("repository", args.repository), ("prefix", args.prefix), ("url", url)):
        if state.get(field, value) != value:
            raise Stop("the state file %s is for the %s %r, not %r"
                       % (state_path, field, state[field], value))
        state[field] = value
    use_admin = rights(ctx, docs, admin_token)
    # Keep mode: the sequence route is for an organization admin.
    seq_admin = bool(wanted) and not ctx.is_admin()
    if seq_admin and not admin_token:
        raise Stop("--codes keep sets the code sequence of the board, and only an organization "
                   "admin can do that. KAIROS_KEY is not an organization admin. Set "
                   "KAIROS_ADMIN_TOKEN.")
    if seq_admin or (use_admin and any(use_admin.values())):
        me = api.json("GET", "/api/whoami", admin=True)
        if me["organization"].get("role") != "admin":
            raise Stop("KAIROS_ADMIN_TOKEN is not an organization admin")
    started = time.time()
    before = api.requests
    start_count = dict(api.count)
    if not pass_create(api, ctx, docs, order, args, state, state_path, staged, use_admin,
                       wanted, seq_admin):
        print("%d requests in %.1f s." % (api.requests - before, time.time() - started))
        return 0
    print("Pass 1: %d items have a code." % len(state["codes"]))
    pass_edges(api, docs, order, args, state, state_path)
    print("Pass 2: %d steps are done, %d edges refused." % (len(state["done"]), len(state["refused"])))
    mapping.update(rewrite_codes(state, issues))
    changed = pass_references(api, docs, order, args, state, state_path, staged, mapping)
    print("Pass 3: %d items got new references." % changed)
    pass_archive(api, docs, args, state, state_path)
    print("Pass 4: %d items are archived, %d restored." % (len(state["archived"]), len(state["restored"])))
    used = {m: n - start_count.get(m, 0) for m, n in api.count.items() if n - start_count.get(m, 0)}
    print("Done. %d requests after the start checks (%s) in %.1f s. The start checks used %d requests."
          % (api.requests - before, ", ".join("%s %d" % kv for kv in sorted(used.items())) or "none",
             time.time() - started, before))
    return 0


if __name__ == "__main__":
    sys.exit("Run scripts/migrate-metis-to-kairos.py")
