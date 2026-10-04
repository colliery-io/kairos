"""Unit tests of the pure parts of `--codes remap`. Standard library only.

Run: python3 -m unittest discover -s scripts/metis_import -t scripts
(`angreal test unit` runs them.)
"""

import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from metis_import import remap  # noqa: E402
from metis_import.api import Api, Stop, is_local  # noqa: E402
from metis_import.metis import inventory, parse  # noqa: E402
from metis_import.run_remap import (  # noqa: E402
    create_body, load_also_map, mapping_signature, staged_texts,
)

MAP = {
    "FIDIUS-T-0042": "COLLIERY-T-0301",
    "FIDIUS-I-0003": "COLLIERY-I-0022",
    "FIDIUS-S-0001": "COLLIERY-D-0014",
    "FIDIUS-V-0001": "COLLIERY-D-0013",
}


def doc(code, level, phase="completed", parent=None, archived=False, created="2026-03-02T10:00:00+00:00",
        blocked_by=(), title="A title", body="Body.\n"):
    return {"short_code": code, "level": level, "phase": phase, "parent": parent, "archived": archived,
            "created_at": created, "blocked_by": list(blocked_by), "title": title, "body": body,
            "tags": []}


class WholeCodes(unittest.TestCase):
    def test_a_known_code_changes(self):
        out, unknown = remap.rewrite_text("See FIDIUS-T-0042.", MAP)
        self.assertEqual(out, "See COLLIERY-T-0301.")
        self.assertEqual(unknown, [])

    def test_a_longer_number_does_not_match(self):
        out, unknown = remap.rewrite_text("FIDIUS-T-00421 and FIDIUS-T-0042", MAP)
        self.assertEqual(out, "FIDIUS-T-00421 and COLLIERY-T-0301")

    def test_a_longer_prefix_does_not_match(self):
        out, _ = remap.rewrite_text("XFIDIUS-T-0042 xFIDIUS-T-0042 FIDIUS-T-0042x", MAP)
        self.assertEqual(out, "XFIDIUS-T-0042 xFIDIUS-T-0042 FIDIUS-T-0042x")

    def test_punctuation_around_a_code(self):
        text = "(FIDIUS-T-0042), `FIDIUS-I-0003`, **FIDIUS-T-0042**: /FIDIUS-S-0001/"
        out, _ = remap.rewrite_text(text, MAP)
        self.assertEqual(out, "(COLLIERY-T-0301), `COLLIERY-I-0022`, **COLLIERY-T-0301**: /COLLIERY-D-0014/")

    def test_an_unknown_code_stays_and_is_listed(self):
        out, unknown = remap.rewrite_text("WEIR-T-0009 and FIDIUS-T-0099", MAP)
        self.assertEqual(out, "WEIR-T-0009 and FIDIUS-T-0099")
        self.assertEqual(unknown, ["WEIR-T-0009", "FIDIUS-T-0099"])

    def test_no_chain_replacement(self):
        # A new code is never read again as an old code.
        chain = {"A-T-0001": "B-T-0001", "B-T-0001": "C-T-0001"}
        out, _ = remap.rewrite_text("A-T-0001 B-T-0001", chain)
        self.assertEqual(out, "B-T-0001 C-T-0001")

    def test_a_prefix_with_digits(self):
        out, _ = remap.rewrite_text("CLOACI2-T-0001", {"CLOACI2-T-0001": "COLLIERY-T-0900"})
        self.assertEqual(out, "COLLIERY-T-0900")


class WikiLinks(unittest.TestCase):
    def test_a_wiki_link_becomes_the_plain_code(self):
        out, _ = remap.rewrite_text("## Parent Initiative\n\n[[FIDIUS-I-0003]]\n", MAP)
        self.assertEqual(out, "## Parent Initiative\n\nCOLLIERY-I-0022\n")

    def test_a_wiki_link_with_spaces_and_a_label(self):
        out, _ = remap.rewrite_text("[[ FIDIUS-T-0042 ]] and [[FIDIUS-T-0042|the egress task]]", MAP)
        self.assertEqual(out, "COLLIERY-T-0301 and the egress task (COLLIERY-T-0301)")

    def test_link_form(self):
        out, _ = remap.rewrite_text("[[FIDIUS-T-0042]]", MAP, wiki="link")
        self.assertEqual(out, "[COLLIERY-T-0301](/items/COLLIERY-T-0301)")

    def test_an_unknown_wiki_link_keeps_its_brackets(self):
        out, unknown = remap.rewrite_text("[[WEIR-T-0001]] [[Parent Initiative]] [[triggers]]", MAP)
        self.assertEqual(out, "[[WEIR-T-0001]] [[Parent Initiative]] [[triggers]]")
        self.assertEqual(unknown, ["WEIR-T-0001"])


class Footer(unittest.TestCase):
    def test_footer_text(self):
        d = doc("FIDIUS-T-0042", "task")
        self.assertEqual(
            remap.footer("fidius", d),
            "\n\n---\n\nThis item came from the Metis record of the repository fidius. "
            "Its Metis code was FIDIUS-T-0042. Metis created it on 2026-03-02. "
            "Its Metis phase was completed.\n")

    def test_footer_of_an_archived_item_with_no_date(self):
        d = doc("FIDIUS-T-0042", "task", phase="", archived=True, created="")
        text = remap.footer("fidius", d)
        self.assertIn("Metis did not record the date when it was created.", text)
        self.assertIn("Metis did not record its phase.", text)
        self.assertTrue(text.endswith("Metis archived it.\n"))

    def test_the_rewrite_does_not_change_the_footer(self):
        d = doc("FIDIUS-T-0042", "task")
        content = "Blocks FIDIUS-T-0042 and [[FIDIUS-I-0003]]." + remap.footer("fidius", d)
        out, _ = remap.rewrite_content(content, MAP)
        body, foot = remap.split_footer(out)
        self.assertEqual(body, "Blocks COLLIERY-T-0301 and COLLIERY-I-0022.")
        self.assertIn("Its Metis code was FIDIUS-T-0042.", foot)

    def test_a_rule_in_the_body_is_not_the_footer(self):
        d = doc("FIDIUS-T-0042", "task")
        content = "Part one FIDIUS-T-0042\n\n---\n\nPart two FIDIUS-I-0003" + remap.footer("fidius", d)
        out, _ = remap.rewrite_content(content, MAP)
        self.assertIn("Part one COLLIERY-T-0301", out)
        self.assertIn("Part two COLLIERY-I-0022", out)
        self.assertIn("Its Metis code was FIDIUS-T-0042.", out)

    def test_content_with_no_footer(self):
        out, _ = remap.rewrite_content("FIDIUS-T-0042", MAP)
        self.assertEqual(out, "COLLIERY-T-0301")

    def test_the_rewrite_runs_two_times_with_no_change(self):
        d = doc("FIDIUS-T-0042", "task")
        once, _ = remap.rewrite_content("FIDIUS-T-0042" + remap.footer("fidius", d), MAP)
        twice, _ = remap.rewrite_content(once, MAP)
        self.assertEqual(once, twice)


class TypesAndOrder(unittest.TestCase):
    def test_type_letters_follow_the_kairos_type(self):
        self.assertEqual(remap.KIND["specification"], "document")
        self.assertEqual(remap.KIND["vision"], "document")
        self.assertNotIn("strategy", remap.KIND.values())
        self.assertTrue(remap.new_code_ok("COLLIERY-D-0013", "COLLIERY", "document"))
        self.assertFalse(remap.new_code_ok("COLLIERY-S-0001", "COLLIERY", "document"))
        self.assertFalse(remap.new_code_ok("OTHER-T-0001", "COLLIERY", "task"))
        self.assertTrue(remap.new_code_ok("COLLIERY-T-10001", "COLLIERY", "task"))

    def test_creation_order(self):
        docs = {d["short_code"]: d for d in [
            doc("X-T-0010", "task"), doc("X-T-0002", "task"), doc("X-S-0002", "specification"),
            doc("X-V-0001", "vision"), doc("X-S-0001", "specification"), doc("X-A-0003", "adr"),
            doc("X-I-0010", "initiative"), doc("X-I-0009", "initiative")]}
        self.assertEqual([d["short_code"] for d in remap.creation_order(docs)], [
            "X-I-0009", "X-I-0010", "X-A-0003", "X-V-0001", "X-S-0001", "X-S-0002",
            "X-T-0002", "X-T-0010"])

    def test_relations(self):
        docs = {d["short_code"]: d for d in [
            doc("X-V-0001", "vision"),
            doc("X-I-0001", "initiative", parent="X-V-0001"),
            doc("X-T-0001", "task", parent="X-I-0001"),
            doc("X-T-0002", "task", parent="X-I-0001", blocked_by=["X-T-0001", "Y-T-0001"]),
            doc("X-S-0001", "specification", parent="X-I-0001"),
            doc("X-S-0002", "specification", parent="X-V-0001"),
            doc("X-S-0003", "specification"),
            doc("X-A-0001", "adr", parent="X-I-0001"),
            doc("X-A-0002", "adr", parent="X-S-0001"),
            doc("X-T-0003", "task", parent="X-I-0404")]}
        edges, skipped = remap.relations(docs)
        self.assertEqual(sorted(edges), sorted([
            ("parent", "X-I-0001", "X-T-0001"), ("parent", "X-I-0001", "X-T-0002"),
            ("blocks", "X-T-0001", "X-T-0002"), ("supports", "X-I-0001", "X-A-0001")]))
        reasons = dict(skipped)
        self.assertIn("no strategy", reasons["X-I-0001"])
        self.assertIn("cannot go from a document to an ADR", reasons["X-A-0002"])
        self.assertIn("delivery board", reasons["X-S-0002"])
        self.assertIn("not in the record", reasons["X-T-0003"])
        self.assertEqual(remap.owner(docs["X-S-0001"], docs), ("parent", "X-I-0001"))
        self.assertEqual(remap.owner(docs["X-S-0003"], docs), ("board", None))
        self.assertEqual(remap.owner(docs["X-V-0001"], docs), ("board", None))

    def test_gaps_and_letters(self):
        docs = {d["short_code"]: d for d in [doc("X-T-0001", "task"), doc("X-T-0004", "task"),
                                             doc("X-I-0001", "task")]}
        self.assertEqual(remap.gaps(docs), {"X-T": [2, 3]})
        self.assertEqual(remap.letter_mismatch(docs), ["X-I-0001"])


class Staged(unittest.TestCase):
    def test_parse(self):
        self.assertEqual(remap.parse_staged("# New title\n\nThe body.\n\n"), ("New title", "The body.\n"))

    def test_no_title_line(self):
        with self.assertRaises(ValueError):
            remap.parse_staged("New title\nbody")

    def test_staged_files_of_a_record(self):
        docs = {"X-T-0001": doc("X-T-0001", "task"), "X-A-0001": doc("X-A-0001", "adr"),
                "X-S-0001": doc("X-S-0001", "specification")}
        with tempfile.TemporaryDirectory() as tmp:
            for name, text in (("X-T-0001.md", "# T\nbody"), ("X-A-0001.md", "# A\nbody"),
                               ("X-S-0001.md", "no title")):
                with open(os.path.join(tmp, name), "w", encoding="utf-8") as f:
                    f.write(text)
            staged, ignored, bad = staged_texts(tmp, docs)
        self.assertEqual(staged, {"X-T-0001": ("T", "body\n")})
        self.assertEqual(ignored, ["X-A-0001"])
        self.assertEqual(len(bad), 1)


class Files(unittest.TestCase):
    def test_parse_and_inventory(self):
        text = ('---\nid: x\nlevel: task\ntitle: "Do it"\nshort_code: "X-T-0002"\n'
                'created_at: 2026-01-02T00:00:00+00:00\nparent: X-I-0001\n'
                'blocked_by: [X-T-0001]\narchived: true\n\ntags:\n  - "#task"\n'
                '  - "#phase/todo"\n  - "#tech-debt"\n---\n\n# Do it\n\nText.\n')
        with tempfile.TemporaryDirectory() as tmp:
            for name in ("a.md", "b.md"):
                with open(os.path.join(tmp, name), "w", encoding="utf-8") as f:
                    f.write(text)
            d = parse(os.path.join(tmp, "a.md"))
            docs, issues = inventory(tmp, strict=False)
        self.assertEqual((d["phase"], d["parent"], d["blocked_by"], d["archived"]),
                         ("todo", "X-I-0001", ["X-T-0001"], True))
        self.assertEqual(d["body"], "# Do it\n\nText.\n")
        # strict=False keeps both files of a duplicated code (see
        # test_two_documents_with_one_code_are_both_kept).
        self.assertEqual(list(docs), ["X-T-0002", "X-T-0002~2"])
        self.assertEqual(issues["ambiguous"], ["X-T-0002"])
        with self.assertRaises(ValueError):
            with tempfile.TemporaryDirectory() as tmp:
                for name in ("a.md", "b.md"):
                    with open(os.path.join(tmp, name), "w", encoding="utf-8") as f:
                        f.write(text)
                inventory(tmp, strict=True)

    def _parse(self, text):
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "a.md")
            with open(path, "w", encoding="utf-8") as f:
                f.write(text)
            return parse(path)

    def test_a_code_in_id_and_inline_tags(self):
        # SKADI-T-0577: the code is in `id`, and the tags are an inline list.
        d = self._parse('---\nid: SKADI-T-0577\nlevel: task\ntitle: "Stop the audiobook"\n'
                        'archived: false\nphase: completed\n'
                        'tags: ["#task", "#phase/completed", "#android", "#bug"]\n---\n\n# Body\n')
        self.assertEqual((d["short_code"], d["level"], d["phase"]),
                         ("SKADI-T-0577", "task", "completed"))
        self.assertIn("#bug", d["tags"])

    def test_no_level_comes_from_the_level_tag(self):
        # SKADI-T-0563: a short code, no level, a blank line before the tags.
        d = self._parse('---\ntitle: "Ignore DROP TABLE"\nshort_code: "SKADI-T-0563"\n\n'
                        'tags:\n  - "#task"\n  - "#phase/backlog"\n  - "#tech-debt"\n\n'
                        'initiative_id: NULL\n---\n\n# Body\n')
        self.assertEqual((d["short_code"], d["level"], d["phase"]),
                         ("SKADI-T-0563", "task", "backlog"))

    def test_no_level_and_no_level_tag_comes_from_the_code_letter(self):
        d = self._parse('---\nshort_code: "SKADI-I-0070"\ntitle: "X"\ntags:\n  - "#phase/discovery"\n'
                        '---\n\n# Body\n')
        self.assertEqual(d["level"], "initiative")

    def test_two_documents_with_one_code_are_both_kept(self):
        # SKADI-T-0577 is in backlog/bugs/ and in backlog/features/.
        with tempfile.TemporaryDirectory() as tmp:
            for sub, title in (("bugs", "Stop the audiobook"), ("features", "Declutter pass")):
                os.makedirs(os.path.join(tmp, sub))
                with open(os.path.join(tmp, sub, "SKADI-T-0577.md"), "w", encoding="utf-8") as f:
                    f.write('---\nshort_code: "SKADI-T-0577"\nlevel: task\ntitle: "%s"\n'
                            'tags:\n  - "#phase/completed"\n---\n\n# %s\n' % (title, title))
            docs, issues = inventory(tmp, strict=False)
        self.assertEqual(sorted(docs), ["SKADI-T-0577", "SKADI-T-0577~2"])
        self.assertEqual(issues["ambiguous"], ["SKADI-T-0577"])
        self.assertEqual(issues["duplicates"], [])
        first, second = docs["SKADI-T-0577"], docs["SKADI-T-0577~2"]
        self.assertEqual((first["title"], second["title"]), ("Stop the audiobook", "Declutter pass"))
        # Each footer has the real code and its own file, so each marker is unique.
        self.assertIn("Its Metis code was SKADI-T-0577.", remap.footer("skadi", second))
        self.assertIn("bugs/SKADI-T-0577.md", remap.footer_marker(first))
        self.assertIn("features/SKADI-T-0577.md", remap.footer_marker(second))
        self.assertNotIn(remap.footer_marker(first), remap.footer("skadi", second))
        # The creation order can read the number of the second key.
        self.assertEqual(len(remap.creation_order(docs)), 2)
        # A reference to the ambiguous code is not rewritten.
        from metis_import.run_remap import rewrite_codes
        codes = rewrite_codes({"codes": {"SKADI-T-0577": "C-T-0001", "SKADI-T-0577~2": "C-T-0002",
                                         "SKADI-T-0001": "C-T-0003"}}, issues)
        self.assertEqual(codes, {"SKADI-T-0001": "C-T-0003"})
        out, _ = remap.rewrite_text("See SKADI-T-0577 and SKADI-T-0001.", codes)
        self.assertEqual(out, "See SKADI-T-0577 and C-T-0003.")

    def test_an_id_that_is_not_a_code_is_not_a_short_code(self):
        self.assertIsNone(self._parse('---\nid: some-slug\nlevel: task\n---\n\n# Body\n'))

    def test_also_map_and_signature(self):
        with tempfile.TemporaryDirectory() as tmp:
            a, b = os.path.join(tmp, "a.json"), os.path.join(tmp, "b.json")
            with open(a, "w") as f:
                f.write('{"codes": {"F-T-0001": "C-T-0010"}}')
            with open(b, "w") as f:
                f.write('{"codes": {"H-T-0001": "C-T-0011"}, "ids": {}}')
            mapping = load_also_map("%s, %s" % (a, b))
        self.assertEqual(mapping, {"F-T-0001": "C-T-0010", "H-T-0001": "C-T-0011"})
        self.assertNotEqual(mapping_signature(mapping), mapping_signature({"F-T-0001": "x"}))


class OldCodesLeft(unittest.TestCase):
    """verify: a reference that the rewrite did not change (COLLIERY-T-3103)."""

    def test_a_code_that_keeps_its_number_is_not_an_old_code(self):
        mapping = {"SKADI-T-0001": "SKADI-T-0001", "SKADI-I-0002": "COLLIERY-I-0408"}
        self.assertEqual(remap.old_codes_left("After SKADI-T-0001.", mapping), [])
        self.assertEqual(remap.old_codes_left("Part of SKADI-I-0002 and [[SKADI-I-0002]], after SKADI-T-0001.",
                                              mapping), ["SKADI-I-0002"])
        self.assertEqual(remap.old_codes_left("FIDIUS-T-0001 is in no mapping.", mapping), [])


class KeepCodes(unittest.TestCase):
    """`--codes keep` (COLLIERY-T-3104): which items keep their number."""

    def record(self):
        docs = {
            "SKADI-V-0001": doc("SKADI-V-0001", "vision"),
            "SKADI-S-0001": doc("SKADI-S-0001", "specification"),
            "SKADI-S-0003": doc("SKADI-S-0003", "specification"),
            "SKADI-S-0004": doc("SKADI-S-0004", "specification", parent="SKADI-I-0002"),
            "SKADI-I-0002": doc("SKADI-I-0002", "initiative"),
            "SKADI-T-0009": doc("SKADI-T-0009", "task"),
            "SKADI-T-0005": doc("SKADI-T-0005", "task"),
            "SKADI-T-0005~2": dict(doc("SKADI-T-0005", "task"), short_code="SKADI-T-0005~2",
                                   metis_code="SKADI-T-0005", metis_path="backlog/b.md"),
            "SKADI-A-0001": doc("SKADI-A-0001", "adr"),
        }
        docs["SKADI-T-0005"].update(metis_code="SKADI-T-0005", metis_path="backlog/a.md")
        return docs

    def test_the_board_prefix_and_the_metis_number(self):
        docs = self.record()
        prefixes = {"delivery": "SKADI", "adr": "SKADI", "initiative": "COLLIERY"}
        wanted, displaced, order = remap.keep_codes(docs, remap.creation_order(docs), "SKADI", prefixes)
        self.assertEqual(wanted, {
            "SKADI-A-0001": "SKADI-A-0001",
            "SKADI-V-0001": "SKADI-D-0001",
            "SKADI-S-0003": "SKADI-D-0003",
            "SKADI-S-0004": "SKADI-D-0004",
            "SKADI-T-0005": "SKADI-T-0005",
            "SKADI-T-0009": "SKADI-T-0009",
        })
        # The vision comes first, so the specification SKADI-S-0001 and the
        # second file of SKADI-T-0005 get the next free number.
        self.assertEqual(displaced, {
            "SKADI-S-0001": ("SKADI-D-0001", "SKADI-V-0001"),
            "SKADI-T-0005~2": ("SKADI-T-0005", "SKADI-T-0005"),
        })
        # The initiative (board prefix COLLIERY) gets a new code. The
        # document that supports it has the delivery board as its owner
        # (COLLIERY-T-3109), so it keeps its number.
        self.assertNotIn("SKADI-I-0002", wanted)
        codes = [d["short_code"] for d in order]
        self.assertEqual(codes, [
            "SKADI-I-0002", "SKADI-A-0001",
            "SKADI-V-0001", "SKADI-S-0003", "SKADI-S-0004", "SKADI-S-0001",
            "SKADI-T-0005", "SKADI-T-0009", "SKADI-T-0005~2",
        ])

    def test_a_board_with_a_different_prefix_keeps_no_number(self):
        docs = self.record()
        prefixes = {"delivery": "SKADI", "adr": "COLLIERY", "initiative": "COLLIERY"}
        wanted, _, _ = remap.keep_codes(docs, remap.creation_order(docs), "SKADI", prefixes)
        self.assertNotIn("SKADI-A-0001", wanted)
        self.assertIn("SKADI-T-0009", wanted)

    def test_the_footer_says_why_an_item_has_the_next_free_number(self):
        docs = self.record()
        note = remap.keep_note("SKADI-T-0005", "SKADI-T-0005", docs)
        self.assertEqual(note, "It did not get the code SKADI-T-0005, because the item of the Metis "
                               "file backlog/a.md has that code. It got the next free number.")
        note = remap.keep_note("SKADI-D-0001", "SKADI-V-0001", docs)
        self.assertIn("the Metis document SKADI-V-0001", note)
        d = dict(docs["SKADI-S-0001"], keep_note=note)
        self.assertTrue(remap.footer("skadi", d).rstrip("\n").endswith(note))
        _, foot = remap.split_footer("Body." + remap.footer("skadi", d))
        self.assertIn(note, foot)

    def test_the_board_that_gives_the_code(self):
        docs = self.record()
        self.assertEqual(remap.role_of(docs["SKADI-T-0009"], docs), "delivery")
        self.assertEqual(remap.role_of(docs["SKADI-S-0003"], docs), "delivery")
        # COLLIERY-T-3109: a document that supports a parent has the
        # delivery board as its owner too.
        self.assertEqual(remap.role_of(docs["SKADI-S-0004"], docs), "delivery")
        self.assertEqual(remap.role_of(docs["SKADI-A-0001"], docs), "adr")
        self.assertTrue(remap.new_code_ok("ACME-D-0004", None, "document"))
        self.assertFalse(remap.new_code_ok("ACME-T-0004", None, "document"))

    def test_a_specification_that_supports_an_initiative_gets_the_delivery_board(self):
        """COLLIERY-T-3109 AC2: the importer gives each document an owner
        board. A specification that supports an initiative names the
        delivery board, and its code gets the prefix of that board."""

        class Ctx:
            boards = {"delivery": {"id": "delivery-id"}, "initiative": {"id": "initiative-id"},
                      "adr": {"id": "adr-id"}}
            prefixes = {"delivery": "SKADI", "initiative": "COLLIERY", "adr": "SKADI"}

        class Args:
            repository = "skadi"

        docs = self.record()
        state = {"codes": {"SKADI-I-0002": "COLLIERY-I-0040"}}
        body = create_body(docs["SKADI-S-0004"], docs, Ctx(), Args(), state, {})
        self.assertEqual(body["board"], "delivery-id")
        self.assertEqual(body["parent_short_code"], "COLLIERY-I-0040")
        # A document with no parent names the delivery board too.
        body = create_body(docs["SKADI-S-0003"], docs, Ctx(), Args(), state, {})
        self.assertEqual(body["board"], "delivery-id")
        self.assertNotIn("parent_short_code", body)
        # The code that the server gives has the prefix of the delivery board.
        prefix = Ctx.prefixes[remap.role_of(docs["SKADI-S-0004"], docs)]
        self.assertEqual(prefix, "SKADI")
        self.assertTrue(remap.new_code_ok("SKADI-D-0040", prefix, "document"))
        self.assertFalse(remap.new_code_ok("COLLIERY-D-0040", prefix, "document"))


class Client(unittest.TestCase):
    def test_local_urls(self):
        self.assertTrue(is_local("http://127.0.0.1:41080"))
        self.assertTrue(is_local("http://localhost:8080/"))
        self.assertFalse(is_local("https://kairos.example.ts.net"))
        self.assertFalse(is_local("http://127.0.0.1.example.com"))

    def test_retry_then_stop(self):
        api = Api("http://127.0.0.1:9", "k", sleep=lambda s: None)
        calls = []

        def fail(method, path, body, admin, extra):
            calls.append(1)
            return 503, "busy", {}
        api._once = fail
        with self.assertRaises(Stop):
            api.call("GET", "/api/whoami")
        self.assertEqual(len(calls), 4)

    def test_a_4xx_is_not_retried(self):
        api = Api("http://127.0.0.1:9", "k", sleep=lambda s: None)
        calls = []

        def refuse(method, path, body, admin, extra):
            calls.append(1)
            return 422, '{"error": {}}', {}
        api._once = refuse
        self.assertEqual(api.call("POST", "/api/tasks", {})[0], 422)
        self.assertEqual(len(calls), 1)


if __name__ == "__main__":
    unittest.main()
