# Diataxis Documentation Spec

Source of truth for the `diataxis-review` skill. Based on the Diataxis framework (Daniele Procida, diataxis.fr). Every rule below is checkable: a reviewer must be able to cite a rule ID and point to text on the page that satisfies or violates it. Rendered from KAIROS-S-0008 (source of truth) — do not edit here.

## 1. The compass

Documentation serves users along two axes:

- **Action vs. cognition**: is the user *doing* something (practical steps) or *thinking* about something (theoretical knowledge)?
- **Acquisition vs. application**: is the user *studying* (acquiring skill/knowledge) or *working* (applying skill/knowledge to a real task)?

The axes yield exactly four modes. Every documentation page must serve one — and only one — mode.

|                 | **Acquisition (study)**            | **Application (work)**              |
|-----------------|------------------------------------|-------------------------------------|
| **Action**      | **Tutorial** — learning-oriented   | **How-to guide** — task-oriented    |
| **Cognition**   | **Explanation** — understanding-oriented | **Reference** — information-oriented |

**Governing principle**: serve the user's need *at the moment of reading*. A learner following a tutorial, a practitioner mid-task, someone looking up a fact, and someone seeking understanding are four different people (often the same person at different moments). A page that tries to serve two moments at once serves neither. When applying the compass: if the content informs *action*, it is a tutorial or how-to; if it informs *cognition*, it is reference or explanation. If it serves *acquisition*, it is a tutorial or explanation; if *application*, a how-to or reference.

## 2. Per-mode contract

### 2.1 Tutorial (learning-oriented)

- **User**: a beginner who does not yet know what they need to know. They are not working; they are learning by doing.
- **Obligation**: the author takes full responsibility for the learner's success. A tutorial is a lesson: the author decides what the learner does, and every step must work, every time, for every learner.
- **Voice**: first-person plural ("we"), warm and encouraging. Concrete, particular actions — never abstract choices. Tell the learner what they will see and confirm what just happened ("You should now see..."). Minimal explanation: link out instead.

Criteria (cite as T1–T6):
- **T1**: Delivers a guaranteed, visible result — the outcome is stated up front and every step contributes to it.
- **T2**: Every step is a concrete action the learner performs; no step offers alternatives, options, or "if you prefer" branches.
- **T3**: The learner sees results early and often; each action's expected output is shown or described so the learner can self-check.
- **T4**: Explanation is minimal — only what is needed to complete the step; anything more is a link to explanation pages.
- **T5**: Assumes only stated prerequisites; no unexplained jargon, no reliance on knowledge the tutorial did not provide or declare.
- **T6**: Repeatable: no steps that depend on environment specifics, timing, or external state the tutorial does not control or pin down.

### 2.2 How-to guide (task-oriented)

- **User**: a competent practitioner at work, with a real-world goal already in mind. They know what they want; they need the sequence to get there.
- **Obligation**: address the user's goal, not the tool's features. The guide navigates a real problem, including its messy real-world edges.
- **Voice**: imperative ("Configure the...", "Run..."). No teaching, no persuasion. Assumes competence: does not define basics or motivate the task.

Criteria (cite as H1–H6):
- **H1**: Title names a real-world goal ("How to migrate X to Y"), not a product feature ("The Y importer").
- **H2**: Opens by stating the goal and any preconditions, then goes straight to steps; no conceptual preamble.
- **H3**: Steps are an ordered, executable sequence; each step is an action, and the sequence demonstrably achieves the titled goal.
- **H4**: Handles legitimate variation via explicit conditionals ("If you use X, do A; otherwise B") — forks are decision points for the working user, not teaching digressions.
- **H5**: Omits the inessential: no completeness for its own sake, no background theory, no definitions of terms a competent user knows.
- **H6**: Assumes prior competence and says so where needed; never walks the user through learning material mid-task.

### 2.3 Reference (information-oriented)

- **User**: a practitioner at work who needs a fact — a parameter, a limit, a signature, a default — and needs to trust it.
- **Obligation**: describe the machinery, truthfully and completely. Reference is shaped by the product, not by the author's ideas or the user's tasks.
- **Voice**: austere, neutral, uniform. States facts ("Command X accepts flags A, B, C"). No instruction, no opinion, no persuasion.

Criteria (cite as R1–R6):
- **R1**: Structure mirrors the structure of the product itself (module/command/endpoint/config hierarchy), so location is predictable.
- **R2**: Content is description only: statements of fact about what exists and what it does. No step sequences, no "you should".
- **R3**: Consistent format throughout: identical section patterns, tables, and ordering across sibling entries.
- **R4**: Complete for its scope: every parameter, return value, error, default, and constraint is listed; nothing is "documented elsewhere only".
- **R5**: Examples, if present, illustrate usage of the described item and remain brief; they never grow into procedures or lessons.
- **R6**: Accurate and current: matches the product version it claims to document; version/applicability is stated where behavior differs.

### 2.4 Explanation (understanding-oriented)

- **User**: a reader at leisure (not mid-task) who wants to understand: why it is this way, how it fits together, what the trade-offs are.
- **Obligation**: deepen and broaden understanding of a topic. Answers "why"; provides context, background, history, alternatives, and connections.
- **Voice**: discursive prose. May and should make connections to other concepts, other systems, and design history. May admit opinion and perspective ("The W design was chosen because...", "Some prefer Z").

Criteria (cite as E1–E6):
- **E1**: Topic is bounded and named as a topic ("About caching", "Why X uses Y"), not as a task or an API surface.
- **E2**: Answers "why": design rationale, trade-offs, constraints, or history appear explicitly.
- **E3**: Makes connections: relates the topic to other parts of the system or to outside concepts/standards.
- **E4**: Readable away from the product: contains no required steps and no material the reader must execute to follow the argument.
- **E5**: Considers alternatives or multiple perspectives where they exist, and may take a position — marked as such.
- **E6**: Contains no canonical facts that exist only here: specifics (limits, defaults, signatures) are cited from or delegated to reference.

## 3. Classification heuristics

A reviewer must determine two things per page: the **declared mode** (what the page is trying to be) and the **actual mode(s)** (what its content is). They frequently differ.

Declared mode — read in this order:
1. **Location**: which docs-tree section the page sits in (tutorials/, how-to/, reference/, explanation/ or equivalents).
2. **Title pattern**: "Getting started with…", "Your first…", "Learn…" → tutorial. "How to…", gerund goals ("Deploying…", "Migrating…") → how-to. Noun phrases naming product parts ("CLI reference", "`config.yaml` options", "API: /users") → reference. "About…", "Understanding…", "Why…", "X in depth", "Architecture" → explanation.
3. **Opening sentences**: "In this tutorial you will…" → tutorial. "This guide shows you how to…" → how-to. "This page lists/describes…" → reference. "This article discusses/explains…" → explanation.

Actual mode — classify each block (paragraph, list, table, code block) by its dominant signal:
- Numbered imperative steps with expected outputs, "we", reassurance → tutorial content.
- Numbered/ordered imperatives assuming context, goal-directed, conditionals for variants → how-to content.
- Tables, parameter lists, signatures, enumerations, uniform declarative sentences → reference content.
- Multi-sentence prose paragraphs of rationale, comparison, history, "because", "the reason" → explanation content.

Tutorial vs. how-to disambiguator: does it teach a beginner (chosen exercise, guaranteed result, explains what the reader sees) or serve a worker (user's own goal, assumes competence, tolerates variation)? Reference vs. explanation disambiguator: does it state what *is* (lookup-shaped, product-shaped) or discuss *why* (argument-shaped, topic-shaped)?

Report the actual mode as a distribution when mixed (e.g., "60% reference, 40% explanation"). A page is **misaligned** when its declared mode ≠ its dominant actual mode, or when >~20% of its content belongs to another mode.

## 4. Mode-mixing anti-patterns

### 4.1 Tutorial that explains too much
- **Detect**: between steps, paragraphs of background, theory, or design rationale; violates T4. Learner must read prose that doesn't change what they do next.
- **Split**: keep the step sequence; move each digression to an explanation page and replace it with one orienting sentence plus a link.

### 4.2 How-to that teaches
- **Detect**: defines basic terms, motivates the task at length, tours features, or explains concepts mid-procedure; violates H5/H6. Often opens with "Before we begin, let's understand…".
- **Split**: strip pedagogy; state prerequisites as links ("Assumes familiarity with X — see [Understanding X]"). If the page is really aimed at beginners, reclassify and rewrite as a tutorial instead.

### 4.3 Reference that instructs
- **Detect**: step sequences, "you should", setup walkthroughs, or task advice inside parameter tables and API entries; violates R2/R5.
- **Split**: extract each procedure into a how-to guide; leave the facts, plus a "Guides" link list. Keep only short illustrative examples (R5).

### 4.4 Explanation buried in reference
- **Detect**: rationale, history, trade-off discussion, or "why we designed it this way" paragraphs inside reference entries; violates R2 and starves the explanation quadrant (readers can't find the discussion, and it bloats lookup).
- **Split**: move discursive prose to an explanation page on that topic; cross-link both ways. Canonical facts stay in reference (E6).

### 4.5 The "all four in one page" quickstart
- **Detect**: a single page (often "Getting Started" or "Quickstart") that installs and teaches (tutorial), covers variant tasks (how-to), tabulates options (reference), and discusses architecture (explanation). Sectional whiplash: numbered steps → option table → "Why this design" prose.
- **Split**: into up to four pages, one per mode, each keeping its title conventions (Section 5). The quickstart itself may survive only as a short tutorial (T1–T6) linking outward.

**General rule**: never fix mode-mixing by deleting content. Relocate it to the quadrant it belongs to; the misplaced material is usually valuable, just misfiled.

## 5. Structural review

Beyond per-page review, assess the tree:

- **S1 — Quadrant coverage**: for each major topic/feature, check which of the four modes exist. Build a topic × mode matrix; empty cells are gaps. Prioritize: no tutorial at all for the product (new users blocked); no reference for a public surface (facts uncheckable); how-to gaps for common tasks; explanation gaps for contentious or surprising designs.
- **S2 — Top-level separation**: the four modes are distinct, findable sections in navigation (names may vary: "Guides" for how-to, "Concepts"/"Discussion" for explanation). Modes are not interleaved in one flat list or nested inside each other.
- **S3 — Naming conventions**: titles signal mode (Section 3 patterns) consistently across the tree. Flag section names that promise one mode and contain another ("Tutorials" full of how-tos is a common failure).
- **S4 — Cross-linking discipline**: tutorials and how-tos link *out* to reference and explanation rather than inlining them; reference links to how-tos for procedures; explanation cites reference for facts. Flag both missing links (dead ends) and inlined content that should be links.
- **S5 — No junk drawer**: flag catch-all sections ("Misc", "Advanced", "FAQ" used as a dumping ground) whose contents should be classified into the four modes. FAQs are acceptable only as thin pointers into the four quadrants.
- **S6 — Proportion**: no single mode dominates to the exclusion of others without justification (an internal API may legitimately skip tutorials; a user-facing product may not).

## 6. Finding format

Every finding the skill emits MUST contain these fields:

- **Page**: path or URL of the page.
- **Declared mode**: Tutorial | How-to | Reference | Explanation | None/Ambiguous — with the signal used (location, title, opening).
- **Actual mode(s)**: dominant mode plus distribution if mixed (e.g., "How-to 70% / Explanation 30%").
- **Violations**: specific criterion IDs (T1–T6, H1–H6, R1–R6, E1–E6, S1–S6) with a short quote or pointer to the offending text for each.
- **Proposed action**: exactly one of —
  - *Rewrite in mode M* (content belongs to one mode; form is wrong),
  - *Split into N pages* (list the target pages and their modes),
  - *Move content to X* (name the destination page/section per mode),
  - *Create missing page* (for S1 gaps: topic + mode),
  - *Restructure navigation* (for S2/S3/S5 findings).
- **Severity**:
  - **Blocking confusion**: the page cannot serve its user's moment — a tutorial that can fail (T1/T6), reference that is wrong or incomplete (R4/R6), a how-to whose steps don't reach the goal (H3), or an entire missing quadrant for a core topic (S1).
  - **Degrading**: the page works but costs the user — mode-mixing over ~20%, teaching in how-tos, instructing in reference, missing cross-links, misleading titles.
  - **Cosmetic**: voice/format drift within the correct mode — wrong person, inconsistent reference formatting (R3), title-pattern deviations with correct content.

Order findings by severity, then by page. Cite this spec's rule IDs in every finding; a finding with no rule ID is invalid.