# Configure semantic retrieval

Make Kairos able to find work that is *about* the same thing, not only work that
uses the same words. This guide assumes you can set environment variables on the
deployment, or Helm values if you installed with the chart.

You need: a PostgreSQL with `pgvector` (Kairos requires it anyway — see
[Install with Helm](install-with-helm.md)).

## It already works

The container ships the model inside it. With no configuration at all, retrieval
runs locally: nothing leaves your network, and there is no API key to manage.

Check by asking any deployment:

```sh
curl -H "Authorization: Bearer $TOKEN" \
  https://<kairos-host>/api/items/<SHORT-CODE>/related
```

`"vector": true` in the response means it is working. `"vector": false` means it
answered from text alone — see [When it answers from text
only](#when-it-answers-from-text-only).

## Fill in the work you already have

New work is embedded within seconds by a background sweep. Work that existed
*before* you enabled retrieval is not, so run the backfill once:

```sh
kairos-server embed-backfill
```

It prints what it did, per tenant:

```
backfilling with local/bge-small-en-v1.5-q (384d)
demo: 20 item(s) updated, 41 text(s) embedded in 0.4s — 20/20 items now current
```

It is safe to interrupt and safe to re-run: it asks the database what is still
missing each time, so there is no cursor to lose. Running it on an already-current
tenant does nothing.

For a large tenant, be kind to whatever is doing the embedding:

```sh
kairos-server embed-backfill --batch 50 --pause-ms 200 --max-batches 100
```

## Build the index

Once vectors exist, build the approximate-search index:

```sh
kairos-server embed-index
```

This is a **table rewrite and an index build** — schedule it like any other.
Idempotent: a second run reports `already pinned and indexed`.

Do this *after* the backfill, not before. It refuses to run if stored vectors
disagree with the configured model, and tells you to re-embed first.

## Use a hosted model instead

Point Kairos at anything speaking the OpenAI embeddings shape — OpenAI, Azure
OpenAI, a local Ollama, a vLLM server:

```yaml
embeddings:
  url: "https://api.openai.com/v1"
  model: "text-embedding-3-small"
  existingSecret: "kairos-embed"   # holding KAIROS_EMBED_API_KEY
```

Or as environment variables: `KAIROS_EMBED_URL`, `KAIROS_EMBED_MODEL`,
`KAIROS_EMBED_API_KEY`. Setting the URL selects the remote provider on its own; a
local Ollama needs no key.

**Changing model invalidates every stored vector.** Vectors from two models are
not comparable, even at the same width, so Kairos treats them as missing rather
than silently comparing across them. After any change:

```sh
kairos-server embed-backfill   # re-embed everything under the new model
kairos-server embed-index      # re-pin the columns and rebuild
```

Until that finishes, retrieval answers from text alone and says so. It does not
return wrong answers in the meantime.

## Turn it off

```yaml
embeddings:
  provider: none
```

Search still works. `related_work` says it is not enabled and points at ordinary
search; `/api/items/{code}/related` returns **503** rather than an empty list,
because an empty list would read as "nothing is related".

## When it answers from text only

`"vector": false`, or a note saying *"Text only"*. Three causes, in the order
worth checking:

1. **Nothing has been embedded yet** — run the backfill above.
2. **The model changed** and the stored vectors are from the old one. The
   backfill fixes it; `embed-backfill` prints `N from another model` when this is
   the cause.
3. **The provider will not start.** The server logs a warning at startup and
   keeps serving. For a remote endpoint, that is usually the URL, the key, or the
   model name. For the local model in a source build rather than the container,
   it is a missing model cache — `angreal dev fetch-model` fills it.

In every case search keeps working and the answers stay honest about being thin.

## Watch the suggestions

Retrieval suggests edges; people confirm them. Whether those suggestions are any
good is visible in how often they are accepted — a confirm rate drifting toward
zero means the suggestions are not recognisable, which is worth knowing before
everyone starts ignoring them.

## Related reading

- [Finding related work](../explanation/finding-related-work.md) — why the
  results are proposals, and why there is no threshold to tune
- [Install with Helm](install-with-helm.md) — the `pgvector` requirement and the
  bundled evaluation database
- [Configuration](../reference/configuration.md) — every `KAIROS_EMBED_*`
  variable and chart value
