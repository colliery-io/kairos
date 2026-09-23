# Capabilities and access: why it is not roles

The first thing to know about authorisation in Kairos is what it is not. There
is no "team lead" role, no "coordinator" role, no "editor" that a user is made
into. There is nothing in the system that answers "what is this person?" — only
"what may this person do, on which board?"

That is not a stylistic preference. It is the decision that most of the rest of
the access model falls out of, and it is the one readers most often mistake for
an oversight.

## Read is open; write is a whitelist

Any member of an organisation can read any board, item or document in it.
Nothing needs granting for that.

This is a position, and it is worth stating as one: it follows from Flight
Levels rather than from a security analysis. The whole purpose of making work
visible on connected boards is coordination across team boundaries — a team
that cannot see what the initiative board is holding cannot tell whether its
own work is on the critical path. Read restrictions would undermine the thing
the product is for.

The consequence to accept knowingly is that **a board is not a confidentiality
boundary.** The tenant is. Each organisation gets its own PostgreSQL schema and
queries never cross it, so the hard boundary is real and it sits one level up
from where people often look for it. Work that genuinely must not be seen by
colleagues does not belong on a Kairos board.

Writes are the opposite: nothing is permitted unless something explicitly
permits it. A blacklist would have been less setup — users can do everything
until restricted — and was rejected for the usual reason, which is that "forgot
to deny" is a failure mode and "forgot to grant" is a support request. In a
system where strategic decisions move through board transitions, a user who
cannot transition a strategy will ask; a user who accidentally transitions one
will not.

## Capabilities are verbs on boards

A grant is a triple: this user, on this board, may do this. The capability
vocabulary is small and fixed — managing each family of items, transitioning
items between columns, configuring a board or templates or metadata, managing a
board's own membership — and it is
[enumerated in the decision](https://github.com/colliery-io/kairos/blob/main/.metis/adrs/KAIROS-A-0006.md)
that defines it rather than restated here. Glob patterns exist as a
convenience, so that granting everything on a board, or all the management
verbs, is one row rather than several. What matters for understanding the model
is that the patterns are the whole of the matching story: nothing else
intervenes between a grant and a decision, so there is no evaluation order to
hold in your head.

**Why the management capabilities are per board rather than global** is the
design decision a reader will otherwise read as an oversight, so it is worth
being explicit. A global "may manage initiatives" capability cannot be
restricted: a coordinator responsible for one initiative board would
automatically gain write access to every other one, and there would be no way
to express "admin here, read-only there" short of adding a second, negative
layer. Board-scoped grants cost more rows and a slightly more involved check;
what they buy is that the granularity matches how the organisation actually
works, because each board genuinely has its own owner and participants.

The alternative that looks most attractive from outside is a role per flight
level — leadership on strategy boards, coordinators on initiative boards, team
leads on delivery boards. It maps onto the default Flight Levels ownership
model exactly, and that is its problem: it hardcodes the assumption that every
strategy board in every tenant has the same access model. Kairos keeps those
named bundles as a convenience layer on top — a default set of grants that
makes a new coordinator's board usable — rather than as part of the
authorisation model. The distinction matters when an organisation needs
something the defaults do not describe: it customises grants, it does not have
to fight a role.

The practical payoff of having no roles is that "what can this person do?" is
answered by listing their grants. There is no inheritance to trace, no global
layer to check afterwards, and an audit is a query.

## The three things that are implied rather than granted

A pure whitelist would be unusable, and three implications keep it honest. Each
is *computed* at check time rather than stored as rows, which is the shared
design idea worth pulling out: nothing needs syncing, and nothing can drift.

**Organisation admins bypass everything.** This is the only role-shaped thing
in the system, and it is acknowledged as an escape hatch — the kind that gets
overused because it always works. It exists because tenant-wide configuration
has to belong to somebody.

**Membership of a board's owning team implies the day-to-day delivery set.**
The implied verbs are the ones a team member uses to do their own work, and the
boundary is drawn deliberately short of anything that changes how the board
itself behaves or who else may use it — so configuration and membership stay
explicit grants, as do the management families belonging to the levels above
delivery. The decision record has the exact set; the shape of it is the part
worth knowing, because it explains why this implication is safe to hand out
automatically and a broader one would not be.

The amendment came directly out of user-acceptance testing, which found that a
team member could not work their own team's delivery board until an admin
hand-granted capabilities per member per board. "Join the team, work the team's
board" is what everyone expects, and the expectation was right.

Two other ways to deliver it were rejected. Auto-granting real rows when
someone joins a team is the obvious implementation and creates a revocation
problem: once an admin has customised the grants, nobody can tell which rows
came from membership and which were deliberate. Keeping the pure whitelist and
seeding defaults leaves the onboarding chore in place. Computing the
implication means leaving the team *is* the revocation, and explicit grants
keep their own audit story untouched.

**Every member may file into any delivery board's Backlog.** This one is a
deliberate widening of the whitelist rather than a convenience: it is the only
place where the model grants a write nobody asked for, and it is narrowly
bounded so that the owning team's triage remains the control point.
[Repositories as execution scope](repositories-as-execution-scope.md) is where
that widening is argued and its bounds described, because it exists for the
sake of cross-repository coordination.

## Things that have no board of their own

Not everything sits on a board, and each case is resolved by asking what board
it *belongs* to rather than by inventing a new access surface.

**A document inherits its parent's board.** Documents are not free-floating
artifacts in Kairos; a document supports a workflow item, and that relationship
is how it is anchored. So editing a document that supports an initiative
requires write access on that initiative's board. The alternative — giving
documents their own synthetic board context — would have created a second place
to grant access and a second place to get it wrong. The consequence worth
knowing is that a document's access changes if its parent moves, which is
correct and occasionally surprising.

**Tenant-wide configuration is organisation-admin only.** Templates, metadata
definitions and the relationship vocabulary are not scoped to a board, so there
is no board-scoped answer to give. This is the case the org-admin bypass exists
for.

## Archived work is not less accessible

One rule holds across the whole model and is easy to get backwards: **whoever
could read work before it was put away can read it after.** Archiving is not a
permission boundary.

Implementing that turned out to require care rather than a new rule. Board
resolution used to consider only live items, so an archived item resolved to no
board at all — and with no board there were no board-scoped capabilities to
find, leaving only the tenant-wide admin policy. Serving archived work without
fixing that would have made archived work *more* restricted than live work,
which inverts the rule precisely. It is a good illustration of how a whitelist
fails: absence of a grant and absence of a subject look identical at the point
of the check.

[Archiving](archiving.md) covers the rest of what that state does and does not
mean.

## Where this comes from

- [Board-scoped capabilities, the whitelist stance, the capability vocabulary and
  the amendment that added team-implied
  capabilities](https://github.com/colliery-io/kairos/blob/main/.metis/adrs/KAIROS-A-0006.md)
  (KAIROS-A-0006).
- [Backlog filing as a computed, tenant-wide
  capability](https://github.com/colliery-io/kairos/blob/main/.metis/adrs/KAIROS-A-0019.md)
  (KAIROS-A-0019).
- [Archiving is not a permission
  boundary](https://github.com/colliery-io/kairos/blob/main/.metis/adrs/KAIROS-A-0020.md)
  (KAIROS-A-0020).

<!-- KAIROS-I-0016 / KAIROS-T-0171 (E6): the capability vocabulary and the
     computed grant sets are cited to KAIROS-A-0006 rather than restated here,
     because no reference page currently enumerates them. Recorded for
     KAIROS-T-0169/T-0175: a reference home for the capability surface is the
     right destination, and this page should cite that instead. -->
