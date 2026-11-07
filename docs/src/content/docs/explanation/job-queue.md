---
title: "The job queue: how the servers share the background work"
---

A Kairos server does more than answer requests. It builds the code index of
each repository, embeds the items for semantic search, runs the retention
pass, sends the signed webhook deliveries and writes back to GitHub. This
page tells how 2 or more servers share that work, so that each piece of work
runs one time.

## Why a queue in the database

Before 0.11.0, each server ran its own background work. The queue of the
code-index builds was in the memory of the server, and so was the count of
failed logins. With 2 replicas, a build could run on both servers, and a
person could try twice as many passwords before the lockout. So the Helm
chart ran 1 replica (KAIROS-T-0380).

The servers already share one thing: the Postgres database. So the work is
in a table of that database, `public.jobs`, and each server takes work from
it (COLLIERY-I-0613). There is no other service to run. The count of failed
logins is in the database too (`public.auth_failures`), so a lockout holds
on each server.

## The table

One row is one piece of work: a job. The table is in the `public` schema,
not in the schema of an organization, so that one query of a server sees
the jobs of all the organizations. The column `tenant` names the
organization of a job. A job of the deployment, such as the retention pass,
has no tenant.

| Column | What it holds |
|---|---|
| `kind` | The type of work, for example `embedded_build` or `webhook_delivery`. |
| `tenant` | The organization of the job, or none. |
| `payload` | What the job works on, as JSON: a repository, a delivery, a pass. |
| `priority` | A higher number runs first. |
| `dedupe_key` | Two open jobs cannot have the same key. |
| `run_after` | The job does not start before this time. |
| `state` | `queued`, `leased`, `done` or `failed`. |
| `leased_by` | The server that runs the job, or ran it last. |
| `lease_until` | The time at which the lease runs out. |
| `heartbeat_at` | The last time that the server wrote the progress of the job. |
| `tries`, `max_tries` | The tries so far, and the most tries. |
| `last_error` | The reason of the last failed try. |
| `finished_at` | The time at which the job ended. |

A server removes a job 7 days after it ended.

## How a server takes a job

Each server runs one worker. The worker asks for the next job with one
`UPDATE` that selects the row `FOR UPDATE SKIP LOCKED`. Two servers that ask
at the same time get 2 different jobs: a row that one server locks, the
other server skips. The same `UPDATE` writes the lease: `state = leased`,
`leased_by` = the server, `lease_until` = now + the lease time, and one more
try.

The worker asks only for the kinds that have a free slot on its server (see
[Limits](#limits)). A server wakes its worker when it adds a job. Else the
worker looks every 2 seconds.

## Order

The next job is the job with the highest priority. Inside one priority, the
job with the oldest `run_after` goes first, then the oldest job.

| Priority | Work |
|---|---|
| 50 | A code-index build that a person asked for. |
| 40 | A code-index update after a push. |
| 30 | The first code-index build of a repository. |
| 20 | A batch of the embedding refresh. |
| 15 | A webhook delivery, and a GitHub write-back. |
| 10 | A periodic pass: retention, and the sweep of the code index. |

For an update, `run_after` is the time of the newest index of the
repository: the time at which it became stale. So the repository that has
been stale longest goes first (KAIROS-T-0334).

## Leases, heartbeats and the stall kill

A job has a lease (`KAIROS_JOB_LEASE_SECS`, 5 minutes). While the server
holds the lease, no other server takes the job.

The lease follows the progress of the job, not a clock. A job tells its
worker when it makes progress: a build after each summary and each batch of
vectors, an embedding batch after each item, the retention pass after each
organization. At each heartbeat (`KAIROS_JOB_HEARTBEAT_SECS`, 30 seconds),
the worker writes the heartbeat and moves the lease forward, but only when
the job made progress since the last write.

When a job makes no progress for the lease time, it has stalled. Its worker
stops it one heartbeat before the lease runs out, so that no other server
takes the job while it still runs. Then the job goes back to the queue, and
the try counts.

When a server stops or loses its connection, it writes no more heartbeats.
The lease runs out, and any server takes the job. A job that a server takes
again continues its work where it can:

- A code-index build continues its run. Each summary is in the summary pool
  of the repository in the database when the model makes it, so the new
  server makes only the summaries that are not there (KAIROS-T-0378). The
  servers do not have to share `KAIROS_CODE_INDEX_DIR`.
- An embedding batch reads the marked items again. An item whose vector is
  current gets no new vector.

## Tries

A job has `KAIROS_JOB_MAX_TRIES` tries (3). A failed try goes back to the
queue after a wait (30 seconds). After the last try, the job is `failed`,
and `last_error` keeps the reason. When the lease of the last try runs out,
the next worker that looks fails the job with the reason "The lease ran out
on the last try".

Some kinds have rules of their own:

- A webhook delivery has `KAIROS_WEBHOOK_MAX_TRIES` tries (12). The waits
  double from `KAIROS_WEBHOOK_RETRY_BASE_SECS` (30 seconds), to 12 hours at
  most.
- A first build of a repository gives its slot to an update or a requested
  build of the same kind that waits. It stops after its next summary and
  goes back to the queue. No try counts.
- A server that cannot run a job now gives it back to the queue, for
  example a periodic pass that it does not run. No try counts.

When a build job fails after its last try, its run ends as `failed`, with
the reason. When a webhook target fails 24 times in a row, Kairos turns it
off and tells its owners.

## Limits

Each server runs a limited number of jobs of each kind at the same time.
The limits are for each server: with 2 servers, 2 embedded builds can run.

| Kind | Work | Limit on one server |
|---|---|---|
| `embedded_build` | A code-index build with the embedded model. | `KAIROS_JOB_LIMIT_EMBEDDED_BUILD` (1) |
| `hosted_build` | A code-index build with a hosted provider. | `KAIROS_JOB_LIMIT_HOSTED_BUILD` (4) |
| `embedding_batch` | A batch of the embedding refresh. | `KAIROS_JOB_LIMIT_EMBEDDING_BATCH` (2) |
| `webhook_delivery` | One signed webhook delivery. | `KAIROS_JOB_LIMIT_WEBHOOK_DELIVERY` (4) |
| `github_writeback` | The comment and the status of one pull request. | `KAIROS_JOB_LIMIT_GITHUB_WRITEBACK` (2) |
| `issue_import` | The import of the open issues of one repository. | `KAIROS_JOB_LIMIT_ISSUE_IMPORT` (2) |
| `issue_writeback` | The comment and the state of one issue that became a task. | `KAIROS_JOB_LIMIT_ISSUE_WRITEBACK` (2) |
| `periodic` | A periodic pass. | `KAIROS_JOB_LIMIT_PERIODIC` (1) |

The limit of the embedded builds keeps the CPU of a server for one model at
a time. A limit of 0 keeps the kind off that server.

## The connections of the jobs

The worker and the jobs use a database pool of their own, with
`KAIROS_JOB_POOL_SIZE` connections (6). The handlers of the requests keep
their pool, with `KAIROS_POOL_SIZE` connections. So a burst of webhook
deliveries, builds and embedding batches fills the job pool, and no request
waits for a connection. When the limits of the kinds add up to more than
the job pool, a job waits for a connection, 5 seconds at most. A job that
gets none fails its try and runs again.

Each connection of both pools has a `statement_timeout` of 30 seconds, a
`lock_timeout` of 5 seconds and an `idle_in_transaction_session_timeout` of
60 seconds. A job whose statement passes one of them fails its try, with
the reason in `last_error`.

An embedding batch holds no connection while the model runs. It reads its
marked items on one connection and gives the connection back. Then it calls
the provider, and writes the vectors on a second connection. A remote
provider that takes seconds holds no connection of the pool.

## Which work runs as jobs

| Work | Kind | Who adds the job | One open job for each |
|---|---|---|---|
| A requested code-index build | `embedded_build` or `hosted_build` | The API, when a person asks | repository |
| A code-index update | `embedded_build` or `hosted_build` | The sweep, when the head of the default branch, a tag or an open pull request has no index | repository |
| A first code-index build | `embedded_build` or `hosted_build` | The sweep, for a repository with no index | repository |
| A batch of the embedding refresh | `embedding_batch` | Each server, for an organization with no row; then the row queues itself again after each run | organization (one row) |
| A webhook delivery | `webhook_delivery` | The change that fires it, in its transaction | delivery |
| A GitHub write-back | `github_writeback` | The change of a linked item, in its transaction | pull request (one queued job) |
| An import of the open issues | `issue_import` | The API, when an organization admin asks; the issue-sync pass, when the interval of the repository is due (KAIROS-T-0670) | repository |
| An issue write-back | `issue_writeback` | The change of the task of an issue, in its transaction; a new write token | issue (one queued job) |
| The retention pass | `periodic` | Each server at its start; then each run adds the next run | pass |
| The sweep of the code index | `periodic` | Each server at its start; then each run adds the next run | pass |
| The issue-sync pass | `periodic` | Each server at its start; the worker puts the same row back in the queue after each look, every 30 seconds | pass (one row) |

A write-back job (`github_writeback`, `issue_writeback`) has no
`dedupe_key`. A unique index on the key of a queued job holds one queued job
for each pull request or issue. When 2 changes add a job at the same time,
the second change waits for the first, and adds no job (KAIROS-T-0637). A
job that runs does not count: a change during the run adds a job, because
the job that runs can have read the items before the change. When the job
that runs goes back to the queue (a retry) and a job of its key waits, it
ends `done`. The job that waits does the write.

The issue-sync pass looks at the repositories whose import interval is due,
and adds an `issue_import` job for each one (KAIROS-T-0670). It keeps one
row, as the embedding refresh does, so the table of jobs does not grow with
each look. Two servers never add the job twice for one due run, and the key
of the import allows one open import for each repository.

A periodic pass adds its next run in the transaction that writes its end.
An embedding batch keeps one row for each organization (KAIROS-T-0636). When
a run ends well, the worker puts the same row back in the queue, with its
tries at 0. The row waits `KAIROS_EMBED_REFRESH_SECS`, or no time after a
full page.
The next run is due one interval after the run was due. So a pass has one
open run at a time, and it runs one time for each interval on all the
servers together.

The work of a request does not wait for a job. A move of a task writes its
delivery job in its own transaction, and the answer comes back at once.

## What stays on each server

- The clones of the repositories and the work databases of the builds, in
  `KAIROS_CODE_INDEX_DIR`. They are a cache: the next build makes them
  again.
- The tools of the builder (`tools/` in `KAIROS_CODE_INDEX_DIR`).
- The list of commits whose build failed on this server. A restart clears
  it.
- The event sockets (`/ws/events`) of the browsers and the agents that
  connect to this server.

## Server roles

`KAIROS_ROLE` (see the configuration reference) sets the kinds of job that a
server takes:

| Role | Kinds of job | Periodic passes |
|---|---|---|
| `all` | every kind | the retention pass and the sweep |
| `api` | every kind except `embedded_build` and `hosted_build` | the retention pass |
| `worker` | `embedded_build` and `hosted_build` | the sweep |

The embedding batches, the webhook deliveries, the GitHub write-backs and
the cleanup of the sites stay on the servers that serve the API. They are
not build work, and they use little memory. A server takes only the runs
of the passes that its role runs: the run of another pass waits for a
server of the other role.

A build job of a worker that stops (for example, the kernel stops it because
it ran out of memory) holds its lease until the lease runs out. The next
worker that looks then fails the job, with the reason, and the breaker of
the repository trips. A server with the role `api` does not fail a build
job: it does not take that kind.

Each server of one database takes the jobs of the kinds of its role, so give
each server the same settings. A server with a different setting does the job in its
own way. For example, a server with no `KAIROS_WEBHOOK_ALLOW_PRIVATE`
refuses a delivery to a private address that another server sends.

## What to watch

The table is the record of the background work. Kairos shows it in three
places: the metrics, an endpoint, and the Admin overview.

**The metrics.** `GET /metrics` gives these families:

| Metric | Type | Labels | What it counts |
|---|---|---|---|
| `kairos_jobs` | gauge | `kind`, `state` | The jobs in the table now, of all the organizations. |
| `kairos_job_failures_total` | counter | `kind` | The jobs that this server wrote as `failed`. |
| `kairos_job_lease_kills_total` | counter | `kind` | The jobs that the watchdog of this server stopped for no progress. |

The `state` label is `queued`, `leased`, `done` or `failed`. One grouped
query reads the gauges. A server keeps the result for 15 seconds, so a
frequent scrape does not load the database. The counters are for each server.
They start at 0 when the server starts. Add the counters of all the servers.

Alert on these conditions:

- `kairos_jobs{state="queued"}` grows for a kind: the servers have too
  little capacity for that kind.
- `rate(kairos_job_failures_total[1h])` is above 0: read the errors.
- `rate(kairos_job_lease_kills_total[1h])` is above 0: a job stalls, or a
  server is too slow.

**The endpoint.** `GET /api/admin/jobs` gives the jobs by kind and state.
It also gives the last 20 failed jobs, newest first, with their error. Kairos
cuts each error to 500 characters. Only an organization admin can read it.
It shows the jobs of the organization and the jobs of the deployment, such
as the periodic passes. It does not show the jobs of other organizations.
`kairos admin jobs` prints the same data. The Admin overview of the GUI
shows it in the panel "Job queue".

A finished job stays in the table for 7 days. Then the worker removes it.

**The table.** These queries need a connection to the database with read
access to `public.jobs`. They show all the organizations.

**Failed jobs.** A failed job does not run again. Its reason is in
`last_error`:

```sql
SELECT kind, tenant, last_error, finished_at
  FROM public.jobs
 WHERE state = 'failed'
 ORDER BY finished_at DESC
 LIMIT 20;
```

A failed build also shows as a failed run on the code-index panel of the
repository.

**Lease expiries.** A lease that ran out means that a server stopped, lost
its connection, or stalled:

```sql
SELECT kind, tenant, leased_by, tries, last_error
  FROM public.jobs
 WHERE last_error LIKE 'The lease ran out%'
    OR last_error LIKE '%made no progress%'
 ORDER BY created_at DESC
 LIMIT 20;
```

The log of the server has these lines at WARN:

- `job stalled`: the worker stopped a job that made no progress.
- `job lost its lease to another server; stopped here`.
- `job heartbeat not written for the lease time; stopped here`: the server
  could not write to the database.
- `job failed: its lease ran out on its last try`.

**Work that waits.** Many queued jobs whose time has come mean that the
servers have too little capacity for a kind:

```sql
SELECT kind, count(*), min(run_after)
  FROM public.jobs
 WHERE state = 'queued' AND run_after <= now()
 GROUP BY kind;
```

Add a server, or raise the limit of the kind.

**The delivery log.** Each try of a webhook delivery has a row in the log of
its board: the time, the HTTP code and the error. Read it with
`kairos boards dispatch log <board>`, on the board settings page, or with
`GET /api/boards/{id}/webhook-deliveries`. A target that Kairos turned off
sends a `delivery_failing` notification to its owners.

## Related reading

- [Configuration: Job queue](../../reference/configuration/#job-queue): the
  settings.
- [Webhook deliveries](../../reference/webhooks/): the request and its
  signature.
- [Install with Helm](../../how-to/install-with-helm/): the replicas.
