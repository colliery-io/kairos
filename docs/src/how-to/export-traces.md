# Export traces to a collector

Get Kairos's HTTP spans into an OpenTelemetry collector, so you can answer "why
was that request slow?" rather than only "how many requests were slow".

You need: a collector reachable from the Kairos pods or container, with its
**OTLP/HTTP** receiver enabled.

## Point Kairos at the collector

One value. Everything else has a working default.

```yaml
config:
  otel:
    endpoint: "http://otel-collector.observability:4318/v1/traces"
```

Or, on the Compose stack, in your `.env`:

```sh
KAIROS_OTEL_ENDPOINT=http://otel-collector:4318/v1/traces
```

Restart, and the log line confirms it:

```
otel: exporting traces over OTLP/HTTP  endpoint=... sample_ratio=1
```

**Unset means off.** With no endpoint Kairos builds no exporter and no span leaves
the process, which is the default.

## Mind the port

Kairos exports **OTLP over HTTP**, not gRPC. That means:

- the port is **4318**, not 4317
- the path is part of the endpoint — `/v1/traces`

A collector's gRPC port will happily accept the TCP connection and then reject
every payload, so a wrong port looks like "connected but nothing arrives". If your
collector only exposes 4317, enable its `otlp/http` receiver:

```yaml
receivers:
  otlp:
    protocols:
      http:
        endpoint: 0.0.0.0:4318
```

## Turn the volume down

Every request is sampled by default, because someone who configured a collector
wants to see spans in it. On a busy deployment that is more than you want to
store:

```yaml
config:
  otel:
    sampleRatio: "0.05"   # keep 5% of traces
```

This is **head** sampling: the decision is made when a trace starts, and it
respects a decision an upstream service already made, so a trace is never cut in
half.

If what you actually want is "keep the slow ones and the failures, drop the rest",
that is **tail** sampling and it belongs in your collector — the collector sees a
whole trace before deciding, and Kairos cannot. A `tail_sampling` processor with a
latency policy does this and pairs well with a low ratio here set to `1.0` instead,
letting the collector do the choosing.

An unparseable or out-of-range ratio **fails startup** rather than being clamped: a
silently clamped typo produces a collector that is mysteriously empty, and nothing
tells you why.

## What you get

A span per HTTP request, with the work inside it as children:

```
GET /api/boards/{id}/items                                   42ms
  http.request.method  GET
  http.route           /api/boards/{id}/items
  http.response.status_code  200
  kairos.tenant        acme
│
├── auth.jit_upsert                                           2ms
├── tenant.resolve                                            3ms
└── db.query                                                 35ms
      db.system      postgresql
      code.filepath  crates/kairos-server/src/api/org/boards.rs
      code.lineno    538
      kairos.tenant  acme
```

`code.filepath` and `code.lineno` are the point: they name **which query**, to the
line. A board that takes four seconds shows you the call site responsible rather
than leaving you to guess which of a handler's queries it was, and a handler that
makes eleven round trips shows eleven children.

`auth.jit_upsert` and `tenant.resolve` are on every authenticated request, so time
spent there would otherwise be an unexplained gap before the first query.

The request span's route is the **matched pattern**, never the concrete path. That
is deliberate: a span named with a real id would make every request its own
operation in your collector's UI, which turns a trace view into a list. Query spans
are named by call site for the same reason — grouping by `code.filepath` and
`code.lineno` is meaningful where grouping by a rendered SQL string is not.

Only **5xx** marks a span as an error. A 404 or a 403 is the server working
correctly, and flagging those would make every permission check look like an
incident.

## When it does not work

**The log says `otel: tracing is DISABLED`.** The endpoint could not be turned into
an exporter — usually a malformed URL. Kairos keeps serving: telemetry is how you
observe the product, not part of it.

**No error, and nothing in the collector.** Almost always the port: see *Mind the
port* above. Check with `curl -v http://your-collector:4318/v1/traces` — an HTTP
receiver answers, a gRPC port does not.

**Spans appear but stop when the process restarts.** Expected for in-flight
batches on an unclean kill; Kairos flushes on a normal shutdown.

**A handler shows no `db.query` children.** It did no database work on that request
— a cache-free read served entirely from the request, or a refusal before the
handler ran. Check the status code on the request span.

**You want spans inside a query.** There are none: a `db.query` span covers one unit
of work through the connection pool, so it tells you *which* call site was slow, not
which index PostgreSQL chose. `EXPLAIN ANALYZE` on the statement at the file and
line the span names is the next step, and the span exists to tell you where to point
it.

**You want MCP tool spans.** There are none yet. An MCP call produces a request span
(`POST /mcp`) with its database children, so slow work is visible — but the tool
name is not on the span, so you cannot yet group by "how slow is `related_work`".

## See also

- [Configuration reference](../reference/configuration.md) — both variables, and
  the chart values that set them.
