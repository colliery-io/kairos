---
id: export-traces-over-otlp-or-stop
level: task
title: "Export traces over OTLP, or stop implying we can"
short_code: "KAIROS-T-0196"
created_at: 2026-09-25T00:12:54.058071+00:00
updated_at: 2026-09-25T03:09:56.366688+00:00
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

# Export traces over OTLP, or stop implying we can

## Objective

Give Kairos real OpenTelemetry trace export, or decide deliberately that it has
none. Until [[KAIROS-T-0177]] the chart offered `config.otelEndpoint`, emitted
`KAIROS_OTEL_ENDPOINT`, and documented both — and no crate had ever read it.
T-0177 removed the switch rather than wire it, because wiring it is a feature
and leaving a dead switch on the panel is worse than having no switch.

This task is where the intent behind that switch gets honoured or dropped on the
record.

## Backlog Item Details

### Type
- [x] Feature - New functionality or enhancement

### Priority
- [x] P2 - Medium (nice to have)

### Business Justification

- **User value**: an operator running Kairos next to other services wants its
  spans in the same collector as everything else. Today the only telemetry out
  of the process is `/metrics` and structured logs — good for "is it up" and
  "what happened", useless for "which of these eleven queries made the board
  take four seconds".
- **Business value**: real, but not urgent. Nobody is blocked; the deployment
  surface is now honest about what it does not do, which is the part that was
  actually broken.

### What it would take

- `opentelemetry` + `opentelemetry-otlp` + `tracing-opentelemetry`, layered onto
  the existing `tracing` subscriber that `KAIROS_LOG_FORMAT` already configures.
- `KAIROS_OTEL_ENDPOINT` back in `config.rs` — which is what makes it real, per
  the [[KAIROS-T-0177]] drift test: a variable the deployments set and the
  binary does not read now fails a test, in either direction.
- The chart value, the compose variable, and
  `docs/src/reference/configuration.md` restored together.
- A decision about sampling, because tracing every request against a work-item
  board is a way to pay for a lot of storage to learn nothing.

### The alternative worth considering

Do nothing, and say so. `/metrics` plus structured logs is a defensible amount
of observability for a single-binary self-hosted product, and OTLP brings a
dependency tree, a sampling decision and an exporter to keep working. If that is
the answer, the outcome of this task is a line in [[KAIROS-A-0013]] rather than
code — which is a real outcome, not a cop-out.

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

## Acceptance Criteria

- [x] A decision, recorded: export OTLP traces, or state in an ADR that Kairos
      deliberately exposes metrics and logs and not traces
- [x] If exporting: `KAIROS_OTEL_ENDPOINT` is read by `config.rs`, the chart
      value and compose variable return, the book documents it, and the
      [[KAIROS-T-0177]] drift tests pass without an exception entry
- [n/a] If not exporting: nothing in the chart, compose, or the book implies
      otherwise — which is already true as of [[KAIROS-T-0177]], so this
      collapses to the ADR line

## Status Updates

**2026-09-25 — filed by [[KAIROS-T-0177]].** That task's audit found three
variables that went nowhere. Two were the reverse defect (documented, never
forwarded) and were fixed. This one was the forward defect: offered, documented,
and read by nothing. Removing it was the honest fix for a bug ticket; adding the
feature it was pretending to be is this ticket.

## Decision — 2026-09-25 (Dylan)

**Wire it up properly.** Kairos exports OTLP traces; the removed switch comes back
connected to something.

Against my recommendation, which was to record "no traces" in an ADR on the
grounds that metrics plus structured logs is enough for a single-binary self-hosted
product. Taking the decision as made: the argument for tracing is that `/metrics`
answers "is it up" and logs answer "what happened", and neither answers "which of
these eleven queries made the board take four seconds" — which is the question an
operator actually has, and the one this product has no answer to.

Scope, from the notes above plus what T-0177's test now enforces:

- `opentelemetry`, `opentelemetry-otlp`, `tracing-opentelemetry`, layered onto the
  subscriber `KAIROS_LOG_FORMAT` already configures.
- `KAIROS_OTEL_ENDPOINT` read by `config.rs`. **This is what makes it real**: the
  [[KAIROS-T-0177]] drift test fails on a variable the deployments set and the
  binary does not read, in either direction — so the chart value cannot come back
  without the code, and the code cannot land without the chart value.
- The chart value, the compose variable, and
  `docs/src/reference/configuration.md` restored together.
- **A sampling decision, which is the real design work.** Tracing every request
  against a work-item board buys a lot of storage to learn very little. Head
  sampling with a configurable ratio is the cheap answer; tail sampling needs a
  collector and is the operator's business, not ours.
## Status Updates

### 2026-09-25 — exported, and wiring the exporter was only half of it

Kairos exports OTLP traces. The switch the chart used to offer is back, connected
to something.

### The finding: there were no spans to export

The exporter went in, reported itself as exporting, and **zero bytes reached the
collector**. Kairos created no spans anywhere — no `TraceLayer`, no
`#[instrument]`, no `span!`, nothing. An OTLP *trace* exporter on a process that
emits no spans is an empty stream, so the feature would have shipped looking broken
rather than absent.

Found by pointing it at a fake collector on a local port and counting POSTs. No
test would have caught it: the exporter was built correctly and every assertion one
would naturally write about configuration passed.

So there is now one span per HTTP request, added to the middleware that already
wraps the whole router and already computes the route pattern, status and tenant —
rather than pulling in `tower-http`'s `TraceLayer` to re-derive them. It carries
`http.request.method`, `http.route`, `http.response.status_code` and
`kairos.tenant`, and is marked an error only on 5xx: a 404 or 403 is the server
working, and flagging those makes every permission check look like an incident.

The span name uses the **matched route pattern**, never the concrete path, for the
same reason the metrics labels do — a span named with a real id makes every request
its own operation in a collector's UI, which turns a trace view into a list.

### The second finding: the async client silently exported nothing

With spans flowing, still zero POSTs. The cause is a pairing, and it is invisible:
the batch span processor runs on **its own OS thread with no tokio runtime**, so
the async `reqwest` exporter had nothing to drive its futures. Spans were recorded
and dropped, with no error logged anywhere.

`reqwest-blocking-client` fixes it, and is what this crate's own default feature set
pairs with `http-proto` — which in hindsight was the hint. Blocking I/O is correct
here precisely because it is off the async runtime. Both the dependency comment and
a comment in `main.rs` that I had written asserting the opposite are corrected;
leaving a confident wrong explanation next to the thing that just bit me would be
worse than no comment.

### Decisions inside the implementation

- **HTTP/protobuf, not gRPC.** The gRPC exporter pulls tonic and a second TLS
  stack; A-0013 asks for one small binary and reqwest+rustls is already here. The
  cost is the port — 4318, not 4317 — which the how-to, the reference, the chart
  comment and `.env.example` all state, because "connected but nothing arrives" is
  a miserable thing to debug.
- **Head sampling, default 1.0.** Someone who configured a collector wants to see
  spans in it; a default that dropped 99% would read as a bug. Wrapped in
  `ParentBased` so an upstream sampling decision is respected rather than cutting a
  trace in half. Tail sampling is a collector feature and belongs there — it can
  see a whole trace and this process cannot.
- **A bad ratio fails at boot**, not clamped. A clamped typo produces a
  mysteriously empty collector with nothing to explain it.
- **A bad endpoint does not.** Telemetry is how you observe the product, not part
  of it, so an unbuildable exporter logs `otel: tracing is DISABLED` and Kairos
  keeps serving.

### Verified by running it, not by reasoning about it

| | |
|---|---|
| spans reach a collector | 1402 bytes to `/v1/traces`, service name present |
| `KAIROS_OTEL_SAMPLE_RATIO=0.0` | zero POSTs |
| malformed endpoint | `healthz=200`, log names the invalid URI |
| unreachable collector | requests stay ~1ms, server survives |

Plus unit tests for the config: off by default, 1.0 when on, inclusive bounds, and
every malformed ratio refused by name — with empty treated as unset, since blanking
a value means turning it off everywhere else in this config.

The [[KAIROS-T-0177]] drift test is what made the chart value and the code land
together: it fails on a variable the deployments set and the binary does not read,
in either direction, so neither half could have gone in alone.

### Gates

lint clean, **403 unit tests**, integration **47/47**, e2e **16**, uat **22
journeys**, `helm lint` clean with all five CI value sets rendering, docs build
green.

### Not done, and named rather than implied

There are no database or MCP spans. The request span tells you *which* request was
slow, not which query inside it — so the motivating question ("which of these
eleven queries made the board take four seconds") is closer but not answered. The
how-to says so under "When it does not work", because an operator planning an
investigation should know before they start.