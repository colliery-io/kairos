---
id: database-spans-say-which-query
level: task
title: "Database spans: say which query made the request slow, not just that it was"
short_code: "KAIROS-T-0199"
created_at: 2026-09-26T11:19:50.247914+00:00
updated_at: 2026-09-26T11:38:05.396356+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#feature"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: NULL
---

# Database spans: say which query made the request slow, not just that it was

## Objective

Put a span around each unit of database work, so a trace answers *which* query made
a request slow rather than only that the request was.

[[KAIROS-T-0196]] shipped OTLP export and one span per HTTP request. That answers
"which request was slow". The question in that ticket's own justification —
"which of these eleven queries made the board take four seconds" — is still
unanswered, and its how-to says so under troubleshooting. This closes that.

## Backlog Item Details

### Type
- [x] Feature - New functionality or enhancement

### Priority
- [x] P2 - Medium (tracing is useful today and this is what makes it answer the question it was bought for)

### Why it is small

All database work funnels through two methods on `BlockingTenantPool`
(`crates/kairos-server/src/blocking.rs`): `run` for tenant-scoped work and
`run_public` for the readiness probe. **134 call sites, two places to instrument** —
the same shape that made the request span cheap in T-0196, where the metrics
middleware already wrapped everything.

The async `TenantPool` is used in four places (JIT user upsert, whoami); worth
covering for completeness but it is not where request latency lives.

### The interesting problem: naming the query

A span called `db.query` on every one of 134 call sites is not much better than no
span — it tells you time went to the database, which `/metrics` already implies. To
answer *which*, the span needs to identify the call site.

The closure is opaque, so the options are:

- **A `label: &'static str` parameter.** Precise, and touches 134 call sites. Too
  invasive for the value, and a parameter people must remember to set well is a
  parameter that ends up saying `"query"`.
- **`#[track_caller]` + `std::panic::Location::caller()`.** The caller's
  `file:line` identifies the query site exactly, with **no call-site changes**.
  Maps onto OpenTelemetry's `code.filepath` / `code.lineno` conventions.

Second option, unless it turns out `#[track_caller]` does not give the call site
through an `async fn` — which is worth **verifying rather than assuming**, since the
attribute's interaction with generated futures has historically been limited. If it
does not work, a thin non-async wrapper capturing `Location::caller()` does.

### The trap

`spawn_blocking` moves the closure to another thread, and **tracing context does not
follow it**. The span has to be captured before the hop and entered inside the
closure, or every database span is an orphan at the root of the trace rather than a
child of the request — which looks like it works, in the sense that spans appear.

That is the failure this ticket most needs to verify against a real collector rather
than reason about.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] Each `BlockingTenantPool::run` / `run_public` call produces a span that is a
      **child of the request span**, verified against a collector rather than
      inferred
- [x] The span identifies its call site (`code.filepath`, `code.lineno`) so "which
      query" is answerable, with no changes at the 134 call sites
- [x] The tenant is on the span, as it is on the request span
- [x] Tracing off (`KAIROS_OTEL_ENDPOINT` unset) costs nothing measurable — spans
      are created either way, so this is about not adding a query or a round trip
- [x] `how-to/export-traces.md` drops the "there are none yet" caveat and shows what
      a trace now looks like
- [x] Ladder green

## Status Updates

**2026-09-26 — filed.** The gap was named at the end of [[KAIROS-T-0196]] and in its
how-to rather than tracked; this is the ticket for it.
### 2026-09-26 — the trace answers the question now

```
GET /api/tasks                                        (ROOT)
├── auth.jit_upsert
├── tenant.resolve
└── db.query   crates/kairos-server/src/api/tasks.rs:156
```

All one trace, all children of the request span, and the query named to the line.

### `#[track_caller]` on an `async fn` does not work, and I checked

The plan rested on `Location::caller()` naming the call site. It does not through an
`async fn` — it reports the function's own body. Rust warns about it
(`ungated_async_fn_track_caller`), and a four-line throwaway crate settled it in a
minute:

```
async fn  -> src/main.rs:3     <- its own body
sync  fn  -> src/main.rs:16    <- the call site
```

So `run` and `run_public` are now `fn`s returning `impl Future` rather than
`async fn`s. **Callers are untouched** — `pool.run(slug, |conn| …).await` compiles and
means exactly what it did — which is what keeps this a two-place change across 134
call sites instead of 134 edits. The signature is the instrumentation.

### The trap, avoided by construction rather than by luck

`spawn_blocking` moves the closure to another thread and tracing context does not
follow. The span is created in the **synchronous** part of `run`, inside the
caller's context, so it takes the request span as its parent; `.instrument()` then
carries it across the hop. Built inside the async block it would have parented to
whatever happened to be current at first poll — which still produces spans, so it
looks like it works.

Verified by decoding the OTLP protobuf and reading `parent_span_id`, not by looking
at a list of span names. A hand-rolled 40-line field walker, because "spans arrived"
was exactly the false positive to avoid.

### Two false starts worth recording

**The first probe showed no `db.query` spans at all.** I had pointed it at tenant
`acme` while the seed is `demo`, so alice had no membership and every request 403'd
before reaching a handler. I had discarded the status codes with `-o /dev/null`, so
the evidence that would have explained it immediately was thrown away. Re-run with
statuses printed: `200`, and the spans appeared.

**`tenant.resolve` needed restructuring, not wrapping.** My first attempt scoped the
connection into an async block and broke a second query further down that shared it.
Both statements are now in one span sharing one connection, which is also more
honest: two spans would measure the same checkout twice and imply a round trip that
is not there.

### What is deliberately not instrumented

- The three remaining async-pool sites — `/metrics`' pool gauge, SCIM token auth and
  API-key auth. The first is not per-request; the other two are single indexed
  lookups on paths whose latency is dominated by what follows.
- `jit_upsert_user` uses `skip_all`. The claims carry an email and a subject, and a
  span is telemetry that leaves the process; nothing here should ship a user's
  identity to a collector by accident.
- No spans *inside* a query. A `db.query` span says which call site was slow, not
  which index PostgreSQL chose — `EXPLAIN ANALYZE` at the file and line the span
  names is the next step, and the how-to says so.
- **MCP tool names are still not on spans.** An MCP call gets a `POST /mcp` request
  span with its database children, so slow work is visible, but "how slow is
  `related_work`" is not answerable yet. Recorded in the how-to's troubleshooting
  rather than left for someone to discover.

### Gates

lint clean, **403 unit tests**, integration **47/47**, e2e **17**, uat **22
journeys**, docs build green.