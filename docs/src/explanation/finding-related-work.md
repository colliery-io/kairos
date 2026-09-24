# Finding related work

Kairos can tell an agent what a piece of work is probably related to. This page
is about why it is shaped the way it is, because the shape is counter-intuitive:
**a good answer here often looks like a weak one.**

## The problem is not search

Search finds what you ask for. The expensive failure in a work-management system
is different: it is the ticket nobody knew to look for. Two teams solving the
same problem in different words. A dependency real enough to break a release and
written down nowhere. A question somebody answered eighteen months ago and put
away.

A person half-remembers *"didn't we look at this?"*. An agent does not, and will
cheerfully re-solve last quarter's problem with complete confidence.

## The claim is a disagreement

Similarity on its own says nothing worth telling anyone. Two tickets about
billing are *supposed* to look alike — they are both about billing.

What is worth telling is where similarity and the graph **disagree**:

| similar, and… | what that suggests |
|---|---|
| nothing joins them | a dependency nobody drew |
| they share a parent | probably the same work written down twice |
| the other is finished or put away | prior art — somebody has been here |

This is why Kairos can do something a general-purpose search engine cannot. Its
graph is **authored**: a human drew every `parent` and `blocks` edge on purpose.
Most systems that do this have to extract a graph from prose first and inherit
every mistake that makes. Here the expensive half is already done, and the
communities already have names — an initiative and its children, a team's board,
a repository's tasks.

## Why you get proposals, not answers

Every result says *possible*. That is not hedging, it is the measurement.

We took every pair of 4,927 real work documents from nineteen repositories and
compared them:

| | average similarity |
|---|---|
| pairs the graph says are related | **0.80** |
| pairs with no relation at all | 0.68 |

Related work does score higher. But unrelated pairs reach **0.82** at the top of
their range — *higher than the average related pair*. The two distributions
overlap across their entire useful span. Repeating it on Kairos's own seeded data
gave the same answer: related 0.71–0.73, unrelated 0.58, and unrelated pairs
reaching 0.75 while genuinely linked pairs fell to 0.57.

**There is no number you could put between them.** Not a carefully tuned one, not
a per-tenant one. At the very top of the range, where the signal is strongest,
about half the pairs are genuinely related and about half are not.

So an agent told flatly *"this blocks you"* would be wrong about half the time,
and would act on it — which is worse than being told nothing. Everything here is
phrased as a suggestion because a suggestion is what the evidence supports.

## What this means for you

**Read the reasoning, not the ranking.** Each proposal says what matched, which
section it matched in, and what the graph did or did not know. That sentence is
the useful part; the order is a hint.

**"Nothing joins them" is weaker than it sounds.** It means no direct edge and no
shared parent — not that a path was searched for and missed. Work graphs are
sparse, so absence of a link is mostly absence of evidence.

**A short answer is not a broken one.** Results are bounded to a handful on
purpose. Forty related items is a research project, not an answer.

**Sometimes it will say it only searched the text.** That happens when a
deployment has no vectors yet, or has just changed models. The answer is real but
thinner — it will have missed work phrased differently — and it tells you so
rather than letting you assume otherwise.

## Confirming makes it better

When a suggestion is right, confirming it draws the edge for real. That is not
bookkeeping: graph distance is half the signal above, so every confirmation makes
the next answer better. The system improves by being used.

An agent cannot confirm its own suggestion. A wrong `parent` edge re-parents work
onto a board that then reports the wrong thing to the wrong people, and nobody
re-reads an edge once it exists. A proposal costs a click; a wrong edge costs a
conversation.

Rejections are kept, too. A pair that keeps being suggested and keeps being
rejected is the clearest evidence available that the retrieval is wrong about
something.

## Related reading

- [Connect over MCP](../how-to/connect-over-mcp.md) — the `related_work` and
  `propose_edge` tools an agent uses
- [Configure semantic retrieval](../how-to/configure-retrieval.md) — models,
  endpoints, and the backfill
- [Archiving](archiving.md) — why finished work stays findable, which is what
  makes prior art possible
- [Repositories as execution scope](repositories-as-execution-scope.md) — why a
  shared repository is a strong hint of real coupling
