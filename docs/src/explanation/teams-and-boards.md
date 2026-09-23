# Teams and boards: ownership, and why a team is hard to delete

A team in Kairos is not a group of users with a permission set attached. It is
an owner: of a delivery board, of some number of repositories, and — through
both of those — of a slice of the work in flight. That ownership is what makes
the routing questions answerable, and it is also why disbanding a team is a
deliberate, obstructive operation rather than a delete button.

## Team types describe; they do not do anything

Every team carries a type drawn from Team Topologies — Matthew Skelton and
Manuel Pais's vocabulary of stream-aligned, platform, enabling and
complicated-subsystem teams. Kairos adopts the words deliberately, and then
does nothing mechanical with them: the type appears in the directory, on the
team page, in what an agent is told about a team. Nothing in authorisation,
routing or board behaviour branches on it.

That is a choice rather than an omission. The four types are a way of talking
about why a team exists and how other teams should expect to interact with it —
a platform team that gets treated like a stream-aligned team is a
misunderstanding worth being able to name. But encoding behaviour into the type
would mean Kairos had opinions about, say, what an enabling team is allowed to
do, and those opinions would be wrong for some organisation inside a month.
Access is granted per board (see [capabilities and
access](capabilities-and-access.md)); the type stays a label that humans and
agents read.

## What a team owns

A delivery board is created with its team and belongs to it. That single fact
carries a surprising amount of weight elsewhere: it is how team membership
implies the day-to-day capabilities on that board, and it is the last hop in
repository routing — a repository has one owning team, and the team has one
delivery board, so a ticket filed against a repository has exactly one place to
land.

The upper two levels work differently. Strategy and initiative boards are
organisation-level and not owned by a team at all. The asymmetry is
intentional: at the delivery level the owner genuinely is a team, and the
schema can say so; at the coordination and strategy levels the owner is a role
— a coordinator, leadership — and roles in Kairos are expressed as capability
grants rather than as entities. Putting a `team_id` on a strategy board would
invent an organisational structure that most tenants do not have.

Teams also own their own pages and their charter, which informs the delivery
board without living on it, in the same way the company vision informs the
strategy board.

## Why a team cannot be deleted while its board still has work

Kairos refuses to delete a team while anything still points at it, and the
refusal names what. First repositories: a team that still owns codebases cannot
be disbanded, because retiring the team would leave those repositories with no
owner and therefore no route for tickets. Then live cards: a team whose
delivery board still holds live work cannot be disbanded either, and the
refusal lists the work by short code.

The reasoning is that a team disbanding is an organisational fact, and the work
does not evaporate with it. Cascading would be easy to implement and would
quietly destroy somebody's queue — the two cards that mattered were probably
the reason another team was waiting. Refusing and naming the blockers turns a
destructive operation into a decision someone has to make about each piece of
work: it moves to another team, or it is put away. Either way a human chose.
The same shape shows up wherever Kairos guards a removal, and it is the reason
the refusals carry lists rather than counts.

### The history is instructive

This guard used to be much stricter, and wrong. Originally the check counted
*every* card that referenced the board, archived ones included, on the argument
that an archived card still holds the foreign key and would be orphaned if it
were ever restored. The effect was that a team became permanent the moment any
card had touched its board — the refusal told the user to "move or delete
them", and neither escaped it, because there was no way to change an item's
board after creation and deleting was a soft delete that still counted.

Two things unlocked it. Moving a task between delivery boards became a real
operation, so "move them" became advice a user could take. And the orphan
argument turned out to be theoretical: nothing restored items at the time, so
the guard was protecting an invariant for a feature that did not exist. Which
left the guard costing something real — a permanent team — to protect nothing,
and the fix was to count only what the question was actually about: cards
someone is still working on. Archived or moved, either satisfies it.

Restore exists now, which retroactively vindicates the worry rather than the
guard: a restore whose board is gone is refused and says so, instead of the
board being pinned forever on its behalf. [Archiving](archiving.md) covers why
that refusal is the right shape.

When the team does finally go, its delivery board is put away with it rather
than destroyed, so the record of a disbanded team's work stays readable. That
is archival in effect and it is why "archive the team" was never needed as a
separate concept.

## Moving work between boards

Work moves between delivery boards, and the interesting part of that design is
who is allowed to do it: capability on **both** boards, the source and the
target. Not because moving is dangerous, but because pushing work onto another
team's board is their decision as much as yours. Landing a card on a team's
board is an assertion that they will do it.

That leaves a gap, and the gap is filled from the other direction: a genuine
cross-team *request* needs no capability on the target board at all. The two
mechanisms are deliberately different — a move places work, a filing proposes
it — and [repositories as execution scope](repositories-as-execution-scope.md)
is where the second one is explained, because it exists for the sake of
repositories rather than of teams.

One constraint on moves follows from repository ownership rather than from
teams: a task bound to a repository can only sit on that repository's owning
team's board. Otherwise a ticket would claim to be executed in a codebase whose
team had never agreed to it. Unbinding the task or re-homing the repository are
both ways out, and both are decisions about ownership rather than about
placement — which is the point.

The endpoints, their refusal codes and the exact capability names are in the
[board and team reference](../reference/rest/boards-and-teams.md) and the
[CLI reference](../reference/cli.md); this page is about why the rules are
shaped the way they are.

## Where this comes from

- [The team lifecycle: the live-only guard, and moving tasks between delivery
  boards](https://github.com/colliery-io/kairos/blob/main/.metis/initiatives/KAIROS-I-0012/initiative.md)
  (KAIROS-I-0012).
- [Teams, team types and board ownership in the data
  model](https://github.com/colliery-io/kairos/blob/main/.metis/adrs/KAIROS-A-0001.md)
  (KAIROS-A-0001).
- [One owning team per repository, and the routing that depends on
  it](https://github.com/colliery-io/kairos/blob/main/.metis/adrs/KAIROS-A-0019.md)
  (KAIROS-A-0019).

<!-- KAIROS-I-0016 / KAIROS-T-0171: how-to guides do not exist yet, so the
     operations here are described without pointing at a page; KAIROS-T-0175
     adds those links in the cross-link pass. -->
