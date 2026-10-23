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
                    names = (f"`{fields['name']}`", f"`kairos:{fields['name']}`")
                    self.assertTrue(any(name in line for name in names))

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



class BlocksEdgesWhenWorkSplits(unittest.TestCase):
    """KAIROS-T-0493: a skill that splits work sets a `blocks` edge for each
    task that must wait, and no edge between parallel tasks, so the graph
    view shows serial and parallel work."""

    SKILLS_THAT_SPLIT = (
        "workflow/decompose",
        "workflow/grill-decomposition",
        "workflow/implement",
        "meta/kairos-vocabulary",
    )

    def test_each_skill_that_splits_work_names_the_rule(self):
        for rel in self.SKILLS_THAT_SPLIT:
            with self.subTest(skill=rel):
                text = read(os.path.join(SKILLS, rel, "SKILL.md"))
                self.assertIn("`blocks`", text)
                self.assertIn("parallel", text)
                self.assertIn("no edge", text)

    def test_decompose_writes_the_order_in_the_task(self):
        text = read(os.path.join(SKILLS, "workflow", "decompose", "SKILL.md"))
        template = text.split("<task-template>", 1)[1].split("</task-template>", 1)[0]
        self.assertIn("## Order", template)
        self.assertIn("can go in parallel", template)
        step5 = text.split("### 5.", 1)[1].split("## Reference", 1)[0]
        self.assertIn("graph view", step5)




class GroundBeforePlanning(unittest.TestCase):
    """KAIROS-T-0499 (COLLIERY-I-0627): implement, grilling and decompose
    start by reading the ADRs and documents of the repository and the
    initiative, and name what they read."""

    GROUND = os.path.join(SKILLS, "meta", "kairos", "GROUND.md")
    SKILLS_THAT_PLAN = (
        "workflow/implement",
        "meta/grilling",
        "workflow/grill-initiative",
        "workflow/grill-decomposition",
        "workflow/decompose",
    )

    def test_the_reference_gives_the_calls_and_the_sentence(self):
        text = read(self.GROUND)
        for phrase in (
            "`get_item`",
            "repository: <slug>, entity_type: [adr, document]",
            # KAIROS-T-0599: the ADRs with no `impacts` link.
            "`get_repository <slug>`",
            "ADRs of the owner team",
            "`team_pages <team>`",
            "`related_work`",
            "parent chain",
            "Read: KAIROS-A-0028, COLLIERY-D-0013. Nothing read contradicts this plan.",
            "contradicts KAIROS-A-0028",
            "Not checked:",
        ):
            self.assertIn(phrase, text)

    def test_each_skill_that_plans_grounds_first(self):
        for rel in self.SKILLS_THAT_PLAN:
            with self.subTest(skill=rel):
                text = read(os.path.join(SKILLS, rel, "SKILL.md"))
                self.assertIn("GROUND.md", text)
                self.assertIn("Read: ...", text)
                link = re.search(r"\]\(([^)]*GROUND\.md)\)", text).group(1)
                target = os.path.normpath(os.path.join(SKILLS, rel, link))
                self.assertTrue(os.path.isfile(target), target)

    def test_implement_grounds_before_the_work(self):
        text = read(os.path.join(SKILLS, "workflow", "implement", "SKILL.md"))
        self.assertLess(text.index("## Ground"), text.index("## Work"))

    def test_handoff_lists_what_was_read(self):
        text = read(os.path.join(SKILLS, "meta", "handoff", "SKILL.md"))
        self.assertIn("Read: ...", text)
        self.assertIn("GROUND.md", text)


class NamesAreCheckedAgainstTheCode(unittest.TestCase):
    """KAIROS-T-0500 (COLLIERY-I-0627): a symbol, file or dependency that a
    task names is checked with the code index before the plan relies on it;
    a stale name and an unchecked claim are marked."""

    def test_implement_and_decompose_check_the_names(self):
        for rel in ("workflow/implement", "workflow/decompose"):
            with self.subTest(skill=rel):
                text = read(os.path.join(SKILLS, rel, "SKILL.md"))
                self.assertIn("Check the names", text)
                for tool in ("`symbol`", "`code_search`", "`path`"):
                    self.assertIn(tool, text)
                self.assertIn("Stale:", text)
                self.assertIn("is not in the code", text)
                self.assertIn('"not checked"', text)
                self.assertIn("blocker", text)

    def test_implement_checks_before_the_work_and_lists_what_stays_unchecked(self):
        text = read(os.path.join(SKILLS, "workflow", "implement", "SKILL.md"))
        self.assertLess(text.index("## Ground"), text.index("## Check the names"))
        self.assertLess(text.index("## Check the names"), text.index("## Work"))
        gate = text.split("## The completion gate", 1)[1]
        self.assertIn('what stays "not checked"', gate)


class RemainingWorkLivesOnTheItems(unittest.TestCase):
    """KAIROS-T-0501 (COLLIERY-I-0627): what is left is the open tasks of
    the initiative and their `blocks` edges; the running skills read and
    edit that list, never rewrite it from memory."""

    RUNNING_SKILLS = (
        "workflow/implement",
        "meta/handoff",
        "workflow/ralph",
        "workflow/ralph-initiative",
    )

    def test_each_running_skill_names_the_rule(self):
        for rel in self.RUNNING_SKILLS:
            with self.subTest(skill=rel):
                text = read(os.path.join(SKILLS, rel, "SKILL.md"))
                self.assertIn("What remains lives on the items", text)
                self.assertIn("`blocks`", text)
                self.assertIn("`search` with `traverse", text)
                self.assertIn('never "out of scope"', text)
                self.assertIn("from memory", text)

    def test_implement_gives_the_traverse_call(self):
        text = read(os.path.join(SKILLS, "workflow", "implement", "SKILL.md"))
        self.assertIn("relationships: [parent, blocks]", text)
        self.assertIn("order of their edges", text)


class RecordThisDecision(unittest.TestCase):
    """KAIROS-T-0503 (COLLIERY-I-0628): domain-modeling drafts an ADR from
    the conversation, and implement asks at the end of a task."""

    def test_domain_modeling_has_the_path(self):
        text = read(os.path.join(SKILLS, "engineering", "domain-modeling", "SKILL.md"))
        section = text.split("## Record this decision", 1)[1]
        for phrase in (
            "`my_boards`",
            "`create_item`",
            "`item_type: adr`",
            "`parent:",
            "`impacts`",
            "`supersedes`",
            "context",
            "consequences",
            "ADR-FORMAT.md",
        ):
            self.assertIn(phrase, section)

    def test_implement_asks_before_completed(self):
        text = read(os.path.join(SKILLS, "workflow", "implement", "SKILL.md"))
        gate = text.split("## The completion gate", 1)[1].split("## Blocked", 1)[0]
        self.assertIn("did this work decide anything an ADR or the spec should hold?", gate)
        self.assertIn("Record this decision", gate)
        self.assertIn("`domain-modeling`", gate)
        self.assertIn("No decision to record.", gate)


class GrillInitiative(unittest.TestCase):
    """COLLIERY-T-1864."""

    def setUp(self):
        path = os.path.join(SKILLS, "workflow", "grill-initiative", "SKILL.md")
        self.fields = frontmatter(path)
        self.text = read(path)

    def test_the_skill_is_user_invoked(self):
        self.assertTrue(is_user_invoked(self.fields))

    def test_it_uses_the_grilling_loop_and_writes_as_it_goes(self):
        self.assertIn("`grilling`", self.text)
        self.assertIn("## Decisions (<who>, <date>)", self.text)
        self.assertIn("`edit_item`", self.text)

    def test_it_never_moves_the_item(self):
        self.assertIn("Never call `transition_item`", self.text)

    def test_it_offers_the_next_skills(self):
        self.assertIn("/kairos:decompose", self.text)



class GrillDecomposition(unittest.TestCase):
    """COLLIERY-T-1865."""

    def setUp(self):
        path = os.path.join(SKILLS, "workflow", "grill-decomposition", "SKILL.md")
        self.fields = frontmatter(path)
        self.text = read(path)

    def test_the_skill_is_user_invoked(self):
        self.assertTrue(is_user_invoked(self.fields))

    def test_it_reviews_the_four_questions(self):
        for heading in ("Slice boundaries", "Order and risk", "Done criteria", "Scope and repositories"):
            self.assertIn(heading, self.text)
        self.assertIn("`blocks`", self.text)
        self.assertIn("`set_repository`", self.text)

    def test_it_goes_back_up_and_never_moves_an_item(self):
        self.assertIn("/kairos:grill-initiative", self.text)
        self.assertIn("Never call `transition_item`", self.text)
        self.assertIn("/kairos:ralph-initiative", self.text)

    def test_decompose_offers_it_at_the_end(self):
        text = read(os.path.join(SKILLS, "workflow", "decompose", "SKILL.md"))
        step5 = text.split("### 5.", 1)[1].split("## Reference", 1)[0]
        self.assertIn("/kairos:grill-decomposition", step5)


class NotificationsInTheSkills(unittest.TestCase):
    """KAIROS-T-0382: implement and triage read and mark the events."""

    def test_implement_and_triage_read_and_mark_events(self):
        for rel in ("workflow/implement", "workflow/triage"):
            with self.subTest(skill=rel):
                text = read(os.path.join(SKILLS, rel, "SKILL.md"))
                self.assertIn("`get_events`", text)
                self.assertIn("`mark_events_read`", text)


class TheStateInTheSkills(unittest.TestCase):
    """KAIROS-T-0507: implement writes the live state of the task with
    `set_state` after each push and before each stop; handoff reads it and
    writes it."""

    def test_implement_sets_the_state_after_a_push_and_before_a_stop(self):
        text = read(os.path.join(SKILLS, "workflow", "implement", "SKILL.md"))
        self.assertIn("`set_state`", text)
        for field in ("branch", "commit", "pushed", "pr_url", "next_step"):
            self.assertIn(f"`{field}`", text)
        # The paragraph of the live state (the first `set_state` is in "Open
        # the work", KAIROS-T-0522).
        self.assertIn("before", text.split("Write it with `set_state`", 1)[1][:600])

    def test_handoff_reads_and_writes_the_state(self):
        text = read(os.path.join(SKILLS, "meta", "handoff", "SKILL.md"))
        self.assertIn("`set_state`", text)
        self.assertIn("state", text.split("`get_item`", 1)[0])


class TheWorkIsInAWorktree(unittest.TestCase):
    """KAIROS-T-0522: implement opens the work in a worktree of the clone
    after the claim, does each later step there, and removes the worktree at
    Completed when the branch is pushed and the PR is open. Ralph says the
    same."""

    def setUp(self):
        self.text = read(os.path.join(SKILLS, "workflow", "implement", "SKILL.md"))

    def section(self, title):
        match = re.search(rf"^## {re.escape(title)}\n(.*?)(?=^## )", self.text, re.S | re.M)
        self.assertIsNotNone(match, f"no section {title}")
        return match.group(1)

    def test_open_the_work_comes_after_the_claim_and_before_the_work(self):
        headings = re.findall(r"^## (.+)$", self.text, re.M)
        self.assertLess(headings.index("Pick up"), headings.index("Open the work"))
        self.assertLess(headings.index("Open the work"), headings.index("Work"))
        self.assertIn("after the claim", self.section("Open the work"))

    def test_open_the_work_gives_the_commands(self):
        text = self.section("Open the work")
        for command in (
            "git -C <clone> fetch origin <default>",
            "git -C <clone> worktree add ../wt-<code> -b <code> origin/<default>",
            "git -C <worktree> remote get-url origin",
            "`fetch_index` with `root=<worktree>`",
            "`set_state` with `branch=<code>`",
        ):
            self.assertIn(command, text)
        self.assertIn("lower case", text)
        self.assertIn("the list of clones of the session context", text)
        self.assertIn("Use `git -C <worktree>` and absolute paths", text)

    def test_the_repository_check_compares_the_origin(self):
        self.assertNotIn("differs from this checkout", self.text)
        self.assertIn("compare with the origin remote", self.text)

    def test_the_commit_is_in_the_worktree_not_on_the_current_branch(self):
        self.assertNotIn("commit to the current branch", self.text)
        self.assertIn("Commit in the worktree, on the branch of the task", self.text)
        self.assertIn('then "Close the work", then `transition_item` to **Completed**', self.text)

    def test_close_the_work_removes_a_pushed_worktree_and_keeps_the_branch(self):
        text = self.section("Close the work")
        self.assertIn("When the branch is pushed and the PR is open, remove the worktree", text)
        self.assertIn("git -C <clone> worktree remove ../wt-<code>", text)
        self.assertIn("Keep the branch", text)
        self.assertIn("Do not use `--force`", text)
        self.assertIn("in the closing note", text)

    def test_ralph_says_the_same(self):
        spec = __import__("importlib.util").util.spec_from_file_location(
            "ralph_for_skills", os.path.join(HERE, "ralph.py")
        )
        ralph = __import__("importlib.util").util.module_from_spec(spec)
        spec.loader.exec_module(ralph)
        procedure = ralph.PROCEDURE
        self.assertNotIn("Commit to the current branch", procedure)
        self.assertIn("git -C <clone> worktree add ../wt-<code> -b <code> origin/<default>", procedure)
        self.assertIn("`fetch_index` with the worktree as `root`", procedure)
        self.assertIn("Commit in the worktree, on the branch of the task", procedure)
        self.assertIn("remove the worktree", procedure)
        skill = read(os.path.join(SKILLS, "workflow", "ralph", "SKILL.md"))
        self.assertIn("open the work in a worktree (`../wt-<code>`", skill)
        self.assertIn("close the work", skill)


class AbortLeavesARecord(unittest.TestCase):
    """KAIROS-T-0523: implement has an Abort section: the note, the state
    with each field, the release of the claim, and the worktree removed or
    kept. cancel-ralph, the Ralph procedure and the channel name it."""

    def setUp(self):
        self.text = read(os.path.join(SKILLS, "workflow", "implement", "SKILL.md"))
        match = re.search(r"^## Abort\n(.*?)(?=^## )", self.text, re.S | re.M)
        self.assertIsNotNone(match, "no Abort section in implement")
        self.abort = match.group(1)

    def test_the_section_writes_the_note_with_what_is_done_and_not_done(self):
        self.assertIn("`append_note`", self.abort)
        self.assertIn("what is done and what is not done", self.abort)
        self.assertIn("`Next step:`", self.abort)

    def test_the_section_writes_each_field_of_the_state(self):
        self.assertIn("`set_state` with each field", self.abort)
        for field in ("branch", "commit", "pushed", "pr_url", "next_step"):
            self.assertIn(f"`{field}`", self.abort)

    def test_the_section_releases_only_its_own_claim(self):
        self.assertIn("you hold the claim, `release_item` the task", self.abort)
        self.assertIn("Do not release the claim of a different person", self.abort)

    def test_the_worktree_is_removed_when_clean_and_kept_else(self):
        self.assertIn("git -C <worktree> status --porcelain", self.abort)
        self.assertIn("git -C <clone> worktree remove ../wt-<code>", self.abort)
        self.assertIn("keep the worktree", self.abort)
        self.assertIn("path of a kept worktree", self.abort)

    def test_the_claim_of_a_different_person_leads_to_abort(self):
        self.assertIn('Stop, do "Abort", and tell the lead.', self.text)

    def test_cancel_ralph_ralph_and_the_channel_name_the_section(self):
        cancel = read(os.path.join(SKILLS, "workflow", "cancel-ralph", "SKILL.md"))
        self.assertIn('"Abort" section of `/kairos:implement`', cancel)
        ralph_skill = read(os.path.join(SKILLS, "workflow", "ralph", "SKILL.md"))
        self.assertIn('"Abort" section of `/kairos:implement`', ralph_skill)
        self.assertIn('"Abort" section of /kairos:implement', read(os.path.join(HERE, "ralph.py")))
        channel = read(os.path.join(PLUGIN, "bin", "kairos-channel"))
        self.assertIn("do the Abort section of kairos:implement", channel)


class BootstrapWritesNoRepositoryConnection(unittest.TestCase):
    """The MCP connection belongs to the user: the plugin server or a
    user-scope entry. A project `.mcp.json` only when the user asks."""

    def setUp(self):
        self.text = read(os.path.join(SKILLS, "meta", "bootstrap", "SKILL.md"))

    def test_the_oauth_path_adds_a_user_scope_entry(self):
        self.assertIn("claude mcp add --scope user", self.text)
        self.assertNotRegex(self.text, r"claude mcp add --transport")

    def test_a_project_entry_is_only_on_request(self):
        self.assertNotIn("propose the project `.mcp.json`", self.text)
        self.assertIn("Only when the user asks for this repository only", self.text)

    def test_the_plugin_ships_no_project_template(self):
        self.assertFalse(os.path.exists(os.path.join(PLUGIN, ".mcp.json")))


class SkillsAgree(unittest.TestCase):
    """KAIROS-T-0546: the skills give one answer for the grill trigger, the
    name of the review skill, the place of dead ends, the ADR link, the
    Kairos tools of a sub-agent, the initiative board, the skipped tasks of
    a loop, and the owner of a repository."""

    def text(self, rel):
        return read(os.path.join(SKILLS, rel))

    def test_the_review_skill_is_named_with_its_namespace(self):
        # Claude Code has a built-in `/code-review` that posts nothing to
        # Kairos. A bare `code-review` can load that one.
        paths = [os.path.join(HERE, "ralph.py")]
        for folder, _, files in os.walk(SKILLS):
            paths += [os.path.join(folder, name) for name in files if name.endswith(".md")]
        own = os.path.join(SKILLS, "engineering", "code-review", "SKILL.md")
        bare = []
        for path in paths:
            for line in read(path).splitlines():
                if path == own and line == "name: code-review":
                    continue
                if re.search(r"(?<![\w:/-])code-review", line):
                    bare.append(f"{os.path.relpath(path, ROOT)}: {line[:80]}")
        self.assertEqual(bare, [])

    def test_the_grill_trigger_names_no_user_invoked_skill_as_its_own(self):
        description = frontmatter(os.path.join(SKILLS, "meta", "grilling", "SKILL.md"))["description"]
        self.assertNotIn("any 'grill'", description)
        for rel in skill_dirs():
            fields = frontmatter(os.path.join(ROOT, rel, "SKILL.md"))
            name = fields["name"]
            if "grill" in name and is_user_invoked(fields):
                with self.subTest(skill=rel):
                    # The user runs it as `/kairos:<name>`; the bare name is a trigger.
                    rest = description.replace(f"/kairos:{name}", "")
                    self.assertNotRegex(rest, rf"\b{re.escape(name)}\b")
        body = self.text("meta/grilling/SKILL.md")
        self.assertIn("writes nothing to Kairos", body)
        self.assertIn("`/kairos:grill-initiative`", body)

    def test_dead_ends_go_on_the_task(self):
        handoff = self.text("meta/handoff/SKILL.md")
        self.assertIn("`append_note`", handoff)
        self.assertNotIn("temporary directory", handoff)
        self.assertNotIn("kairos-handoff-", handoff)
        self.assertIn("dead ends ruled out", self.text("workflow/implement/SKILL.md"))

    def test_an_adr_links_to_its_item_with_parent(self):
        adr_format = self.text("engineering/domain-modeling/ADR-FORMAT.md")
        self.assertIn("Send `parent`", adr_format)
        self.assertIn("`supports` edge", adr_format)
        for rel in (
            "engineering/domain-modeling/ADR-FORMAT.md",
            "engineering/domain-modeling/SKILL.md",
            "workflow/grill-initiative/SKILL.md",
            "workflow/grill-decomposition/SKILL.md",
            "workflow/implement/SKILL.md",
        ):
            with self.subTest(skill=rel):
                text = self.text(rel)
                self.assertNotIn("`informs`", text)
                self.assertNotIn("ADRs that inform", text)

    def test_a_sub_agent_has_the_kairos_tools(self):
        review = self.text("engineering/code-review/SKILL.md")
        self.assertNotIn("no Kairos access", review)
        self.assertIn("inherit the MCP tools of the session", review)
        self.assertIn("with Kairos tool access", self.text("engineering/research/SKILL.md"))

    def test_to_initiative_uses_the_initiative_board(self):
        self.assertIn("`initiative_board`", self.text("workflow/to-initiative/SKILL.md"))
        self.assertIn("`initiative_board`", self.text("workflow/grill-initiative/SKILL.md"))

    def test_ralph_initiative_appends_the_skipped_tasks(self):
        text = self.text("workflow/ralph-initiative/SKILL.md")
        self.assertIn("skipped tasks and their reasons on the initiative with `append_note`", text)

    def test_onboard_team_changes_the_owner_with_update_repository(self):
        text = self.text("workflow/onboard-team/SKILL.md")
        self.assertNotIn("does not change the owner", text)
        self.assertIn("`update_repository` with `team`", text)


def sentences(text):
    """The sentences of a Markdown text: each line, list item or table
    cell split at a full stop, a question mark or a colon at its end."""
    found = []
    for line in text.splitlines():
        line = re.sub(r"^\s*(?:[-*]|\d+\.)\s+", "", line).strip()
        if not line or line.startswith(("#", "```", "|")):
            continue
        for part in re.split(r"(?<=[.?!:;])\s+", line):
            if part.strip():
                found.append(part.strip())
    return found


class SeamsAndLoopExits(unittest.TestCase):
    """KAIROS-T-0547: `tdd` takes the seams that the task names, with no
    confirmation, so a loop can run it; Ralph has a default iteration
    limit, a NEED INPUT exit and a cancel by folder."""

    def text(self, rel):
        return read(os.path.join(SKILLS, rel))

    def seams_section(self):
        body = self.text("engineering/tdd/SKILL.md")
        match = re.search(r"## Seams.*?\n(.*?)\n## ", body, re.S)
        self.assertIsNotNone(match)
        return match.group(1)

    def test_the_tdd_trigger_matches_the_body(self):
        fields = frontmatter(os.path.join(SKILLS, "engineering", "tdd", "SKILL.md"))
        self.assertIn('"red-green"', fields["description"])
        self.assertNotIn("refactor", fields["description"])

    def test_tdd_takes_the_seams_that_the_task_names(self):
        seams = self.seams_section()
        for text in (
            "the task names",
            "acceptance criteria",
            "notes",
            "`get_item`",
            "do not ask the user to confirm",
            "`append_note`",
            "<promise>NEED INPUT</promise>",
        ):
            with self.subTest(text=text):
                self.assertIn(text, seams)
        self.assertNotIn("No test is written at an unconfirmed seam", seams)

    def test_the_new_text_has_short_sentences(self):
        texts = {
            "tdd seams": self.seams_section(),
            "cancel-ralph": self.text("workflow/cancel-ralph/SKILL.md").split("---", 2)[2],
        }
        for name, text in texts.items():
            for sentence in sentences(text):
                with self.subTest(skill=name, sentence=sentence[:60]):
                    self.assertLessEqual(len(sentence.split()), 20)

    def test_each_ralph_skill_names_the_limit_and_the_need_input_exit(self):
        for rel in ("workflow/ralph/SKILL.md", "workflow/ralph-initiative/SKILL.md"):
            with self.subTest(skill=rel):
                text = self.text(rel)
                self.assertIn("<promise>NEED INPUT</promise>", text)
                self.assertIn("the question on the task with `append_note`", text)
                self.assertIn("25 by default", text)
                self.assertIn("`--max-iterations 0`", text)
                self.assertNotIn("no limit by default", text)

    def test_cancel_ralph_finds_a_loop_by_folder(self):
        text = self.text("workflow/cancel-ralph/SKILL.md")
        self.assertIn("`/clear`", text)
        self.assertIn("folder", text)
        self.assertIn("--loop", text)
        self.assertIn("$ARGUMENTS", text)

    def test_the_readme_and_the_router_name_the_new_exit(self):
        self.assertIn("<promise>NEED INPUT</promise>", read(os.path.join(PLUGIN, "README.md")))
        self.assertIn("NEED INPUT", self.text("meta/kairos/SKILL.md"))


def load_channel():
    """The module of `plugin/bin/kairos-channel` (a script with no `.py`)."""
    import importlib.machinery
    import importlib.util

    loader = importlib.machinery.SourceFileLoader("kairos_channel", os.path.join(PLUGIN, "bin", "kairos-channel"))
    module = importlib.util.module_from_spec(importlib.util.spec_from_loader("kairos_channel", loader))
    loader.exec_module(module)
    return module


class OnePushPolicy(unittest.TestCase):
    """KAIROS-T-0542, KAIROS-A-0046: `implement` states the push policy once
    (push the feature branch, open a draft PR, `set_state`, stop); Ralph,
    the router and the dispatch channel name the same policy."""

    def implement_section(self):
        body = read(os.path.join(SKILLS, "workflow", "implement", "SKILL.md"))
        match = re.search(r"## Push and open the PR\n(.*?)\n## ", body, re.S)
        self.assertIsNotNone(match)
        return body, match.group(1)

    def test_implement_states_the_policy_once(self):
        body, section = self.implement_section()
        self.assertEqual(body.count("## Push and open the PR"), 1)
        for text in (
            "`git -C <worktree> push -u origin <code>`",
            "`gh pr create --draft",
            "`set_state` with each field",
            "`pushed: true`",
            "`pr_url`",
            "Stop at the PR",
            "Do not merge, release, tag or deploy",
            "Do not push the default branch",
            "`--force`",
            "KAIROS-A-0046",
        ):
            with self.subTest(text=text):
                self.assertIn(text, section)
        # The gate points at the section; it does not restate it.
        self.assertIn('Do "Push and open the PR", then "Close the work"', body)

    def test_the_section_has_short_sentences(self):
        _, section = self.implement_section()
        for sentence in sentences(section):
            with self.subTest(sentence=sentence[:60]):
                self.assertLessEqual(len(sentence.split()), 20)

    def test_ralph_pushes_and_does_not_stop_at_a_feature_push(self):
        ralph = read(os.path.join(HERE, "ralph.py"))
        texts = {
            "ralph.py": ralph.split("PROCEDURE = ", 1)[1].split('"""', 2)[1],
            "ralph": read(os.path.join(SKILLS, "workflow", "ralph", "SKILL.md")),
            "ralph-initiative": read(os.path.join(SKILLS, "workflow", "ralph-initiative", "SKILL.md")),
        }
        for name, text in texts.items():
            with self.subTest(text=name):
                self.assertNotIn("a design decision, a push,", text)
                self.assertIn("a push that the git guard asks for", text)
                self.assertIn("draft PR", text)
                self.assertIn("KAIROS-A-0046", text)
        self.assertIn("`git -C <worktree> push -u origin <code>`", texts["ralph.py"])
        self.assertIn("`gh pr create --draft`", texts["ralph.py"])
        self.assertNotIn("a push, a deploy", read(os.path.join(SKILLS, "meta", "kairos", "SKILL.md")))

    def test_the_channel_names_the_same_policy(self):
        flat = load_channel().INSTRUCTIONS
        for phrase in (
            "Push and open the PR section of kairos:implement",
            "git push -u origin <branch>",
            "draft pull request",
            "set_state",
            "no merge, no release, no tag",
        ):
            with self.subTest(phrase=phrase):
                self.assertIn(phrase, flat)


class SkillsWriteNoRepoFileForWiringOrNotes(unittest.TestCase):
    """KAIROS-T-0527: bootstrap keeps the wiring out of git through the
    local exclude of the clone, never the tracked .gitignore; prototype
    keeps its answer in Kairos, never in a NOTES.md next to the code."""

    def test_bootstrap_writes_the_local_exclude_not_gitignore(self):
        text = read(os.path.join(PLUGIN, "skills", "meta", "bootstrap", "SKILL.md"))
        self.assertIn("info/exclude", text)
        self.assertIn("git rev-parse --git-common-dir", text)
        self.assertNotIn("If `.gitignore` does not already cover it, append", text)
        self.assertIn("not a git checkout", text)

    def test_prototype_keeps_its_answer_in_kairos(self):
        text = read(os.path.join(PLUGIN, "skills", "engineering", "prototype", "SKILL.md"))
        self.assertNotIn("NOTES.md", text)
        self.assertIn("append_note", text)

class AgentKeyHowToMatchesTheManifest(unittest.TestCase):
    """KAIROS-T-0600: the how-to "Give an agent your key" describes the
    connection that `.claude-plugin/plugin.json` ships: the plugin's own
    `kairos` server, fed by the variables of the user settings. Since #270
    that server is `kairos mcp-proxy`, which reads KAIROS_URL and
    KAIROS_MCP_KEY itself (`crate::auth`), and the wiring is a project entry
    of the config of the person."""

    REPO = os.path.dirname(PLUGIN)
    HOW_TO = os.path.join(REPO, "docs", "src", "content", "docs", "how-to", "give-an-agent-your-key.md")
    MANIFEST = os.path.join(REPO, ".claude-plugin", "plugin.json")
    PROXY = os.path.join(REPO, "crates", "kairos-cli", "src", "commands", "mcp_proxy.rs")

    def test_the_how_to_names_the_variables_of_the_plugin_server(self):
        import json

        manifest = json.load(open(self.MANIFEST))
        server = manifest["mcpServers"]["kairos"]
        self.assertEqual(server["command"], "${CLAUDE_PLUGIN_ROOT}/plugin/bin/kairos-mcp")
        launcher = read(os.path.join(PLUGIN, "bin", "kairos-mcp"))
        self.assertIn('"mcp-proxy"', launcher)
        proxy = read(self.PROXY)
        text = read(self.HOW_TO)
        for name in ("KAIROS_URL", "KAIROS_MCP_KEY"):
            self.assertIn(name, proxy, f"the proxy must name {name}")
            self.assertIn(name, text, f"the how-to must name {name}")
        self.assertIn("kairos mcp-proxy", text)
        self.assertIn("config.toml", text)
        self.assertIn("writes only", text)
        self.assertNotIn("writes this entry", text)

if __name__ == "__main__":
    unittest.main()
