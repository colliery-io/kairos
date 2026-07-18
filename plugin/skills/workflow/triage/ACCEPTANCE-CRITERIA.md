# Writing Acceptance Criteria

Acceptance criteria written during triage are the contract the implementing agent works from. The item's discussion and history are context — the criteria are what "done" means. Write them into the item's content (`edit_item`) under an `## Acceptance Criteria` heading, as a checklist.

## Principles

### Durability over precision

The item may sit in Todo for days or weeks while the codebase changes underneath it. Write criteria that stay true through renames, moves, and refactors:

- **Do** describe interfaces, types, and behavioral contracts by name
- **Don't** reference file paths or line numbers — they go stale
- **Don't** assume the current implementation structure survives

### Behavioral, not procedural

Describe **what** the system should do, not **how** to implement it. The implementer explores the codebase fresh and makes its own implementation decisions.

- **Good:** "The `SkillConfig` type accepts an optional `schedule` field of type `CronExpression`"
- **Bad:** "Open src/types/skill.ts and add a schedule field on line 42"

### Independently verifiable

The implementer must be able to demonstrate each criterion with a command and observed output. Each criterion is a checkable observation, not a feeling.

- **Good:** "Running the item-list command with `--json` emits valid JSON for both success and error cases"
- **Bad:** "JSON output works correctly"

### Explicit scope boundaries

State what is out of scope in an `## Out of Scope` section beside the criteria. This stops the implementer gold-plating or absorbing adjacent features.

## Template

```markdown
## Summary

One line: what needs to happen.

## Current behavior

What happens now. For a Bug, the broken behavior (with the verified
reproduction from triage). For a Task, the status quo the change builds on.

## Desired behavior

What happens after the work is complete, including edge cases and error
conditions.

## Key interfaces

- `TypeName` — what changes and why
- `functionName()` — what it returns now vs what it should return

## Acceptance Criteria

- [ ] Specific, verifiable criterion 1
- [ ] Specific, verifiable criterion 2

## Out of Scope

- Thing that must NOT change under this item
- Adjacent feature that seems related but is separate
```

## Example

```markdown
## Summary

Description truncation drops mid-word, producing broken output.

## Current behavior

Descriptions over 1024 characters are cut at exactly 1024 characters
regardless of word boundaries, ending mid-word ("…wants to confi").
Verified during triage: reproduced against the seeded demo data.

## Desired behavior

Truncation breaks at the last word boundary before 1024 characters and
appends "..." to signal truncation.

## Key interfaces

- The validation logic that populates the metadata `description` field —
  no type change, but it must respect word boundaries

## Acceptance Criteria

- [ ] Descriptions under 1024 chars pass through unchanged
- [ ] Descriptions over 1024 chars break at the last word boundary before 1024
- [ ] Truncated descriptions end with "..." and total ≤ 1024 chars

## Out of Scope

- Changing the 1024-char limit itself
- Multi-line description support
```

If a criterion can't be demonstrated with a command and its output, rewrite it until it can.
