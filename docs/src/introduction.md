# Kairos

Kairos is Flight Levels work management: strategy, initiatives and delivery
on boards that connect to each other, with an agent-facing surface so machines
can do the work alongside people. It ships as one stateless binary serving the
GUI, the REST API, MCP and SCIM — against a PostgreSQL and an OIDC issuer you
bring.

## Where to go

This book is organised by what you are doing right now, not by feature. The
four sections answer four different questions:

- **[Tutorials](tutorials/run-kairos-locally.md)** — you have not used Kairos
  and want to see it work. Start here; the lesson is meant to be followed
  start to finish.
- **[How-to guides](how-to/install-with-helm.md)** — you have a goal and need
  the sequence. Grouped by whether you are running Kairos, doing work in it, or
  wiring an agent up to it.
- **[Reference](reference/cli.md)** — you need a fact and need to trust it.
  [Commands](reference/cli.md), [configuration](reference/configuration.md),
  [MCP tools](reference/mcp-tools.md), [endpoints](reference/rest-api.md),
  [capabilities](reference/capabilities.md), [errors](reference/errors.md),
  [vocabulary](reference/glossary.md).
- **[Explanation](explanation/flight-levels.md)** — you want to understand why
  Kairos is shaped this way. Read at leisure, not mid-task.

If you are not sure, the [glossary](reference/glossary.md) is a good first
stop. Kairos has a few words that mean more than one thing — "archived" most of
all — and knowing which is which saves time later.


## What this book does not cover

**Contributing to Kairos.** Documentation for people changing the code lives
beside the code it describes, which is where you will already be looking:

- [`README.md`](https://github.com/colliery-io/kairos/blob/main/README.md) —
  clone, build, test
- [`uat/README.md`](https://github.com/colliery-io/kairos/blob/main/uat/README.md)
  — the user-acceptance journeys
- [`e2e/README.md`](https://github.com/colliery-io/kairos/blob/main/e2e/README.md)
  — the end-to-end smoke
- [`docs/gui-conventions.md`](https://github.com/colliery-io/kairos/blob/main/docs/gui-conventions.md)
  — GUI conventions
- [`plugin/README.md`](https://github.com/colliery-io/kairos/blob/main/plugin/README.md)
  — the Claude Code plugin

That is a deliberate boundary rather than a gap: this book serves operators,
the people doing the work, and agent authors. "Contributor" is an audience
rather than a kind of documentation, and those files are more useful next to
the code than collected here.
