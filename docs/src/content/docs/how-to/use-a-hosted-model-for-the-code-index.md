---
title: "Use a hosted model for the code index summaries"
---

Kairos writes a summary of each function and file of an indexed repository.
The test functions and the test files get a summary too. Thus a repository
with much test code sends more requests to the provider.
By default a model embedded in the server writes them, on the CPU of the
host. An organization can send this work to a hosted provider instead:
Ollama Cloud, or AWS Bedrock. This page tells an operator and an
organization admin how.

## What leaves the host

For a repository on a hosted provider, each request to the provider has:

- the code of one changed function or type, with its signature (test code
  too),
- the signatures of the functions it calls,
- for a file or a folder, the summaries of its parts,
- for a file with no functions or types of its own (for example a `lib.rs`
  of `mod` and `use` lines), its doc comment and its `mod`, `use`, import
  and export lines.

Nothing else leaves the host. The whole repository stays. The key of the
organization stays. The work of a repository on the embedded model stays.

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

## Choose the repositories on the provider

To send all the repositories to the provider, an organization admin sets
the default summarizer of the repositories. On the page Admin, Code index,
set "Default summarizer of the repositories" to `hosted`. Or:

```bash
kairos admin code-index-settings set --default-summaries hosted
```

A new repository follows the organization. A repository from before 0.9.0
keeps its value: set it to `organization` to follow the default.

To choose for one repository, set its "Code index summaries" on the page
Admin, Repositories, or:

```bash
# Keep the code of this repository on the host
kairos repos update <slug> --code-index-summaries embedded
# Follow the default of the organization again
kairos repos update <slug> --code-index-summaries organization
```

A value set on a repository wins over the default. Kairos refuses `hosted`,
as the default or on a repository, when the organization has no hosted
provider. The next build of a repository on the provider makes its
summaries there. Each run
of the builder records the model that wrote its summaries: see `kairos
repos builds <slug>`, or the panel of the repository.

## The CLI on a repository on the provider

`kairos index update` in a checkout of such a repository makes no summary.
It links the summaries of the pool of Kairos, and it says so. The CLI never
has the key of the organization.

## The search of the code tools

`code_search` of the code tools (`kairos index mcp`) compares the vector of
the query with the vectors of the summaries. One model must make the two
vectors. The CLI gets the vector of the query from the first source that it
has:

1. The local vector model of the CLI, when it is the model of the index.
2. Kairos, when the CLI has a connection for the checkout: the connection
   of `kairos index update`. Kairos makes the vector with the vector model
   of the organization. So the CLI does not need the key of the provider.
   Each search sends one short request (`POST
   /api/code-index/query-vector`), with a time limit of 5 seconds.
3. No vector. The search then uses the text of the summaries, and its first
   line tells why.

The search also finds the words of the query in the names, the signatures
and the summaries. It puts the two ranks together. A word of a name counts
more than a word of a summary. The search cuts each identifier into its
words: `parseDocument` and `parse_document` give "parse" and "document".
It finds the stem of each word: "parsed" finds "parse". The index keeps
these words in an SQLite FTS5 table with the Porter stemmer.

Test code ranks below the other code. A Rust file in a `tests/` folder is
test code too, with its helpers. To find tests, put a test word in the
query. The test words are "test", "tested", "spec", "fixture", "mock",
"stub", "assert" and their plurals. Then the test code ranks with the other
code.
For example, "how is parsing tested" gives the tests first.

A type and its `impl` blocks give one result: the type, with the best rank
of the group. Each method of an `impl` block is a different result. An
`impl` block of a type that is not in the index is a result too.

So with `--vector-provider remote`, the search uses vectors when the CLI
can connect to Kairos. A CLI with no local vector model (the release build
for Intel macOS) uses Kairos too. The text of the query goes to Kairos,
and then to the vector provider of the organization.

## The image with no embedded model

Each release has two images. `ghcr.io/colliery-io/kairos:<version>` has the
embedded model. `ghcr.io/colliery-io/kairos:<version>-hosted` has no
llama.cpp and downloads no summary model. Use it when every organization of
the deployment is on a hosted provider. With the Helm chart, set
`image.variant: hosted`.

On the hosted image, a repository on the embedded model gets a failed run.
Its text says: set a hosted provider and the repository on it, or run the
image with the model.

## Related

- [Build the code index again](../build-the-code-index-again/)
- [Configuration](../../reference/configuration/): the settings of the
  providers.
