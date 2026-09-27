# Repositories as execution scope: where the code is, and who decides the board

Boards and delivery streams are where work is planned. A repository is where
the work is done. Kairos treats those as two different kinds of thing on
purpose, and the second one is a first-class, team-owned entity rather than a
label on a task.

The two meet on a task, and the rule for how they meet is short. The team
decides the board: whoever creates a task names a board, or names a team and
gets that team's delivery board. The board decides the team of the task. The
repository is an optional link that says where the code is, and setting it or
clearing it changes nothing else.

It was not always that way. The first version of this design let the
repository choose the board, and readers of the code will meet both decision
records, so this page explains the design as it stands and also how it got
there.

## The agent's frame is the checkout

Kairos's stated purpose is that agents can work inside a codebase and complete
work there, while knowing enough about other teams' codebases to open pull
requests against them and coordinate across boundaries. The unit an agent
actually executes in is a git repository. It is checked out in one; the tests
it runs, the conventions it follows and the gates it has to pass are that
repository's; its pull requests go there.

A team, though, routinely owns several repositories, and one delivery board
plans across all of them. So the planning unit and the execution unit are
genuinely different, and a ticket model that can only name the first one cannot
tell an agent which of its team's work belongs to the checkout it is standing
in.

Before repositories were entities, that was exactly the situation. A task
carried a board and a team and nothing else, so a team with three codebases had
one board and no way to say which ticket belonged to which. The only
repository-shaped record in the system existed so that webhooks could mirror
pull requests onto items: a repository record missing a name, which nothing
else read. The plugin tied an agent's session to a *board*, so an agent working
in one repository saw another repository's tickets as its own queue, and the
server never learned which checkout the agent was in. There was no directory
either: an agent could not ask who owns a codebase or how that team wants work
done in it.

## One field did two jobs

The decision that introduced repositories (KAIROS-A-0019) gave the new field
two jobs. It said where the code is. It also chose the board: given a
repository you knew its owning team, given the team you knew its delivery
board, and a task that named a repository went there. A task with a repository
could sit only on the board of the repository's owner, and a move to any other
board was refused.

The attraction was that every question of placement had one answer that nobody
had to choose. The cost showed up as soon as teams worked in each other's
code, which is the ordinary case and not the exception. A web team that fixes
a bug in the platform team's service is doing web's work, planned by web,
counted against web's capacity. Under the first rule that task could not sit
on web's board, because its repository belonged to platform. The team had two
bad options: leave the repository off, and lose the agent's queue, or put the
task on a board whose team had not planned it. A change of owner was worse.
Re-homing a repository silently changed which board every linked task was
allowed to be on, and nobody found out until a move was refused.

The amendment (COLLIERY-A-0023, approved on 2026-09-27) separates the jobs. It
amends the earlier decision and does not replace it: repositories are still
entities, each still has exactly one owning team, and a task still links to at
most one. What changed is that the repository no longer has any say in where a
task goes.

## The team decides the board

A task is somebody's work before it is work in some codebase, so the question
a create has to answer is *whose*. The caller answers it by naming a board, or
by naming a team, which stands for that team's one delivery board. A
repository on its own is not an answer, and a create that names only a
repository is refused with a message that says what is missing.

Refusing is a choice. The server could have kept the old reading as a
fallback, so that a repository alone still meant "the owner's board". That
would have kept two rules alive under one field, and the caller who relied on
the fallback would be the one who had not noticed that the rule changed. A
refusal that names the fix is cheaper than a task that quietly turns up on the
wrong team's board.

The team of a task is read from its board and cannot be sent separately. A
caller may still name a team beside a board, and naming the board's own team
is harmless; naming any other is refused rather than ignored, because the
caller evidently believes the task will be that team's work and it will not
be. The reason for deriving the team is that several things trust it: a team's
work documents, its rollups, the team filter in search. While callers could
send it, three writers disagreed about what it meant.

The link, for its part, is free. A task on any team's board may link to any
live repository, and a move between boards does not look at the link at all.
Work that has no codebase is ordinary: a task with no repository belongs to
its team and is complete as it stands. Support work is the main example.

The exact arguments, status codes and messages are in the
[work items reference](../reference/rest/work-items.md), the
[CLI reference](../reference/cli.md) and the
[MCP tools reference](../reference/mcp-tools.md).

## One owning team, at most one repository per task

Two cardinality choices survive from the first design unchanged.

**A repository has exactly one owning team.** Ownership used to be the middle
link of the routing chain, and that reason is gone. The better reason was
always the other one: somebody has to be answerable for a codebase, and "who
do I talk to about this repository?" should have one answer that the directory
can give.

Many-to-many ownership was the obvious alternative, and both decisions
considered it and kept one owner. The first rejected it because routing needed
a primary owner. The amendment took that reason away and kept the rule, and
the argument that remains is about responsibility: the duties described below
need one team that is answerable for them, and a repository with two owners is
one where each can assume the other reviewed the change. Several teams
*working* in a repository is the normal case and needs no second owner.
Attaching repositories to boards rather than to teams was also considered in
the first decision, and loses the ability to answer who owns a codebase except
by inference through a board, which is backwards.

**A task links to at most one repository.** Work that touches several
codebases is decomposed into one task per repository, joined by the parent and
blocking edges that already exist. There is no join table.

This one reads as a limitation and is closer to a discovery: an agent has to do
that decomposition anyway, because it will open a separate pull request in each
repository. Allowing many repositories per task would make an agent's queue a
join, make "done" ambiguous per repository, and blur the link between a pull
request and the item it belongs to.

Only tasks link at all. Strategies, initiatives, documents and ADRs stay
repository-less, because that is where cross-repository intent lives. An
initiative that spans four codebases is not missing a field; being above the
execution scope is what it is for.

## What ownership means

If owning a repository does not bring its tasks to your board, it is fair to
ask what it does bring. The answer is three duties, and they are the ones that
belong to the codebase rather than to any one piece of work in it:

- **The review of code.** Changes to the repository are reviewed by the team
  that owns it.
- **The release.** The owning team decides when and how what is merged goes
  out.
- **The standards of the repository.** How work is done there: its
  conventions, its gates, and the "how to work here" description that an agent
  reads before it starts.

The duties apply to every change, including a pull request submitted by a
different delivery team. That is the point of separating them from placement.
The web team plans and tracks its own task on its own board; the platform team
still reviews the pull request, because the pull request is in platform's
repository.

The review itself is not a ticket. The git provider already manages a pull
request: it assigns reviewers, records approval and blocks the merge. A team
does not create a task on the owner's board to ask for a review, because that
would copy a queue that exists and is better kept where the code is.

Ownership also does not give the owning team a list of the tasks that link to
its repository, and it sends no notification when a task links. Such a list
was considered and rejected. The case against it is that it would be a second
inbox beside the board, and it would invite the owner to manage work that a
different team planned. What the owner does see is what it is answerable for:
the repository shows its open branches and pull requests, whichever team's
task they belong to, and a count of open linked tasks across all boards.

The repository record still names the delivery board of the owning team. It
is useful as the answer to "where do I send a request to the people who own
this?", and it should be read as the owner's board and not as the place tasks
go.

## Requests between teams are support work

Teams request work of each other. No team pushes work to a different team.

Any member of an organisation may create a task on any team's delivery board,
with no grant, no setup and no prior arrangement. This is a deliberate
widening of the whitelist stance described in
[capabilities and access](capabilities-and-access.md), and the argument for it
is that the product does not work without it. The value proposition is an agent
in one repository asking the right team for what it needs without a human
first arranging permissions between two teams. If every pair of collaborating
teams needs administrative setup before an agent can coordinate, the day-one
cross-team story is gone.

What a person who does not manage the board creates is a *request*, and the
widening is bounded to exactly that. The request goes to the board's entry
column. It is counted as support work, whatever kind of task it is. The sender
cannot ask for the planned lane and cannot name a later column; both are
refused, not quietly corrected. Afterwards the sender cannot move the request:
not to another column, not to the planned lane, not to another board. What the
sender keeps is what any author keeps. They may edit the request, withdraw it
by archiving it, and link it, so that the sender's own board shows what it is
waiting on. [Capabilities and
access](capabilities-and-access.md#the-person-who-made-it-may-edit-it) argues
where that line sits and why.

The lane is the part that needed an argument. A team's planned lane is its
plan: the work it chose, in the order it chose. Work that arrives from outside
was by definition not in the plan when it arrived, and a board that files it
under "planned" misreports how much of the team's capacity was its own to
spend. Counting requests as support makes the incoming work of a team visible
as a quantity. Nothing is lost by it, because the receiving team can take a
request into its plan: a member of that team changes the work class, and from
then on it is planned work that the team planned.

So the incoming work of a team is its support lane, and that is the whole
intake mechanism. There is no separate inbox and no notification, for the same
reason the owner of a repository gets no list: a second place to look is a
second place to forget.

A repository is optional on a request and is not part of the condition. Under
the first design it was required, because the repository was how the server
knew whose board the task was for. That left no way to ask a team for
something that has no codebase yet, and once the repository stopped choosing
the board the condition had stopped meaning anything.

The computed capability behind this keeps its first name, `file_backlog`,
although "backlog" is now only the usual name of an entry column. Because the
name is kept, what `whoami` reports and what clients match on did not change.
Its exact bounds are in the
[capabilities reference](../reference/capabilities.md).

Two narrower options were rejected: requiring an explicit grant per
collaborating pair, which restores the setup cost, and removing cross-team
creation altogether, which reduces agents to leaving notes for a human to
transcribe. One more option was rejected: a default intake team per
repository, so that a request could name a repository and let the server find
the team. That is close to the first design under another name, and it has the
same weak point, a request with no repository.

The risk is accepted rather than solved. A noisy member can fill another
team's entry column, triage is the remedy, and rate limiting is not in scope.
If that becomes a real problem in practice, making the capability explicit or
letting a team opt out is the thing to revisit.

## An agent works the board of its own team

Once a repository is a real entity, an agent can know which one it is in.
Bootstrapping a checkout detects the repository from the git remote and
records it. The same step records a board, and the board comes from the team
of the agent, not from the owner of the repository. An agent on the web team,
checked out in platform's repository, works web's board.

Its queue is that board, filtered by the repository of the checkout: the work
its own team planned, in the codebase it is standing in. The board without the
filter is the wider lens. The repository record carries the short "how to work
here" description and the in-flight pull requests, which is why looking a
repository up is worth doing before starting.

The work of other teams in the same repository is not in the queue. That is a
decision and not an oversight. A queue across all boards was considered: every
open task that links to this repository, whoever planned it. It was rejected.
A queue is a list of things to do, and an agent has no standing to do another
team's work. Without a grant it could not write to those tasks in any case,
since its capabilities come from its own team. What an agent does need from the
neighbours is awareness, so that it does not collide with them, and that is a
search: the repository filter finds every task that links to the repository,
on any board, and the commits show the rest.

The mechanics are in the
[execution scope reference](../reference/rest/execution-scope.md) and the
[MCP tools reference](../reference/mcp-tools.md). What matters here is the
direction: the server learns which codebase an agent is in and can answer
questions per codebase, and it learns whose agent it is from the team, as it
does for a person.

## What was considered and rejected

The amendment records five alternatives that it rejected. Each is argued where
it arises above. They are gathered here because a reader arriving with one of
them in mind will want to find it. The second column is a summary of the
reason the record gives.

| Alternative | The case against it |
|---|---|
| Several teams own one repository | Ownership is the responsibility for review, release and standards, and one team must have it. A second team that works in the repository needs no ownership once the repository does not choose the board. |
| A list of linked tasks for the owning team | A team works only what it planned or what it accepted. The incoming work of a team is its support lane. |
| A queue across all boards for an agent | The task of a different team is the work of that team, in whatever repository the code is. Search gives the awareness. |
| No cross-team creation | Teams must be able to request work of each other inside Kairos. |
| A default intake team per repository | It was needed only with several owners, or while the repository chose the board. The person who sends a request always names a team. |

## The vision said the opposite

Kairos's vision originally read "delivery streams over repositories —
repositories are metadata on tasks, not organisational boundaries", and the
first decision reversed it. Taking a position on your own stated principles
deserves an explanation rather than a quiet edit, so:

That principle was a reaction to the predecessor system, where work was scoped
to a single repository and the repository therefore *was* the organisational
boundary. Escaping that limit was right, and the limit is still gone: delivery
streams still span repositories, teams still own several, and boards still plan
across all of them. What the vision did was conflate a planning unit with an
execution unit, on the strength of having just escaped a system where they were
the same thing.

One way to read the amendment is that it recovers part of the vision's first
instinct. A repository on a task is close to metadata again: a link, with no
power over where the task sits. What did not come back is the idea that a
repository is *only* metadata. It is an entity with an owner, a description
and duties attached, and that part of the first decision stands.

## Costs, and what remains open

A repository is one more axis on board views, search, the team page and the
agent-facing tools, which is worth it for teams with several codebases and
noise for teams with one. Tasks with no repository stay valid, so the axis is
optional rather than mandatory.

The closed-form answer is gone, and that is a real cost. A caller now has to
know whose work a task is before creating it. For a person that is rarely a
burden. For an agent it means the checkout has to be wired to a team, and an
agent that belongs to several teams has to be told which one works there.

Retiring a repository is guarded the way other removals are: it is refused
while live tasks or a webhook connection still reference it, and the tasks of
every team count. Archived tasks do not hold a repository down, for the reason
[archiving](archiving.md) explains: a wind-down guard asks about live work.

One gap is known. The web interface offers "new task" only on a board the
person manages, so it has no control for sending a request to a different
team yet. Requests work over the REST API, the CLI and MCP. The gap is
recorded as COLLIERY-T-0232.

Two questions are left open from the first decision. Tenants with genuinely
co-owned codebases, where one answerable team turns out to be inadequate,
would be the reason to revisit many-to-many ownership. And monorepo tenants
wanting sub-repository scope, where a task links to a path prefix and not to a
whole repository, would be the reason to revisit whether a repository needs a
path dimension at all.

## Where this comes from

- [Repositories as first-class execution scope: the cardinality choices, the
  first cross-team filing rule and the vision
  amendment](https://github.com/colliery-io/kairos/blob/main/.metis/adrs/KAIROS-A-0019.md)
  (KAIROS-A-0019).
- COLLIERY-A-0023, approved on 2026-09-27, which amends the record above: the
  team decides the board, and a repository is a link. It is kept in the Kairos
  deployment where this project tracks its own work, so there is no file to
  link to.
- [The whitelist stance that requests
  widen](https://github.com/colliery-io/kairos/blob/main/.metis/adrs/KAIROS-A-0006.md)
  (KAIROS-A-0006).
- [The vision](https://github.com/colliery-io/kairos/blob/main/.metis/vision.md)
  (KAIROS-V-0001).
