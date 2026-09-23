# Repositories as execution scope: where tickets are issued and executed

Boards and delivery streams are where work is planned. A repository is where a
ticket is issued against and executed in. Kairos treats those as two different
kinds of thing on purpose, and the second one is a first-class, team-owned
entity rather than a label on a task.

That is a reversal of an earlier principle, which makes it worth explaining
rather than merely stating.

## The agent's frame is the checkout

Kairos's stated purpose is that agents can work inside a codebase and complete
work there, while knowing enough about other teams' codebases to open pull
requests against them and coordinate across boundaries. The unit an agent
actually executes in is a git repository. It is checked out in one; the tests
it runs, the conventions it follows and the gates it has to pass are that
repository's; its pull requests land there.

A team, though, routinely owns several repositories, and one delivery board
plans across all of them. So the planning unit and the execution unit are
genuinely different, and a ticket model that can only name the first one cannot
tell an agent what is its work and what is a neighbour's.

Before this decision, that was exactly the situation. A task carried a board
and a team and nothing else, so a team with three codebases had one board and
no way to say which ticket belonged to which. The only repository-shaped record
in the system existed so that webhooks could mirror pull requests onto items —
a repository record missing a name, which nothing else read. The plugin bound
an agent's session to a *board*, so an agent working in one repository saw
another repository's tickets as its own queue, and the server never learned
which checkout the agent was in. There was no directory either: an agent could
not ask who owns a codebase or where to file against it, so the only way to
coordinate across teams was to already know the other team's board name.

## One owning team, at most one repository per task

Two cardinality choices carry most of the design, and both were made to keep
every routing question closed-form.

**A repository has exactly one owning team.** Given a repository you know the
team; given the team you know its delivery board; given the board you know its
entry column. Nothing in that chain is a choice, which is what makes it
automatable. Genuinely shared codebases are modelled by granting the co-owning
team access on the owner's board through ordinary capability grants, not by
giving the repository two owners.

Many-to-many ownership was the obvious alternative and models shared codebases
directly. It was rejected because routing needs a primary owner anyway: every
"which board does this ticket go to?" question becomes a decision someone has
to make, and the administrative surface grows to match. Attaching repositories
to boards rather than to teams was also considered, and loses the ability to
answer "who do I talk to about this codebase?" — ownership becomes something
inferred through a board, which is backwards.

**A task binds to at most one repository.** Work that touches several codebases
is decomposed into one task per repository, joined by the parent and blocking
edges that already exist. There is no join table.

This one reads as a limitation and is closer to a discovery: an agent has to do
that decomposition anyway, because it will open a separate pull request in each
repository. Allowing many repositories per task would make an agent's queue a
join, make "done" ambiguous per repository, and blur the link between a pull
request and the item it belongs to.

Only tasks bind at all. Strategies, initiatives, documents and ADRs stay
repository-less, because that is where cross-repository intent lives. An
initiative that spans four codebases is not missing a field; being above the
execution scope is what it is for.

The binding is what routes a ticket: filing a task against a repository puts it
on the owning team's delivery board. The rule that a repository-bound task may
only sit on its owner's board is enforced in the service layer rather than as a
database constraint, which is a small decision with a useful consequence —
re-homing a repository between teams is one update, and its tasks are
re-checked on their next write instead of needing a migration.

## What it does to the agent loop

Once a repository is a real entity, an agent's scope can be the repository
instead of the board. Bootstrapping a checkout detects which repository it is
from the git remote and records it, so the session context becomes "this
repository's open work", with the team board available as the wider lens rather
than as the default. The repository record also carries a short "how to work
here" description — the thing an agent reads before starting — and its
in-flight pull requests, which is why looking a repository up is a useful
operation and not just a lookup.

The mechanics of that — the endpoints, the fields, the agent-facing tools — are
in the [execution scope reference](../reference/rest/execution-scope.md). What
matters here is the direction of the change: the server now learns which
codebase an agent is in, and can therefore answer questions per codebase.

## Cross-team filing, and why it is default-on

Any authenticated member of an organisation may file a task into any team's
Backlog against one of that team's repositories. No grant, no setup, no prior
arrangement.

This is a deliberate widening of the whitelist stance described in
[capabilities and access](capabilities-and-access.md), and the argument for it
is that the product does not work without it. The value proposition is an agent
in one repository opening the right ticket — and later the right pull request —
against another team's repository, without a human first arranging permissions
between two teams. If every pair of collaborating teams needs administrative
setup before an agent can coordinate, the day-one cross-team story is gone.

The widening is bounded to the minimum that achieves it: a task, into the
Backlog column, and nothing else. The filer cannot move it out of Backlog, edit
it, delete it, or change its fields. The owning team's triage is the control
point, which is a control they already had. Two narrower options were rejected
— requiring an explicit grant per collaborating pair, which restores the setup
cost, and allowing no cross-team creation at all, which reduces agents to
leaving breadcrumbs for a human to transcribe.

The risk is accepted rather than solved: a noisy tenant can fill another team's
Backlog, triage is the remedy, and rate limiting is not in scope. If that
becomes a real problem in practice, making the filing capability explicit or
per-team opt-out is the thing to revisit.

Filing also composes with what already existed rather than adding plumbing. A
filed task records who filed it, the filing skill adds a blocking edge back to
the originating item so the requesting team's board shows the dependency, and a
pull request opened later in that repository naming the ticket's short code
links itself to the ticket through the existing forge webhook. That reuse is a
theme of the whole decision: the webhook record was already a repository
missing a name, pull-request links already attached to items by short code,
blocking edges already expressed cross-team dependency, and the team-implied
capability already showed how to add a computed one. Nothing here is a new
mechanism, only a promoted one.

## The vision said the opposite

Kairos's vision originally read "delivery streams over repositories —
repositories are metadata on tasks, not organisational boundaries", and this
decision reverses it. Taking a position on your own stated principles deserves
an explanation rather than a quiet edit, so:

That principle was a reaction to the predecessor system, where work was scoped
to a single repository and the repository therefore *was* the organisational
boundary. Escaping that limit was right, and the limit is still gone — delivery
streams still span repositories, teams still own several, and boards still plan
across all of them. What the vision did was conflate a planning unit with an
execution unit, on the strength of having just escaped a system where they were
the same thing. The vision now reads "streams and boards plan the work;
repositories are where tickets are issued and executed", and the decision
record is the account of why it changed.

Worth noting that the earlier principle was never implemented either: even the
task-level repository metadata it described was not built. So the reversal
replaced an intention rather than a working design.

## Costs, and what remains open

A repository is one more axis on board views, search, the team page and the
agent-facing tools — worth it for teams with several codebases, and noise for
teams with one. Tasks with no repository stay valid, for non-code work and for
tenants not using repositories at all, so the axis is optional rather than
mandatory.

Retiring a repository is guarded the way other removals are: it is refused
while tasks or a webhook connection still reference it. Archived tasks do not
hold a repository down, for the reason [archiving](archiving.md) explains — a
wind-down guard asks about live work.

Two questions are explicitly left open. Tenants with genuinely co-owned
codebases, where "grant the second team on the owner's board" turns out to be
inadequate, would be the reason to revisit many-to-many ownership. And monorepo
tenants wanting sub-repository scope — a ticket issued against a path prefix
rather than a whole repository — would be the reason to revisit whether a
repository needs a path dimension at all.

## Where this comes from

- [Repositories as first-class execution scope: the cardinality choices, the
  cross-team filing rule, the vision amendment and the
  alternatives](https://github.com/colliery-io/kairos/blob/main/.metis/adrs/KAIROS-A-0019.md)
  (KAIROS-A-0019).
- [The whitelist stance that Backlog filing
  widens](https://github.com/colliery-io/kairos/blob/main/.metis/adrs/KAIROS-A-0006.md)
  (KAIROS-A-0006).
- [The amended principle, in its current
  form](https://github.com/colliery-io/kairos/blob/main/.metis/vision.md)
  (KAIROS-V-0001).

<!-- KAIROS-I-0016 / KAIROS-T-0171 (E6): the filing capability's exact bounds
     belong in reference. reference/mcp-tools.md names the computed capability
     and the Backlog-behind-triage rule but is not in the spine yet; recorded
     for KAIROS-T-0175. -->
