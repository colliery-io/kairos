#!/usr/bin/env python3
"""Copy a Metis work record into a Kairos tenant.

Two modes. `--codes keep` (the default) keeps the numbers and needs an empty
tenant: it is the mode that moved the kairos record on 2026-09-26, and the
text below describes it. `--codes remap` gives each item a new code from the
organization sequence and rewrites the references: see
scripts/metis_import/run_remap.py and the usage at the end of this text.

One-way and resumable. In keep mode every Metis document becomes one Kairos
item:

    vision         -> strategy   (S)   on the strategy board
    initiative     -> initiative (I)   on the initiative board
    task           -> task       (T)   on the repository's delivery board
    adr            -> adr        (A)   on the ADR board
    specification  -> document   (D)   supporting its Metis parent

Kairos mints short codes from one sequence per type and offers no way to set
one. So the numbers are kept by construction rather than by request: items are
created in ascending Metis order into a tenant that holds none of that type,
and the code that comes back is checked against the code that was expected.
The first mismatch stops the run, because every create after it would be off
by one and the damage would be silent.

Items land in the column their Metis phase names (`column_id` on the REST
create), not in the entry column followed by a walk through the transition
graph: the walk would write hundreds of transitions that never happened.

Content is copied verbatim, minus the YAML frontmatter, plus a provenance
footer. Short codes INSIDE the text are left as Metis wrote them, because the
same codes are cited by the code base and by the commit history.

Edges: Metis `parent` becomes a `parent` edge (parent -> child) and
`blocked_by` becomes `blocks` (blocker -> blocked). They go through the MCP
endpoint's `link_items`, which a board manager may call; the REST route is
org-admin only.

State is written to `<metis-dir>/kairos-migration.json` after every step, so a
re-run continues where the last one stopped and never creates an item twice.

Usage:
    KAIROS_KEY=kairos_sk_... [KAIROS_ADMIN_TOKEN=kairos_ss_...] \\
      scripts/migrate-metis-to-kairos.py --url https://kairos.example \\
        --metis .metis --repository kairos --prefix COLLIERY plan|apply|verify

KAIROS_KEY is a principal that manages the delivery, initiative and ADR
boards. KAIROS_ADMIN_TOKEN is only used for what sits on the strategy board:
the strategy itself and the documents that support it.

Remap mode:
    KAIROS_KEY=kairos_sk_... [KAIROS_ADMIN_TOKEN=kairos_ss_...] \\
      scripts/migrate-metis-to-kairos.py --codes remap \\
        --url https://kairos.example --metis ~/Desktop/fidius/.metis \\
        --repository fidius --prefix COLLIERY \\
        --delivery-board colliery-io-delivery \\
        [--also-map ~/kairos-import/fidius/import-state.json,...] \\
        [--content-dir ~/kairos-import/fidius/ste] plan|apply|verify

The state goes to ~/kairos-import/<repository>/import-state.json (--state).
plan reads only the files, and the boards too when --url is given. apply
refuses a URL that is not 127.0.0.1 or localhost unless
--i-mean-the-live-deployment is given.
"""

import argparse
import datetime
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from metis_import import run_remap  # noqa: E402
from metis_import.api import Api, Stop, is_local  # noqa: E402
from metis_import.metis import parse  # noqa: E402,F401

LETTER = {"strategy": "S", "initiative": "I", "task": "T", "document": "D", "adr": "A"}
KIND = {
    "vision": "strategy",
    "initiative": "initiative",
    "task": "task",
    "adr": "adr",
    "specification": "document",
}
ROUTE = {
    "strategy": "strategies",
    "initiative": "initiatives",
    "task": "tasks",
    "document": "documents",
    "adr": "adrs",
}
# Metis phase -> Kairos column name, per board level.
COLUMN = {
    "strategy": {"draft": "Draft", "review": "Review", "published": "Active"},
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
# Metis specification phase -> Kairos document editorial lifecycle.
LIFECYCLE = {
    "discovery": "draft",
    "drafting": "draft",
    "review": "review",
    "published": "published",
}
TASK_TYPE = {"#bug": "bug", "#tech-debt": "tech_debt", "#feature": "task"}
# Creation order. Documents come after what they support.
ORDER = ["strategy", "initiative", "adr", "document", "task"]


def inventory(metis_dir):
    docs = {}
    for root, _dirs, files in os.walk(metis_dir):
        for name in files:
            if not name.endswith(".md") or name == "code-index.md":
                continue
            doc = parse(os.path.join(root, name))
            if doc is None or doc["level"] not in KIND:
                continue
            if doc["short_code"] in docs:
                raise Stop("duplicate Metis short code %s" % doc["short_code"])
            docs[doc["short_code"]] = doc
    return docs


def number(code):
    return int(code.rsplit("-", 1)[1])


def new_code(prefix, doc):
    kind = KIND[doc["level"]]
    return "%s-%s-%04d" % (prefix, LETTER[kind], number(doc["short_code"]))


def footer(prefix, doc, today):
    created = doc.get("created_at", "")[:10] or "an unrecorded date"
    return (
        "\n---\n\n"
        "*Migrated from Metis `%s` on %s. Created in Metis on %s; phase at migration: "
        "%s%s. Short codes in this text are Metis codes and were left as written, "
        "because the code and the commit history cite them. `KAIROS-T-n`, `KAIROS-I-n` "
        "and `KAIROS-A-n` are `%s-T-n`, `%s-I-n` and `%s-A-n` here; a Metis "
        "specification `KAIROS-S-n` is the document `%s-D-n`; the vision "
        "`KAIROS-V-0001` is the strategy `%s-S-0001`.*\n"
        % (
            doc["short_code"],
            today,
            created,
            doc["phase"] or "unrecorded",
            ", archived" if doc["archived"] else "",
            prefix, prefix, prefix, prefix, prefix,
        )
    )


def boards(api):
    listing = api.json("GET", "/api/boards?limit=100")
    out = {}
    for row in listing.get("items", listing if isinstance(listing, list) else []):
        full = api.json("GET", "/api/boards/" + row["id"])
        full["by_name"] = {c["name"]: c["id"] for c in full["columns"] if not c.get("removed_at")}
        out[full["slug"]] = full
    return out


def plan(docs, prefix):
    steps = []
    for kind in ORDER:
        group = sorted((d for d in docs.values() if KIND[d["level"]] == kind),
                       key=lambda d: number(d["short_code"]))
        numbers = [number(d["short_code"]) for d in group]
        if numbers != list(range(1, len(numbers) + 1)):
            raise Stop("Metis %s numbers are not 1..%d without gaps; the codes cannot be kept"
                       % (kind, len(numbers)))
        steps.extend(group)
    return steps


def target_board(kind, doc, args, all_boards):
    if kind == "strategy":
        return all_boards[args.strategy_board]
    if kind == "initiative":
        return all_boards[args.initiative_board]
    if kind == "adr":
        return all_boards[args.adr_board]
    if kind == "task":
        return all_boards[args.delivery_board]
    return None


def create(api, doc, args, all_boards, state, docs, today):
    kind = KIND[doc["level"]]
    expected = new_code(args.prefix, doc)
    body = {"title": doc["title"], "content": doc["body"] + footer(args.prefix, doc, today)}
    admin = False
    board = target_board(kind, doc, args, all_boards)
    if board is not None:
        column = COLUMN[kind].get(doc["phase"])
        if column is None or column not in board["by_name"]:
            raise Stop("%s: no column for phase %r on board %s"
                       % (doc["short_code"], doc["phase"], board["slug"]))
        body["board_id"] = board["id"]
        body["column_id"] = board["by_name"][column]
    if kind == "strategy":
        admin = True
    elif kind == "initiative":
        size = doc.get("estimated_complexity", "").lower()
        if size in ("xs", "s", "m", "l", "xl"):
            body["complexity"] = size
    elif kind == "adr":
        if doc.get("decision_date"):
            body["decision_date"] = doc["decision_date"][:10]
        if doc.get("decision_maker"):
            body["decision_maker"] = doc["decision_maker"]
    elif kind == "task":
        body["repository"] = args.repository
        for tag in doc["tags"]:
            if tag in TASK_TYPE:
                body["task_type"] = TASK_TYPE[tag]
    elif kind == "document":
        parent = doc["parent"]
        if parent is None or parent not in state["codes"]:
            raise Stop("%s: a document needs a migrated parent, found %r" % (doc["short_code"], parent))
        body["parent_short_code"] = state["codes"][parent]
        admin = KIND[docs[parent]["level"]] == "strategy"
    made = api.json("POST", "/api/" + ROUTE[kind], body, admin=admin)
    got = made.get("short_code")
    if got != expected:
        raise Stop("%s came back as %s, expected %s. STOP: every later code would be off."
                   % (doc["short_code"], got, expected))
    return got, admin


def save(path, state):
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(state, f, indent=1, sort_keys=True)
        f.write("\n")
    os.replace(tmp, path)


def main():
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    p.add_argument("mode", choices=["plan", "apply", "verify"])
    p.add_argument("--codes", choices=["keep", "remap"], default="keep",
                   help="keep: keep the numbers (empty tenant). remap: new codes from the sequence")
    p.add_argument("--url", help="required, except for plan in remap mode")
    p.add_argument("--metis", default=".metis")
    p.add_argument("--repository", required=True)
    p.add_argument("--prefix", required=True)
    p.add_argument("--delivery-board", required=True)
    p.add_argument("--initiative-board", default="initiatives")
    p.add_argument("--adr-board", default="adrs")
    p.add_argument("--strategy-board", default="strategy")
    p.add_argument("--limit", type=int, default=0, help="stop after this many creates (0 = all)")
    p.add_argument("--state", help="remap: the state file (default ~/kairos-import/<repository>/import-state.json)")
    p.add_argument("--also-map", help="remap: state files of earlier runs, separated by commas")
    p.add_argument("--content-dir", help="remap: directory of staged texts <OLD-CODE>.md")
    p.add_argument("--wiki-links", choices=["plain", "link"], default="plain",
                   help="remap: [[CODE]] becomes the plain new code, or a markdown link")
    p.add_argument("--tenant", help="send this X-Tenant header (a deployment with more than one tenant)")
    p.add_argument("--i-mean-the-live-deployment", action="store_true",
                   help="permit apply against a URL that is not 127.0.0.1 or localhost")
    args = p.parse_args()
    sys.stdout.reconfigure(line_buffering=True)

    if args.mode == "apply" and args.url and not is_local(args.url) \
            and not args.i_mean_the_live_deployment:
        raise Stop("apply against %s is refused. That URL is not 127.0.0.1 or localhost. "
                   "Add --i-mean-the-live-deployment to write to it." % args.url)
    if args.codes == "remap":
        sys.exit(run_remap.main(args))
    if not args.url:
        raise Stop("--url is required in keep mode")

    docs = inventory(args.metis)
    steps = plan(docs, args.prefix)
    state_path = os.path.join(args.metis, "kairos-migration.json")
    state = {"codes": {}, "edges": [], "lifecycle": [], "archived": [], "skipped_edges": []}
    if os.path.exists(state_path):
        with open(state_path, encoding="utf-8") as f:
            state.update(json.load(f))

    counts = {}
    for d in steps:
        key = (KIND[d["level"]], d["phase"], d["archived"])
        counts[key] = counts.get(key, 0) + 1
    if args.mode == "plan":
        for (kind, phase, arch), n in sorted(counts.items()):
            print("%4d  %-10s %-10s%s" % (n, kind, phase, "  (archived)" if arch else ""))
        parents = sum(1 for d in steps if d["parent"] and KIND[d["level"]] != "document")
        blocks = sum(len(d["blocked_by"]) for d in steps)
        print("%4d  items; %d parent edges, %d blocks edges; %d already migrated"
              % (len(steps), parents, blocks, len(state["codes"])))
        return

    api = Api(args.url, os.environ.get("KAIROS_KEY"), os.environ.get("KAIROS_ADMIN_TOKEN"), args.tenant)
    all_boards = boards(api)
    today = datetime.date.today().isoformat()

    if args.mode == "verify":
        bad = 0
        for d in steps:
            kind = KIND[d["level"]]
            code = new_code(args.prefix, d)
            status, raw, _ = api.call("GET", "/api/%s/%s" % (ROUTE[kind], code))
            if d["archived"] and status in (404, 410):
                continue
            if status != 200:
                print("MISSING %s (%s): HTTP %s" % (code, d["short_code"], status))
                bad += 1
                continue
            item = json.loads(raw)
            if item.get("title") != d["title"]:
                print("TITLE   %s: %r != %r" % (code, item.get("title"), d["title"]))
                bad += 1
            if not item.get("content", "").startswith(d["body"].rstrip("\n")[:200]):
                print("CONTENT %s differs at the start" % code)
                bad += 1
            board = target_board(kind, d, args, all_boards)
            if board is not None:
                want = board["by_name"][COLUMN[kind][d["phase"]]]
                if item.get("column_id") != want:
                    print("COLUMN  %s is not in %s" % (code, COLUMN[kind][d["phase"]]))
                    bad += 1
        print("verified %d items, %d problems" % (len(steps), bad))
        sys.exit(1 if bad else 0)

    # --- apply: items -------------------------------------------------------
    made = 0
    for d in steps:
        if d["short_code"] in state["codes"]:
            continue
        code, _ = create(api, d, args, all_boards, state, docs, today)
        state["codes"][d["short_code"]] = code
        save(state_path, state)
        made += 1
        print("%s -> %s  [%s]" % (d["short_code"], code, d["phase"]))
        if args.limit and made >= args.limit:
            print("stopped at --limit %d" % args.limit)
            return

    # --- apply: document lifecycle ------------------------------------------
    for d in steps:
        if KIND[d["level"]] != "document" or d["short_code"] in state["lifecycle"]:
            continue
        value = LIFECYCLE.get(d["phase"])
        if value and value != "draft":
            code = state["codes"][d["short_code"]]
            admin = KIND[docs[d["parent"]]["level"]] == "strategy"
            api.json("PATCH", "/api/documents/%s/lifecycle" % code, {"lifecycle": value}, admin=admin)
            print("%s lifecycle %s" % (code, value))
        state["lifecycle"].append(d["short_code"])
        save(state_path, state)

    # --- apply: edges -------------------------------------------------------
    wanted = []
    for d in steps:
        kind = KIND[d["level"]]
        if d["parent"] and kind in ("initiative", "task"):
            wanted.append(("parent", d["parent"], d["short_code"]))
        for blocker in d["blocked_by"]:
            wanted.append(("blocks", blocker, d["short_code"]))
    for rel, src, dst in wanted:
        key = "%s %s %s" % (rel, src, dst)
        if key in state["edges"] or key in [s.split(" :: ")[0] for s in state["skipped_edges"]]:
            continue
        if src not in state["codes"] or dst not in state["codes"]:
            state["skipped_edges"].append("%s :: an end is not a migrated item" % key)
            save(state_path, state)
            continue
        ok, text = api.tool("link_items", {"source": state["codes"][src],
                                           "target": state["codes"][dst],
                                           "relationship": rel})
        if ok or "ALREADY_LINKED" in text:
            state["edges"].append(key)
        elif "RELATIONSHIP_RULE" in text or "CYCLE_DETECTED" in text:
            state["skipped_edges"].append("%s :: %s" % (key, text[:160]))
            print("skipped %s: %s" % (key, text[:160]))
        else:
            save(state_path, state)
            raise Stop("link %s: %s" % (key, text[:300]))
        save(state_path, state)
    print("%d edges, %d skipped" % (len(state["edges"]), len(state["skipped_edges"])))

    # --- apply: what Metis had archived -------------------------------------
    for d in steps:
        if not d["archived"] or d["short_code"] in state["archived"]:
            continue
        kind = KIND[d["level"]]
        code = state["codes"][d["short_code"]]
        api.json("DELETE", "/api/%s/%s" % (ROUTE[kind], code), ok=(200, 204))
        state["archived"].append(d["short_code"])
        save(state_path, state)
        print("%s archived, as it was in Metis" % code)

    print("done: %d items" % len(state["codes"]))


if __name__ == "__main__":
    try:
        main()
    except Stop as e:
        print("STOP: %s" % e, file=sys.stderr)
        sys.exit(2)
