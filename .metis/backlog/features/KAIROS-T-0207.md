---
id: procedural-text-follows-asd-ste100
level: task
title: "Procedural text follows ASD-STE100, and the prompts say so"
short_code: "KAIROS-T-0207"
created_at: 2026-09-26T13:20:59.735888+00:00
updated_at: 2026-09-26T19:56:01.045029+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#feature"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: NULL
---

# Procedural text follows ASD-STE100, and the prompts say so

## Objective

Adopt ASD-STE100 (Simplified Technical English) for **procedural** text, and make the
plugin's skills instruct agents to write that way. Today there is no writing-style
guidance anywhere in `plugin/skills/` — twenty skills, none of them saying how to
write.

## Decisions (Dylan, 2026-09-26)

| Question | Decision |
|---|---|
| Scope | **Procedural text only** |
| Strictness | **Full STE, including the dictionary** |

### Where it applies, and where it must not

STE is a controlled language for aircraft maintenance procedures. It is excellent at
instructions and actively hostile to argument, so the scope follows the Diátaxis
split this repo already uses ([[KAIROS-S-0008]]):

| STE applies | STE does not apply |
|---|---|
| `tutorials/`, `how-to/`, `reference/` | `explanation/` |
| acceptance criteria | ADRs |
| error messages, CLI help | task Status Updates |
| | commit messages |

The exclusions are the point rather than an escape hatch. A ~900-word vocabulary
cannot express "this amends an absolute into a default", or "that argument does not
survive multiple issuers" — and those sentences are most of what the ADRs and the
status updates are *for*. Constraining them would flatten the reasoning this
repository keeps deliberately.

### Full STE means a dictionary, which is the real cost

Chosen over the rules-only option, so this is not just a list of sentence rules.
It needs:

- **The approved vocabulary** — one meaning per approved word, and a rule that
  unapproved words are replaced rather than explained.
- **A Technical Names list.** Every Kairos domain term is a Technical Name under STE
  and none are in the approved dictionary: *tenant, organization, initiative,
  strategy, board, column, capability, short code, delivery stream, proposal,
  session, issuer, claim*. This list has to be authored and maintained, and it is
  the part that makes compliance real rather than aspirational.
- **Technical Verbs** — the small set of domain verbs permitted beyond the approved
  list: *provision, transition, archive, revoke, grant, embed, propose, confirm*.

The writing rules themselves: one instruction per sentence; procedural sentences at
most 20 words; descriptive at most 25; active voice; present tense; no gerund chains;
articles never dropped; one term per concept, always, with no synonyms for variety.

## Implementation Notes

### It belongs in Metis first

`plugin/references/` are **rendered artifacts**, not sources — `plugin/README.md`
says so, and `architecture-review.md` / `diataxis.md` are rendered from
[[KAIROS-S-0007]] and [[KAIROS-S-0008]] by `scripts/render-references.sh`.

So the shape is settled by precedent: author a **specification** as the source of
truth, render it to `plugin/references/simplified-technical-english.md`, and have the
skills that produce procedural text point at it the way `diataxis-review/SKILL.md`
points at the Diátaxis reference — "the single source of truth … read it end to end
… this file only sequences the work."

### Which skills

Not all twenty. The ones that write or review procedural text:
`review/diataxis-review`, `workflow/decompose` and `workflow/triage` (acceptance
criteria), `workflow/implement`, and `engineering/grill-with-docs`. The others
(`meta/*`, `engineering/research`) mostly produce argument and should be left alone —
adding the instruction everywhere would be the flattening this decision avoids.

### The honest problem: compliance is not checkable by reading

Full STE is normally tool-assisted, and a rule nothing enforces decays. Options,
to weigh at pickup rather than now:

- A `angreal docs ste` check over `docs/src/{tutorials,how-to,reference}` for the
  mechanical rules — sentence length, passive voice, gerund chains. Catches most
  violations and cannot check vocabulary.
- Vocabulary checking needs the approved word list as data, which is licensed by
  ASD. **Worth confirming the licensing position before committing to ship a copy
  of it.**
- Review-only, accepting decay.

Whichever is chosen, say so — an unenforced style rule that everyone believes is
enforced is worse than an acknowledged aspiration.

## Acceptance Criteria

## Acceptance Criteria

- [x] A Metis specification is the source of truth: scope, the writing rules, the
      Technical Names and Technical Verbs lists — [[KAIROS-S-0009]]
- [x] Rendered to `plugin/references/` by `scripts/render-references.sh`, and the
      rendered file is not hand-edited
- [x] Only the procedural-text skills reference it, and the exclusions are stated in
      the specification so the omission reads as deliberate
- [x] The licensing position on redistributing the approved vocabulary is checked and
      recorded
- [x] How compliance is checked is decided and recorded, including "not checked" if
      that is the answer
- [x] `docs/src/` is **not** rewritten as part of this — the rule applies going
      forward, and a retrofit is its own task with its own review

## Status Updates

**2026-09-26 — filed mid-[[KAIROS-I-0018]].** Raised while the local-auth Ralph loop
was two tasks in. Filed rather than done immediately so the loop is not derailed, and
because "full STE including the dictionary" is a larger artefact than a line added to
some prompts.

Sequencing worth noting: [[KAIROS-T-0206]] writes how-to pages, which are procedural
and therefore in scope. Doing this first would mean those pages are written to the
rule instead of retrofitted.

### 2026-09-26 — done

[[KAIROS-S-0009]] is the specification, rendered to
`plugin/references/simplified-technical-english.md`, and five skills point at it.
`angreal docs ste` enforces the mechanical half.

#### The licensing answer changes the design

**ASD-STE100 has been free of charge since Issue 6 (2013), and its copyright is fully
owned by ASD.** Copies are distributed by the STE Maintenance Group on request, not
redistributed by third parties. The asd-ste100.org site returns 401 to an automated
fetch, so this was confirmed from secondary sources rather than from the licence text
itself — worth re-checking against the licence in a downloaded copy before anyone acts
on it further.

That settles the question the task left open: **Kairos ships no copy of the
dictionary.** So "full STE including the dictionary" is delivered as far as it can
honestly be — the specification carries the writing rules in our own words, plus the
two lists that are ours to maintain, and points a writer at asd-ste100.org for the
approved-word list.

The consequence is stated rather than buried: **STE-V1 cannot be checked here**, and
the checker says so on every run, in its own output. A clean run is not STE
conformance, and the specification's section 6.3 names this as the honest gap.

#### The specification

Every rule has an ID so a reviewer or the checker can cite it, following the
[[KAIROS-S-0008]] precedent. Sentences (STE-S1..S4), grammar (STE-G1..G6), words
(STE-V1..V4), procedures (STE-P1..P3).

Section 4 is the part that makes STE-V3 real: **23 Technical Names and 10 Technical
Verbs**, each with the synonyms that are now defects. The most useful entry is
`organization` over `tenant` — they are the same thing, and using both on one page
teaches a reader they are different. `tenant` is now reserved for writing about the
schema itself. Maintaining that list is part of adding a domain concept, which is
recorded as a review trigger.

Section 5 carries four worked before/after pairs, all taken from real text rather than
invented.

#### How compliance is checked: baselined, not big-bang

`scripts/ste-check.py`, wired up as `angreal docs ste` and **CI Gate 5**. It checks
STE-S2, STE-S4, STE-G1, STE-G4 and STE-V3 over `tutorials/`, `how-to/` and
`reference/` — and never reads `explanation/`, ADRs, Status Updates or commit
messages. If it ever starts to, that is a defect in the checker.

The corpus has **754 mechanical violations across 33 files** today.
`scripts/ste-baseline.json` records the count per file, and the gate fails only when a
file gets **worse**; a file with no baseline entry must be clean. That is how the rule
applies from today forward without the corpus rewrite the last acceptance criterion
forbids — and the baseline is a ceiling that only ever goes down. Verified both ways:
the gate passes at baseline, and appending one 23-word sentence to a clean file makes
it exit 1.

A retrofit of the 754 is a separate task with its own review, exactly as the criterion
says.

#### The five skills, and the one that needed care

`diataxis-review`, `decompose`, `triage`'s `ACCEPTANCE-CRITERIA.md`, `implement`, and
`grill-with-docs`. Nothing else references it, and every relative link was checked to
resolve.

Two of the five needed the **exclusions** stated as loudly as the rule:

- `diataxis-review` reviews explanation pages as well as procedural ones, so it is
  told that flagging an explanation page for a long sentence is a *false finding* —
  one that would flatten the reasoning the mode exists to carry.
- `grill-with-docs` produces glossary terms **and ADRs**, which follow opposite rules.
  Glossary definitions are reference text and follow STE; ADRs are explicitly out of
  scope, and the skill now says why — a controlled vocabulary cannot carry "this
  amends an absolute into a default".

`implement` is told that a how-to page, an error message and CLI help follow STE while
its commit message, code comments and the reasoning it records on the item do not.

#### Two defects found while building it

1. **The checker counted a link list as one sentence.** `prose_blocks` joined the lines
   of a block with spaces, so a six-item "Related" section read as one 28-word
   sentence. It was the only false positive on the one page written to the rule, which
   is what made it obvious. A list item is now its own block — which is also what
   STE-S1 means by a unit.
2. **`read_only=True` is not a valid `angreal.ToolDescription` kwarg** and failed the
   whole task file to load with a bare `TypeError`. `risk_level="read_only"` alone is
   the right form.

Also fixed: the five genuine violations the checker found on
`docs/src/how-to/use-local-accounts.md`, the page [[KAIROS-T-0206]] wrote to this rule
before the rule existed. Three "org admin" and two over-long sentences. That is
finishing that page rather than retrofitting the corpus, and it is why the file has no
baseline entry.

#### Left for Dylan

**[[KAIROS-S-0009]] is in `review`, not `published`.** It is a repository-wide writing
policy and nobody has reviewed it. [[KAIROS-S-0008]] is published; this should be too,
once read.

`angreal docs ste`, `docs build`, `test lint` and `test unit` are green.