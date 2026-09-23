# Archiving: what it means to put work away

Kairos has one answer to what archiving means, and it is deliberately a small
one:

> **Archived means: this is old, so it is hidden by default. Nothing more.**

Everything on this page is a consequence of that sentence, including several
behaviours that look like bugs until you have read it.

The decision is recent, and it was made because the product had never actually
answered the question. There was one soft delete, stamping a timestamp on the
row. Two different words were used for it — "delete" on the surfaces, "archive"
in the rule that a team may be wound down once its board holds no live cards —
and nobody had said which one the product meant.

## What it replaced, and why that was worse than it sounded

A walk through the product looking for the answer found that archiving behaved
like destruction. Fetching an archived item by short code returned a 404. So
did its version history. The rows were still in the database and nothing served
them, which is a distinction visible only to somebody reading the schema: to
every caller the behaviour was indistinguishable from a hard delete.

The item's custom field values were unreachable in the same way — and yet still
counted by the guard that refuses to retire a field definition while anything
uses it. One archived card could therefore make a custom field permanently
unretirable, with no route to the card that held it. That is not a small bug;
it is a trap, and the only reason it existed was that nobody had decided what
the state meant.

What survived was the activity trail, which records *that* work existed and who
touched it, and never what it said.

So the old behaviour destroyed the audit answer while keeping the audit
question. The point of a system of record is that the record outlives the work,
and an organisation asks "what did that ticket say?" precisely about work that
is finished — which is exactly the work that gets archived. Version history
makes this sharpest: the whole reason Kairos keeps copy-forward history is to
reconstruct what a record said at a point in time, and it went dark at the
moment that mattered.

## The six things the sentence commits to

Read as rules rather than as prose, the decision says: archived content stays
retrievable, marked as archived, along with its history. It stays searchable
when asked for explicitly. Default listings — boards, queues, directories,
ordinary search — hide it, which is the entire user-visible purpose of the
state. It is not a [permission boundary](capabilities-and-access.md). It is not
live work. And it is still content.

Those six are numbered in
[the decision record](https://github.com/colliery-io/kairos/blob/main/.metis/adrs/KAIROS-A-0020.md),
which is where the reference pages' citations to "rule 2" and "rule 3" resolve;
this page argues them rather than restating them as a specification.

"Marked" carries more weight than it looks. Archived rows are marked, never
disguised: anything that serves one says so, through the `archived_at`
timestamp in the payload, a marker in the rendered text an agent sees, and a
banner in the interface. An auditor must never mistake archived work for live
work, which is a different failure from hiding it. The
[schema reference](../reference/rest/schemas.md) is where that marker and the
opt-in that asks for archived rows are specified; what matters here is that
neither is a privilege — they are questions anyone may ask.

The last two look contradictory and are not. They are the two halves of the
distinction this page exists to draw, and the clearest formulation of it is:

> **Containment is a fact about the record; progress is a fact about live work.**

An item's relationship list names its archived children, because what this
initiative contained is a historical fact and omitting rows would make the
record wrong. Its progress rollup does not count them, because progress is a
question about work someone is still doing. An archived item does not block
anything, because archived work cannot be waiting on anyone. Same rows, two
questions, two honest answers.

That is also why the guards that ask "is there still work here?" — winding down
a [team](teams-and-boards.md), retiring a
[repository](repositories-as-execution-scope.md), removing a board column — go
on counting live rows only. Putting a board's cards away is still how a team is
wound down, and that rule is unchanged by any of this.

The counterweight is the sixth rule. A guard that asks a different question —
"does anything still refer to this definition?" — may legitimately count an
archived carrier, because the reference is a fact about the record. But it may
only do so if the refusal *names* the carriers, so the user can go and reach
them. Retrievability is what makes counting archived rows honest; without it,
the same guard is the trap described above. The custom-field bug was fixed by
making archived work reachable, not by changing the guard.

## Read-only, and why the freeze needed no new guard

Archived work is read-only, plus one verb that puts it back.

The way that fell out is a nice accident of how the code was already shaped.
Every mutating path resolved its target through a live-only lookup, and every
read path had to be widened to see archived rows. So the freeze is the absence
of a change: widening the reads while leaving the writes alone *is* the
read-only rule, with no second guard to keep in sync. The reason this is
written down in the design rather than left to be discovered is that the
asymmetry looks like an inconsistency, and a later reader tidying it into
uniformity would silently unfreeze the archive.

Restore is the inverse of archiving rather than a new privilege, so it needs
the same capability that archiving needed. Two things about it surprise people,
and both are decisions rather than omissions.

### Restore does not un-cascade

Archiving a parent cascades to its children. Restoring that parent does not.

The reason is that a cascade was one act on a subtree, and undoing it is not
the same act in reverse. Some of those descendants were probably archived
earlier, on their own merits, by somebody who meant it — and after the cascade
there is no way to distinguish them from the ones the cascade swept up.
Un-cascading would therefore resurrect work nobody asked back, and would do so
invisibly.

So restore restores the item it was asked about, and reports the archived
descendants it did not touch, so that each one can be a separate decision. That
is stated in the [restore endpoint's
reference](../reference/rest/across-any-work-item.md) rather than being
something a caller has to discover.

### Restore refuses rather than re-homing

If an item's board, column, owning team or repository has been removed since it
was put away, restore fails and names what is missing.

The tempting alternative is to put the item somewhere sensible — the team's
other board, a default column — and the objection is that doing so would invent
a fact about the record. An archived card says which column it was in when it
was put away, and that is precisely the audit fact worth keeping; silently
re-homing it would overwrite the answer with a guess, and would also be a
mutation of frozen material. Refusing and naming the obstacle leaves the choice
with the person who has the context, exactly as the team wind-down guard does.
Moving the item somewhere that still exists is then an ordinary operation rather
than a special case in the restore path.

This is also why a board column that still holds archived cards is soft-deleted
rather than destroyed. Removing a column had to become possible even when only
archived cards remained — otherwise a single old card pinned a column forever —
but destroying the column would have taken the card's column name with it. Put
away one level down, the column keeps satisfying the archived card's reference,
so an archived card still renders with the real column it was put away in, and
live boards never show the column again. The same decision applied recursively,
which is why it needed no new concept.

## The vocabulary, which is genuinely confusing

Kairos uses the word "archived" for two unrelated things, and knowing which is
which saves real time.

**The document editorial lifecycle** — draft, review, published, archived — is
a status on a document, describing where it is in its own authoring cycle. A
published document can be editorially archived and remain perfectly live: it is
listed, editable, and reachable like anything else. This sense of the word
predates the decision on this page and is what the GUI's document status shows.

**Putting work away** is the state this page is about: the timestamp that hides
an item from default listings. It applies to every family of item, it is not a
status on a document, and it has nothing to do with the editorial lifecycle.

Because the two would have collided in the interface, the GUI calls the second
one **"put away"** — an item shows a "put away" marker, says when it was put
away and by whom, and says it is read-only while it is. That is copy chosen to
avoid the collision, not a third concept.

There is one more piece of unfortunate vocabulary, recorded here because it is
load bearing: on the wire and in the CLI, the verb is still "delete". It was an
accurate name when deleting meant disappearing, and it is now actively
misleading. Renaming it is a larger change than it looks — wire compatibility,
GUI copy, command nouns — and it is deliberately not part of this decision. The
glossary is the place to check which sense of a word a given surface means.

## Alternatives, and what they would have cost

Four alternatives were considered for what the state should mean.

*Serve archived content to administrators only* was rejected by the decision
maker on the grounds that it makes archiving a permission boundary, which is
more than a visibility default — and it breaks audit for precisely the people
who ask the question most, the engineers and team leads who were there.

*A separate archived state alongside the soft delete* was rejected as two
concepts where one will do, plus a migration and a second guard to keep in sync
with the first. Framing the state as a visibility default is what keeps it
cheap: no new state machine, no new capability, and the existing timestamp
already carries it. What changed was who is allowed to ask past it, and the
answer is anyone who could see it before, if they say so.

*Document the limitation and move on* was rejected because it leaves a system
of record that cannot answer a question about its own records.

*Hard delete on archive* was never seriously considered — it would make
copy-forward history and the retention sweeper meaningless — but it is recorded
anyway, because the old 404 behaviour was indistinguishable from it to every
caller. That is the sort of thing worth writing down: a behaviour nobody chose
can still be the behaviour you ship.

One smaller alternative is worth recording because it was the obvious
implementation and could not work. Adding an "include archived" parameter to
the existing handlers, leaving the underlying views alone, was much less work —
and the parameter would have had nothing to widen, because the views that hid
archived rows sat *upstream* of the lookups the parameter would have modified.
The existing search flag was in exactly that position, which is why it was a
silent no-op whenever a text query was also supplied. Fixing the views was the
whole job.

## What this design costs, and what it leaves open

Every list query now has two modes and the default is the one that has to stay
fast, so wherever the opt-in is plumbed the archived branch needs its own
thought about indexes.

Reachability also has to be total. An auditor who finds the answer on one
surface will reasonably assume the others agree, so a partial implementation
would have been worse than none — which is why the decision was implemented
across the API, MCP, the CLI and the GUI together rather than incrementally.

And the open question is erasure, not visibility. Nothing in Kairos currently
destroys archived content: the retention sweeper is not wired into the server
at all, and even when it is, it purges history and activity rows rather than
the archived records themselves. So archived content is permanent, and this
decision makes that permanent *by design* rather than by accident. A tenant
with a regulatory erasure requirement sits directly against the rule that
archiving is not a permission boundary, and would force a real deletion path
rather than a visibility flag. That is a review trigger on the decision, not a
promise about it.

## Where this comes from

- [KAIROS-A-0020](https://github.com/colliery-io/kairos/blob/main/.metis/adrs/KAIROS-A-0020.md)
  — the decision, its six rules, and the alternatives.
- [KAIROS-I-0015](https://github.com/colliery-io/kairos/blob/main/.metis/initiatives/KAIROS-I-0015/initiative.md)
  — implementing it across every surface, including the column soft delete and
  what the code knew that the survey did not.
- [KAIROS-I-0012](https://github.com/colliery-io/kairos/blob/main/.metis/initiatives/KAIROS-I-0012/initiative.md)
  — the live-only guard that archiving a board clear is still how a team is
  wound down.

<!-- KAIROS-I-0016 / KAIROS-T-0171 (E6): the archived marker, the search opt-in,
     the restore refusal code and the editorial statuses are reference facts,
     named here and specified there. The glossary and the MCP tool reference are
     not in the spine yet, so they are named in prose without a link;
     KAIROS-T-0175 adds those links in the cross-link pass. -->
