# Kairos

Kairos is Flight Levels work management: strategy, initiatives and delivery
on boards that connect to each other, with an agent-facing surface so machines
can do the work alongside people. It ships as one stateless binary serving the
GUI, the REST API, MCP and SCIM — against a PostgreSQL and an OIDC issuer you
bring.

## Where to go

This book is organised by what you are doing right now, not by feature. The
four sections answer four different questions:

- **Tutorials** — you have not used Kairos and want to see it work. Start
  here; the lessons are meant to be followed start to finish.
- **How-to guides** — you have a goal and need the sequence. Grouped by
  whether you are running Kairos, doing work in it, or wiring an agent up to
  it.
- **Reference** — you need a fact and need to trust it. Commands,
  configuration values, MCP tools, endpoints, vocabulary.
- **Explanation** — you want to understand why Kairos is shaped this way.
  Read at leisure, not mid-task.

If you are not sure, the glossary is a good first stop. Kairos has a few
words that mean more than one thing — "archived" most of all — and knowing
which is which saves time later.

<!-- KAIROS-I-0016: the four bullets above and the glossary mention link to
     their pages once those pages exist; KAIROS-T-0175 adds the links as part
     of the cross-link check. Deliberately unlinked for now rather than
     pointing at files mdBook has not been told to create. -->

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
