# ADR Format

ADRs are Kairos items, not repo files. To record one:

1. `create_item` with `item_type: adr`, the decision's short title as `title`, the template below as `content`, and `decision_maker` when known.
2. `link_items` (`informs`) from the new ADR to the item whose work produced the decision — the task, initiative, or other item being discussed — so the decision stays discoverable from the work it shaped. If no such item exists, skip the link.

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

Status lives in the system, not the content: the ADR's board column tracks its lifecycle, and revisiting a decision means creating a new ADR and `link_items` (`supersedes`) from the new one to the old — never rewriting the old one.

## When to offer an ADR

All three of these must be true:

1. **Hard to reverse** — the cost of changing your mind later is meaningful
2. **Surprising without context** — a future reader will look at the code and wonder "why on earth did they do it this way?"
3. **The result of a real trade-off** — there were genuine alternatives and you picked one for specific reasons

If a decision is easy to reverse, skip it — you'll just reverse it. If it's not surprising, nobody will wonder why. If there was no real alternative, there's nothing to record beyond "we did the obvious thing."

### What qualifies

- **Architectural shape.** "We're using a monorepo." "The write model is event-sourced, the read model is projected into Postgres."
- **Integration patterns between contexts.** "Ordering and Billing communicate via domain events, not synchronous HTTP."
- **Technology choices that carry lock-in.** Database, message bus, auth provider, deployment target. Not every library — just the ones that would take a quarter to swap out.
- **Boundary and scope decisions.** "Customer data is owned by the Customer context; other contexts reference it by ID only." The explicit no-s are as valuable as the yes-s.
- **Deliberate deviations from the obvious path.** "We're using manual SQL instead of an ORM because X." Anything where a reasonable reader would assume the opposite. These stop the next engineer from "fixing" something that was deliberate.
- **Constraints not visible in the code.** "We can't use AWS because of compliance requirements." "Response times must be under 200ms because of the partner API contract."
- **Rejected alternatives when the rejection is non-obvious.** If you considered GraphQL and picked REST for subtle reasons, record it — otherwise someone will suggest GraphQL again in six months.
