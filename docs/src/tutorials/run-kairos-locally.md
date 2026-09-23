# Run Kairos locally

In this tutorial we will start Kairos on your machine, sign in, and put a
piece of work on a board. By the end you will have a running deployment with
demo data in it and one task of your own, and you will have seen the same work
from the command line and in the browser.

It takes about fifteen minutes, most of which is a Rust build.

## Before we start

You will need:

- **Docker**, running. Kairos needs PostgreSQL and an identity provider, and
  we will run both in containers.
- **Rust**, via [rustup](https://rustup.rs). The repository pins its own
  toolchain, so you do not need a particular version installed.
- **[angreal](https://pypi.org/project/angreal/)**: `pip install angreal`. Most
  of the commands below go through it; two build and run the CLI with `cargo`
  directly.
- A clone of the repository, and a terminal in it.

We are running from a clone rather than from a release because this is the
lesson where you look at Kairos, and the clone gives us the demo data to look
at.

## Start the backing services

Kairos keeps nothing itself — it needs a database, and it needs somebody to
tell it who you are. Start both:

```sh
angreal services up
```

You should see two containers reported healthy:

```
 Container kairos-postgres  Healthy
 Container kairos-dex       Healthy
```

`kairos-postgres` is the database. `kairos-dex` is
[Dex](https://dexidp.io/), a small OIDC identity provider standing in for the
one a real deployment would bring. It already knows about four users, and we
will sign in as one of them.

## Create the database and put some data in it

```sh
angreal db migrate
```

The last line tells you it worked:

```
All migrations completed successfully
```

```sh
angreal db seed
```

The first created Kairos's tables. The second fills a tenant called `demo`
with a small organisation: two teams, a few boards, and a couple of dozen
items across them. The second prints an inventory of what it made — three
users, two teams, five boards and a couple of dozen items:

```
seeded tenant 'demo' (schema org_demo): 3 users (alice=org admin, bob, carol),
2 teams, 2 repositories, 5 boards, 1 delivery stream(s), 1 strategy,
4 initiatives (2 buckets), 11 tasks, 1 document, 2 ADRs, … 
```

If you have run it before it will say the tenant is already seeded and do
nothing; `angreal db seed --force` starts over.

## Build the interface and start the server

```sh
angreal web build
```

This compiles the browser interface to WebAssembly. It finishes with:

```
GUI bundle written to crates/kairos-web/dist
```

Now start the server:

```sh
angreal dev serve
```

It builds and then starts — the first build takes a few minutes, and
subsequent ones are seconds. When it is ready you will see:

```
Serving http://localhost:41080 (tenant `demo`, issuer http://localhost:41558/dex)
Sign in as alice@kairos.test / alice-password. Ctrl-C to stop.
```

Leave this running. Open a second terminal for everything below.

## Look at it in the browser

Open <http://localhost:41080>. You will be sent to Dex to sign in. Use:

- **Email**: `alice@kairos.test`
- **Password**: `alice-password`

Dex will hand you back to Kairos, and you will land on the boards overview.
You should see five boards: Strategy, Initiatives, Architecture Decisions, and
two delivery boards — Platform Delivery and Web Delivery. Open **Platform
Delivery** and you will see cards sitting in columns.

That shape is the point of Kairos, and the [flight
levels](../explanation/flight-levels.md) page explains why there are three
kinds of board rather than one. For now, notice that a delivery board belongs
to a team while the upper boards do not.

## Log in from the command line

Kairos is not only a website — the same deployment answers a CLI and an
agent-facing API. Build the CLI and log in:

```sh
cargo build -p kairos-cli
./target/debug/kairos login --url http://localhost:41080
```

The CLI prints a URL and a short code, and waits:

```
Discovered OIDC issuer: http://localhost:41558/dex

To sign in, open: http://localhost:41558/dex/device?user_code=SBCM-THZP
(or visit http://localhost:41558/dex/device and enter code: SBCM-THZP)

Waiting for approval (polling every 5s; the code expires in 600s)...
```

Open that URL and sign in as `alice@kairos.test` again. The CLI notices and
stores the token. This is the OAuth device flow — the same thing a smart TV
does — so the CLI never sees your password.

Check who Kairos thinks you are:

```sh
./target/debug/kairos whoami
```

```
user:   alice <alice@kairos.test>
org:    demo (role: admin)
teams:  Platform
```

Alice is an admin of the `demo` organisation and a member of the Platform
team. That team membership is what lets her work on Platform's board without
anybody granting her anything — see [capabilities and
access](../explanation/capabilities-and-access.md).

## Create your first piece of work

```sh
./target/debug/kairos tasks create \
  --repo payments-api \
  --title "Try Kairos out" \
  --content "My first piece of work."
```

```
Created task DEMO-T-0013 (version 1): Try Kairos out
```

Your short code may differ — mine was `DEMO-T-0013`. Use yours below.

Notice what we did not have to say: which board. We named a **repository**, and
Kairos routed the task to the board of the team that owns it. That is
[repositories as execution scope](../explanation/repositories-as-execution-scope.md),
and it is why an agent working in a repository does not need to know your
org chart.

Now look at the board it landed on. `boards show` takes the board's id, so
list the boards first:

```sh
./target/debug/kairos boards list
```

Copy the id on the **Platform Delivery** row, and use it:

```sh
./target/debug/kairos boards show <the id you copied>
```

```
Board: Platform Delivery (slug platform-delivery, level delivery, id aa525a31-…)

== Backlog (3 items, id 76e86e75-1590-415e-8f48-cc37fa222a2b)
   DEMO-T-0006  [task] Invoice webhook handler
   DEMO-T-0011  [task] Portal needs a bulk invoice export endpoint
   DEMO-T-0013  [task] Try Kairos out
```

Your task is in **Backlog**, the column new work arrives in.

## Move it

Each column's id is printed next to its name above. Copy the id of the
**Todo** column, and move your task there:

```sh
./target/debug/kairos tasks transition DEMO-T-0013 --to <the Todo column id>
```

```
Transitioned task DEMO-T-0013 to column 9b2a0dd5-c9ef-48e9-a947-d866dcf21618
```

Now go back to the browser and reload Platform Delivery. Your card has moved
to Todo. The CLI and the interface are two views of one deployment, not two
systems that synchronise.

The columns and the arrows between them are configuration rather than code, so
Kairos knows which moves this board allows and refuses the rest — naming the
columns you could have moved to instead. [Errors](../reference/errors.md)
describes that refusal, and every other one.

## What you have

A Kairos deployment running against a real database and a real identity
provider, with demo data, your own task on a board, and two ways of reaching
it.

When you are finished:

```sh
# Ctrl-C the server, then:
angreal services down
```

The database lives in a Docker volume, so `angreal services up` picks up where
you left off. `angreal services clean` removes the volume too.

## Where to go next

- [Install with Helm](../how-to/install-with-helm.md) — the same deployment for
  real, from the published chart. (A Kubernetes *tutorial* is coming; it is
  waiting on an ARM build of the image, so today the how-to is the honest
  route.)
- [Flight levels](../explanation/flight-levels.md) — why the boards are
  arranged the way they are
- [CLI reference](../reference/cli.md) — every command and flag
- The how-to guides, for a goal you have rather than a lesson we chose
