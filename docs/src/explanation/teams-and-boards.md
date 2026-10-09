# Teams and boards: ownership, and why a team is hard to delete

A team in Kairos is not a group of users with a permission set attached. It is
an owner: of a delivery board, of some number of repositories, and — through
both of those — of a slice of the work in flight. That ownership is what makes
"whose work is this?" answerable, and it is also why disbanding a team is a
deliberate, obstructive operation rather than a delete button.

## Team types describe; they do not do anything

Every team carries a type drawn from Team Topologies — Matthew Skelton and
Manuel Pais's vocabulary of stream-aligned, platform, enabling and
complicated-subsystem teams. Kairos adopts the words deliberately, and then
does nothing mechanical with them: the type appears in the directory, on the
team page, in what an agent is told about a team. Nothing in authorisation,
task placement or board behaviour branches on it.

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
implies the day-to-day capabilities on that board, and it is how a task gets
its team. A task takes the team of the board it is on, so naming a team when
creating a task is the same as naming that team's delivery board, and there is
exactly one.

A team also owns repositories, and that ownership is of a different kind. It
makes the team answerable for the codebase: its review, its release and its
standards. It does not bring tasks to the team's board. A task on any team's
board may link to any repository, which is argued in [repositories as
execution scope](repositories-as-execution-scope.md).

The upper levels work differently. Strategy, initiative and ADR boards are
boards of the organisation, and no delivery team owns them. The asymmetry is
intentional: at the delivery level the owner genuinely is a team, and the
schema can say so; at the coordination and strategy levels the owner is a role
— a coordinator, leadership — and roles in Kairos are expressed as capability
grants rather than as entities. Putting a `team_id` on a strategy board would
invent an organisational structure that most tenants do not have.

ADR boards are the exception at the upper level. A decision about how one team
builds its own product is delivery work, and its owner is that team. An ADR
board can therefore have a team: the ADR board of the team, which holds the
delivery ADRs of the team and takes the prefix of the team, so that the
decisions of Skadi read `SKADI-A-0001` next to its tasks. A team has at most
one. An ADR board with no team stays a board of the organisation, for the
decisions that cross teams.

A member of the team writes ADRs on the ADR board of the team with no grant.
The team owns these decisions, so the team rule gives `manage_adrs` on that
board, in addition to the delivery set. It does not give `manage_adrs` on the
ADR board of the organisation: a decision that crosses teams still needs a
grant.

## The teams of an initiative or a strategy

An initiative or a strategy is on a board of the organisation, so it has no
owner team. It still has teams: the teams that do its work. Kairos reads them
from the work, and a person can add a team before the work exists.

An initiative gets the team of the board of each live task below it. A
strategy gets the teams of the live initiatives below it, so it reaches the
tasks two levels down. When a task moves to the board of a different team, the
teams of its initiative change with it. Archived work, archived boards and
archived teams give no team.

Before an initiative is divided into tasks, it has no task to read. A person
can set a team on it by hand, with the `impacts` link from the item to the
team. The team set by hand stays when tasks come; the two sources are shown
apart, so a reader can tell a plan from a fact. Clearing a team set by hand
does not remove a team that the item gets from its tasks.

A team of an item gives no right on the item. The cards of the boards of
initiatives and of strategies show the teams as pills, and the boards filter by
team, with a filter for the items that have no team.

## Every board has a team

A board cannot have no team. The rule has two forms, one for each kind of
board.

A delivery board has a delivery team. The team is a record in the team
directory, and the board holds it in `team_id`. Kairos refuses to create a
delivery board with no team, and the refusal names the missing team. A live
delivery board cannot lose its team: no operation clears the team of a board
or moves a board to a different team. When a team is deleted, its delivery
board and its ADR board are put away with it.

A board of the organisation has no record in the team directory and no
`team_id`. Its team is the list of the members of the board. Admission is
about being added to the board: a person joins the team of a strategy board
when an administrator adds the person to that board, and leaves it when the
administrator removes the person. The list is on the configuration page of the
board, and `GET /api/boards/{id}/members` returns it to every member of the
tenant.

The reason for the first form is what a delivery board decides. A task takes
the team of its board, so a task on a board with no team has no team. It is in
no team's work, no team's rollup and no search by team. Any member of the
tenant can also send a request to any delivery board. A request to a board
with no team is a request to nobody.

The reason for the second form is the asymmetry above. The people who decide
strategy are a group, and a reader must be able to see who they are. But they
are a group because they were admitted to the board, not because the
organisation chart has a team of that name.

The rule applies when a board is created. Kairos does not change boards that
exist. A delivery board that was created with no team before the rule stays as
it is, and the board list shows it under **Needs a team**.

Teams also own their own pages and their charter, which informs the delivery
board without living on it, in the same way the company vision informs the
strategy board.

## Why a team cannot be deleted while its board still has work

Kairos refuses to delete a team while anything still points at it, and the
refusal names what. First repositories: a team that still owns codebases cannot
be disbanded, because retiring the team would leave those repositories with no
owner, and so with nobody answerable for their review, release and standards.
Then live cards: a team whose
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
mechanisms are deliberately different. A move places work. A request proposes
it: it arrives in the entry column of the receiving team's board as support
work, and that team decides what happens next. Teams request work of each
other; no team pushes work to a different team. [Repositories as execution
scope](repositories-as-execution-scope.md#requests-between-teams-are-support-work)
is where the request is argued and its bounds described.

A move does not look at the repository of the task. The task keeps its link
and takes the team of the board it arrives on, because the link says where the
code is and nothing about whose work it is. For the same reason a change of a
repository's owner changes nothing about the tasks that link to it.

### A board also owns documents

A board holds cards, and since COLLIERY-T-0269 it can also own documents. The
two are different relations. A card sits in a column of the board. A document
that names the board sits nowhere: it has no column and the board view does
not show it. The board gives the right to edit the document, and that is all.

Any live board can own a document, whatever its level. The vision of a product
usually names the delivery board of the team that builds the product, because
the members of that team can then edit it with no grant. A board of the
organisation can own a document too, and the people who edit it are the ones
with `manage_documents` on that board.

A document changes owner the way a task changes board: with the capability on
both boards, the one that owns it now and the new one. The person who wrote
the document can still edit it, but cannot hand it to another team alone, for
the reason a task cannot be pushed onto a board.

Owning documents is one more reason a board can refuse to go. A board that
owns a live document is not deleted, and a team whose delivery board owns one
is not deleted either. The documents name a different board first, or are put
away. [Repositories as execution
scope](repositories-as-execution-scope.md#a-document-is-about-a-repository-a-board-owns-it)
explains why the owner is a board and not the repository that the document is
about.

The endpoints, their refusal codes and the exact capability names are in the
[board and team reference](../reference/rest/boards-and-teams.md) and the
[CLI reference](../reference/cli.md); this page is about why the rules are
shaped the way they are.

## Who works on a task: the claim

A task in Active has a **claim**: the person who works on it. The claim answers
the question a team asks of every card in Active, "who has this?", and it
answers it without anyone having to set a field.

The claim starts by itself. The person who moves a task to Active gets it. When
an agent moves the task with the [agent key](../how-to/give-an-agent-your-key.md)
of a person, the claim names the person and marks the agent: "Alice (agent)".
A person is always responsible for the work, also when an agent does it. That is
why a service account takes no claim. A service account does machine work with
no person, so a move to Active by a service account leaves the task with no
claim.

The claim ends when the work does: the task leaves Active, to Completed, to
Blocked, or back to Todo. It also ends at a **hand-off** to a named person, and
at a **release**, which leaves the task in Active and free for anyone. The next
person who moves a free task into Active gets the claim. An archive of the task
ends its claim too.

A claim does not lock the task. A different person can still change it, and the
change is recorded as usual. The response of the change carries a warning that
names the person who has the claim, so that nobody overwrites the work of a
colleague or of an agent without knowing. A lock would stop the work of a team
when the person who has the claim is away; a warning tells, and the team
decides.

Who may hand off or release a claim follows from who may move the task. The
person who has the claim may give it to someone else or release it. Anyone with
`transition_items` on the board may do the same, because that person may move
the task out of Active anyway, and that ends the claim. A hand-off goes only to
a person of the organization.

Which column holds claims is a flag of the column, `claims`, next to the flag
`is_done`. A new delivery board has it on its Active column, and an admin with
`configure_boards` can move it. When the flag goes off, the claims of the tasks
in that column end.

The endpoints and the tools are in the [task reference](../reference/rest/work-items.md),
the [MCP tools reference](../reference/mcp-tools.md) and the
[CLI reference](../reference/cli.md).

## Where this comes from

- [The team lifecycle: the live-only guard, and moving tasks between delivery
  boards](https://github.com/colliery-io/kairos/blob/main/.metis/initiatives/KAIROS-I-0012/initiative.md)
  (KAIROS-I-0012).
- [Teams, team types and board ownership in the data
  model](https://github.com/colliery-io/kairos/blob/main/.metis/adrs/KAIROS-A-0001.md)
  (KAIROS-A-0001).
- A task in Active has a claim (KAIROS-T-0359, KAIROS-A-0024: a person is
  always responsible, and a service account takes no claim).
- A board always has a team, in two forms (COLLIERY-T-0230, a decision of the
  product owner on 2026-09-27).
- A board owns a document (COLLIERY-T-0269, a decision of the product owner
  on 2026-09-29).
- [One owning team per
  repository](https://github.com/colliery-io/kairos/blob/main/.metis/adrs/KAIROS-A-0019.md)
  (KAIROS-A-0019), amended by COLLIERY-A-0023: the team decides the board, and
  a repository is a link.

<!-- KAIROS-I-0016 / KAIROS-T-0171: how-to guides do not exist yet, so the
     operations here are described without pointing at a page; KAIROS-T-0175
     adds those links in the cross-link pass. -->
