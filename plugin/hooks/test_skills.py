#!/usr/bin/env python3
"""Checks of the skill set of the kairos plugin against its manifest, its
router and its README (the sync rule of KAIROS-A-0014), and of the text that
COLLIERY-T-1866, COLLIERY-T-1864 and COLLIERY-T-1865 put in the skills.
Stdlib only; run with `python3 -m unittest plugin/hooks/test_skills.py` (or
`angreal test unit`, which includes it).

These tests read the files as text. What an agent does with a skill is a
manual check in a real session: the steps are on each task."""

import json
import os
import re
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
PLUGIN = os.path.dirname(HERE)
ROOT = os.path.dirname(PLUGIN)
SKILLS = os.path.join(PLUGIN, "skills")


def read(path):
    with open(path, encoding="utf-8") as handle:
        return handle.read()


def frontmatter(path):
    """The `key: value` lines between the two `---` lines at the top."""
    text = read(path)
    match = re.match(r"---\n(.*?)\n---\n", text, re.S)
    if not match:
        return None
    fields = {}
    for line in match.group(1).splitlines():
        key, sep, value = line.partition(":")
        if sep and not line.startswith(" "):
            fields[key.strip()] = value.strip().strip('"')
    return fields


def skill_dirs():
    """Each folder under plugin/skills/<bucket>/ that has a SKILL.md, as a
    path relative to the repository root."""
    found = []
    for bucket in sorted(os.listdir(SKILLS)):
        bucket_dir = os.path.join(SKILLS, bucket)
        if not os.path.isdir(bucket_dir):
            continue
        for name in sorted(os.listdir(bucket_dir)):
            if os.path.isfile(os.path.join(bucket_dir, name, "SKILL.md")):
                found.append(f"./plugin/skills/{bucket}/{name}")
    return found


def is_user_invoked(fields):
    return fields.get("disable-model-invocation") == "true"


class SkillSetInSync(unittest.TestCase):
    def setUp(self):
        self.manifest = json.loads(read(os.path.join(ROOT, ".claude-plugin", "plugin.json")))
        self.router = read(os.path.join(SKILLS, "meta", "kairos", "SKILL.md"))
        self.readme = read(os.path.join(PLUGIN, "README.md"))
        self.skills = {}
        for rel in skill_dirs():
            path = os.path.join(ROOT, rel, "SKILL.md")
            self.skills[rel] = frontmatter(path)

    def test_manifest_lists_each_skill_folder(self):
        self.assertEqual(sorted(self.manifest["skills"]), sorted(self.skills))

    def test_each_skill_has_a_name_and_a_description(self):
        for rel, fields in self.skills.items():
            with self.subTest(skill=rel):
                self.assertIsNotNone(fields, "no frontmatter")
                self.assertEqual(fields.get("name"), os.path.basename(rel))
                self.assertTrue(fields.get("description"), "no description")

    def test_router_lists_each_user_invoked_skill(self):
        for rel, fields in self.skills.items():
            if is_user_invoked(fields):
                with self.subTest(skill=rel):
                    self.assertIn(f"`/kairos:{fields['name']}`", self.router)

    def test_router_lists_each_model_invoked_skill(self):
        line = next(
            (l for l in self.router.splitlines() if l.startswith("Model-invoked disciplines")),
            "",
        )
        for rel, fields in self.skills.items():
            if not is_user_invoked(fields):
                with self.subTest(skill=rel):
                    self.assertIn(f"`{fields['name']}`", line)

    def test_readme_names_each_skill(self):
        for rel, fields in self.skills.items():
            with self.subTest(skill=rel):
                self.assertIn(fields["name"], self.readme)


class KairosVocabulary(unittest.TestCase):
    """COLLIERY-T-1866."""

    def test_the_skill_is_model_invoked_and_has_its_reference(self):
        folder = os.path.join(SKILLS, "meta", "kairos-vocabulary")
        fields = frontmatter(os.path.join(folder, "SKILL.md"))
        self.assertFalse(is_user_invoked(fields))
        for word in ("task 1", "slice 2", "D3", "epic", "sprint", "mark it done"):
            self.assertIn(word, fields["description"])
        self.assertTrue(os.path.isfile(os.path.join(folder, "ITEM-TYPES.md")))

    def test_decompose_proposes_quoted_titles(self):
        text = read(os.path.join(SKILLS, "workflow", "decompose", "SKILL.md"))
        step4 = text.split("### 4. Quiz the user", 1)[1].split("### 5.", 1)[0]
        self.assertIn("quoted titles", step4)
        self.assertIn('"not yet created"', step4)
        self.assertNotIn("numbered list", step4)
        self.assertNotIn("which other slices", step4)
        self.assertIn("kairos-vocabulary", step4)

    def test_skills_that_name_work_point_to_the_vocabulary(self):
        for rel in ("meta/grilling", "workflow/to-initiative", "workflow/triage"):
            with self.subTest(skill=rel):
                text = read(os.path.join(SKILLS, rel, "SKILL.md"))
                self.assertIn("kairos-vocabulary", text)


if __name__ == "__main__":
    unittest.main()
