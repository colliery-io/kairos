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

One span per HTTP request, named `<METHOD> <route>`:

```
GET /api/boards/{id}/items      42ms
  http.request.method  GET
  http.route           /api/boards/{id}/items
  http.response.status_code  200
  kairos.tenant        acme
```

The route is the **matched pattern**, never the concrete path. That is deliberate:
a span named with a real id would make every request its own operation in your
collector's UI, which turns a trace view into a list.

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

**You want database or MCP spans.** There are none yet — the request span is the
whole instrumentation today. It tells you *which* request was slow, not which query
inside it. That is worth knowing before you plan an investigation around it.

## See also

- [Configuration reference](../reference/configuration.md) — both variables, and
  the chart values that set them.
