---
name: domain-modeling
description: Build and sharpen a project's domain model. Use when the user wants to pin down domain terminology or a ubiquitous language, maintain the glossary in CONTEXT.md, record an architectural decision as an ADR, or when another skill needs to maintain the domain model.
---

# Domain Modeling

Actively build and sharpen the project's domain model as you design. This is the *active* discipline — challenging terms, inventing edge-case scenarios, and writing the glossary and decisions down the moment they crystallise. (Merely *reading* `CONTEXT.md` for vocabulary is not this skill — that's a one-line habit any skill can do. This skill is for when you're changing the model, not just consuming it.)

## Where the model lives

The glossary is a repo file; the decisions are Kairos ADRs.

Most repos have a single context — one `CONTEXT.md` at the root:

```
/
├── CONTEXT.md
└── src/
```

If a `CONTEXT-MAP.md` exists at the root, the repo has multiple contexts. The map points to where each one lives:

```
/
├── CONTEXT-MAP.md
└── src/
    ├── ordering/
    │   └── CONTEXT.md
    └── billing/
        └── CONTEXT.md
```

Create files lazily — only when you have something to write. If no `CONTEXT.md` exists, create one when the first term is resolved. Decisions never get repo files: record them as Kairos ADRs per [ADR-FORMAT.md](./ADR-FORMAT.md).

## During the session

### Challenge against the glossary

When the user uses a term that conflicts with the existing language in `CONTEXT.md`, call it out immediately. "Your glossary defines 'cancellation' as X, but you seem to mean Y — which is it?"

### Sharpen fuzzy language

When the user uses vague or overloaded terms, propose a precise canonical term. "You're saying 'account' — do you mean the Customer or the User? Those are different things."

### Discuss concrete scenarios

When domain relationships are being discussed, stress-test them with specific scenarios. Invent scenarios that probe edge cases and force the user to be precise about the boundaries between concepts.

### Cross-reference with code

When the user states how something works, check whether the code agrees. If you find a contradiction, surface it: "Your code cancels entire Orders, but you just said partial cancellation is possible — which is right?"

### Update CONTEXT.md inline

When a term is resolved, update `CONTEXT.md` right there. Don't batch these up — capture them as they happen. Use the format in [CONTEXT-FORMAT.md](./CONTEXT-FORMAT.md).

`CONTEXT.md` should be totally devoid of implementation details. Do not treat `CONTEXT.md` as a spec, a scratch pad, or a repository for implementation decisions. It is a glossary and nothing else.

### Offer an ADR when a reader would get it wrong

Offer an ADR when a reader of the code would get the decision wrong without it. The reader can be a person or a later agent. That includes:

- **A choice against a library default.** The reader expects the default and "fixes" the code.
- **A rejected option that will come back.** Someone will suggest it again.
- **A decision of the user.** The code does not show why. Examples: the alias list of KAIROS-T-0673, the reference forms of KAIROS-T-0689.

Skip routine bug fixes. Skip the choice that any reader expects. Create the ADR per [ADR-FORMAT.md](./ADR-FORMAT.md).

### Cite, do not copy

A code comment or a test that depends on the decision names the ADR short code, for example `// KAIROS-A-0069`. It does not repeat the reasoning. The ADR holds the reason; the citation leads the reader to it.

## Record this decision

One short path when the user says "record this decision", or when `implement` finds one at the end of a task (COLLIERY-I-0628). A decision about the system under work goes to Kairos, not to local memory.

1. Find the ADR board of the team: `adr_board` in the SessionStart context, else `my_boards`.
2. Draft from the conversation: the title (the decision in one clause), the context (what was true, what forced the choice), the decision, the consequences (what changes for the code and the people). Show the draft to the user. Keep the words the conversation used.
3. `create_item` with `item_type: adr`, `board: <adr board>`, `parent: <the task or the initiative that produced it>`, the draft as `content` ([ADR-FORMAT.md](./ADR-FORMAT.md)).
4. `link_items` with `impacts` from the ADR to each repository that it is about.
5. A decision that changes an existing ADR. It changes a part of it: `link_items` with `amends` from the new ADR to the old one, and the new ADR names the part. It replaces all of it: `link_items` with `supersedes` from the new ADR to the old one. The old one stays as it is (KAIROS-A-0069).
6. Show the short code of the ADR, and write it on the task or the initiative.
7. A spec or a plan that the decision needs is a document, not an ADR. `promote_note` with `kind: document` makes a draft. When the user agrees with the text, `set_lifecycle` with `lifecycle: published` publishes it. A document is not on a board, so `transition_item` does not move it.
