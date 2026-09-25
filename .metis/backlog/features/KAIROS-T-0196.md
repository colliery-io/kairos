---
id: export-traces-over-otlp-or-stop
level: task
title: "Export traces over OTLP, or stop implying we can"
short_code: "KAIROS-T-0196"
created_at: 2026-09-25T00:12:54.058071+00:00
updated_at: 2026-09-25T00:12:54.058071+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/backlog"
  - "#feature"


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

- [ ] A decision, recorded: export OTLP traces, or state in an ADR that Kairos
      deliberately exposes metrics and logs and not traces
- [ ] If exporting: `KAIROS_OTEL_ENDPOINT` is read by `config.rs`, the chart
      value and compose variable return, the book documents it, and the
      [[KAIROS-T-0177]] drift tests pass without an exception entry
- [ ] If not exporting: nothing in the chart, compose, or the book implies
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
