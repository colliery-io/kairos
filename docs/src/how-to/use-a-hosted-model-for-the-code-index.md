# Use a hosted model for the code index summaries

Kairos writes a summary of each function and file of an indexed repository.
By default a model embedded in the server writes them, on the CPU of the
host. An organization can send this work to a hosted provider instead:
Ollama Cloud, or AWS Bedrock. This page tells an operator and an
organization admin how.

## What leaves the host

For a repository on a hosted provider, each request to the provider has:

- the code of one changed function or type, with its signature,
- the signatures of the functions it calls,
- for a file or a folder, the summaries of its parts.

Nothing else leaves the host. The whole repository stays. The key of the
organization stays. The work of a repository that did not opt in stays.

## Set the provider of the organization

An organization admin sets the provider on the page Admin, Code index, or
with the CLI. The secret comes from standard input.

```bash
# Ollama Cloud, 8 requests at a time
printf '%s' "$OLLAMA_KEY" | kairos admin code-index-settings set \
    --summary-provider ollama-cloud \
    --summary-url https://ollama.com/v1 \
    --summary-model gemma4:31b \
    --summary-secret-stdin \
    --concurrency 8

# AWS Bedrock: the credentials as <access key id>:<secret access key>
printf '%s:%s' "$AWS_ACCESS_KEY_ID" "$AWS_SECRET_ACCESS_KEY" | kairos admin code-index-settings set \
    --summary-provider bedrock \
    --summary-region us-east-1 \
    --summary-model anthropic.claude-3-5-haiku-20241022-v1:0 \
    --summary-secret-stdin
```

`kairos admin code-index-settings show` gives the settings. It shows whether
a secret is set, by whom and when, and never the secret. The deployment
needs `KAIROS_SECRETS_KEY`: Kairos keeps each secret encrypted with it.

The vectors of the summaries have their own setting. `--vector-provider
remote` with `--vector-url` and `--vector-model` sends them to an
OpenAI-compatible embeddings endpoint. A local Ollama needs no secret.

## Opt a repository in

A repository uses the hosted provider only after a person opts it in. On
the page Admin, Repositories, set "Code index summaries" to `hosted`. Or:

```bash
kairos repos update <slug> --code-index-summaries hosted
```

Kairos refuses `hosted` when the organization has no hosted provider. The
next build of the repository makes its summaries on the provider. Each run
of the builder records the model that wrote its summaries: see `kairos
repos builds <slug>`, or the panel of the repository.

## The CLI on an opted-in repository

`kairos index update` in a checkout of such a repository makes no summary,
whatever `KAIROS_INDEX_SUMMARIZE` says. It links the summaries of the pool
of Kairos, and it says so. The CLI never has the key of the organization.

## The image with no embedded model

Each release has two images. `ghcr.io/colliery-io/kairos:<version>` has the
embedded model. `ghcr.io/colliery-io/kairos:<version>-hosted` has no
llama.cpp and downloads no summary model. Use it when every organization of
the deployment is on a hosted provider. With the Helm chart, set
`image.variant: hosted`.

On the hosted image, a repository with no opt-in gets a failed run. Its
text says: set a hosted provider and opt the repository in, or run the
image with the model.

## Related

- [Build the code index again](build-the-code-index-again.md)
- [Configuration](../reference/configuration.md): the settings of the
  providers.
