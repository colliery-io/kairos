# ADR Format

ADRs are Kairos items, not repo files. To record one:

1. `create_item` with `item_type: adr`, the decision's short title as `title`, the template below as `content`, and `decision_maker` when known. Send `parent`: the item whose work produced the decision (the task, the initiative, or the other item under discussion). Kairos makes a `supports` edge from the ADR to it, and `get_item` on that item lists the ADR. With no such item, send no `parent`. Send `board`: for a decision about the delivery work of your team, the ADR board of your team (`adr_board` in the SessionStart context; `my_boards` names the team of each ADR board). Its ADRs have the prefix of the team. For a decision of the organization, the ADR board of the organization.
2. `link_items` (`impacts`) from the new ADR to each repository that the decision is about: `target` is the slug of the repository. `get_repository` then lists the decision for each agent that works in that repository. The link gives no right on the ADR. If the decision is about no repository, skip the link.

Kairos assigns the short code; there is no manual numbering and no `docs/adr/` directory. In a multi-context repo (one with a `CONTEXT-MAP.md`), name the bounded context in the ADR's title so the decision's scope stays visible.

## Template (the item's content)

```md
{1-3 sentences: what's the context, what did we decide, and why.}
```

That's it. An ADR can be a single paragraph. The value is in recording *that* a decision was made and *why* — not in filling out sections.

## Optional sections

Only include these when they add genuine value. Most ADRs won't need them.

- **Considered Options** — only when the rejected alternatives are worth remembering
- **Consequences** — only when non-obvious downstream effects need to be called out

Status lives in the system, not the content: the ADR's board column tracks its lifecycle. To revisit a decision, create a new ADR and never rewrite the old one (KAIROS-A-0069):

- The new ADR changes a part of the old one, and the rest stays in force: `link_items` (`amends`) from the new one to the old. Name the part in the new ADR ("amends rule 6 of KAIROS-A-0021"). `get_item` of the old ADR lists its amendments, oldest first.
- The new ADR replaces all of the old one: `link_items` (`supersedes`) from the new one to the old.

## When to offer an ADR

Offer an ADR when a reader of the code would get the decision wrong without it. The reader can be a person or a later agent. That includes:

1. **A choice against a library default.** The reader expects the default and "fixes" the code.
2. **A rejected option that will come back.** Someone will suggest it again in six months.
3. **A decision of the user.** The code does not show why, and a later agent would undo it. Examples: the alias list of KAIROS-T-0673, the reference forms of KAIROS-T-0689.

Skip routine bug fixes. Skip the choice that any reader expects: there is nothing to record beyond "we did the obvious thing."

## Cite it, do not copy it

The code that depends on the decision cites the ADR by its short code: a code comment (`// KAIROS-A-0069: amend for part, supersede for all`) or the name or doc comment of a test. The citation does not repeat the reasoning. The ADR holds it, and a copy goes stale when the ADR is amended.

### What qualifies

- **Architectural shape.** "We're using a monorepo." "The write model is event-sourced, the read model is projected into Postgres."
- **Integration patterns between contexts.** "Ordering and Billing communicate via domain events, not synchronous HTTP."
- **Technology choices that carry lock-in.** Database, message bus, auth provider, deployment target. Not every library — just the ones that would take a quarter to swap out.
- **Boundary and scope decisions.** "Customer data is owned by the Customer context; other contexts reference it by ID only." The explicit no-s are as valuable as the yes-s.
- **Deliberate deviations from the obvious path.** "We're using manual SQL instead of an ORM because X." Anything where a reasonable reader would assume the opposite. These stop the next engineer from "fixing" something that was deliberate.
- **Constraints not visible in the code.** "We can't use AWS because of compliance requirements." "Response times must be under 200ms because of the partner API contract."
- **Rejected alternatives when the rejection is non-obvious.** If you considered GraphQL and picked REST for subtle reasons, record it — otherwise someone will suggest GraphQL again in six months.
