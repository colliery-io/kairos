---
id: procedural-text-follows-asd-ste100
level: task
title: "Procedural text follows ASD-STE100, and the prompts say so"
short_code: "KAIROS-T-0207"
created_at: 2026-09-26T13:20:59.735888+00:00
updated_at: 2026-09-26T13:20:59.735888+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/backlog"
  - "#feature"


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

- [ ] A Metis specification is the source of truth: scope, the writing rules, the
      Technical Names and Technical Verbs lists
- [ ] Rendered to `plugin/references/` by `scripts/render-references.sh`, and the
      rendered file is not hand-edited
- [ ] Only the procedural-text skills reference it, and the exclusions are stated in
      the specification so the omission reads as deliberate
- [ ] The licensing position on redistributing the approved vocabulary is checked and
      recorded
- [ ] How compliance is checked is decided and recorded, including "not checked" if
      that is the answer
- [ ] `docs/src/` is **not** rewritten as part of this — the rule applies going
      forward, and a retrofit is its own task with its own review

## Status Updates

**2026-09-26 — filed mid-[[KAIROS-I-0018]].** Raised while the local-auth Ralph loop
was two tasks in. Filed rather than done immediately so the loop is not derailed, and
because "full STE including the dictionary" is a larger artefact than a line added to
some prompts.

Sequencing worth noting: [[KAIROS-T-0206]] writes how-to pages, which are procedural
and therefore in scope. Doing this first would mean those pages are written to the
rule instead of retrofitted.
