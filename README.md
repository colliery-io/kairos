# Kairos

Flight Levels work management: strategy, initiatives and delivery on boards
that connect to each other, with an agent-facing surface so machines can do the
work alongside people. One stateless binary serves the GUI, the REST API, MCP
and SCIM, against a PostgreSQL and an OIDC issuer you bring.

**📖 Documentation: <https://colliery-io.github.io/kairos/>**

The book is organised by what you are doing — a tutorial to see it work,
how-to guides for a goal you already have, reference for a fact you need to
trust, and explanation for why it is shaped this way. Start with
[Run Kairos locally](https://colliery-io.github.io/kairos/tutorials/run-kairos-locally.html).

## Install the CLI

`kairos` is the command-line client. It talks to the same HTTP API as the GUI
and MCP clients.

```sh
# macOS Apple Silicon
curl -fsSL -o kairos.tar.gz \
  https://github.com/colliery-io/kairos/releases/download/v0.2.0/kairos-0.2.0-aarch64-apple-darwin.tar.gz

# macOS Intel
curl -fsSL -o kairos.tar.gz \
  https://github.com/colliery-io/kairos/releases/download/v0.2.0/kairos-0.2.0-x86_64-apple-darwin.tar.gz

# Linux x86_64
curl -fsSL -o kairos.tar.gz \
  https://github.com/colliery-io/kairos/releases/download/v0.2.0/kairos-0.2.0-x86_64-unknown-linux-gnu.tar.gz

# Linux arm64
curl -fsSL -o kairos.tar.gz \
  https://github.com/colliery-io/kairos/releases/download/v0.2.0/kairos-0.2.0-aarch64-unknown-linux-gnu.tar.gz

tar -xzf kairos.tar.gz
install -m 0755 kairos ~/.local/bin/   # or any directory on your PATH
kairos login --url https://kairos.example.com
```

Each tarball has a `.sha256` beside it on the release. The
[CLI reference](https://colliery-io.github.io/kairos/reference/cli.html) covers
every command and flag.

## Run a deployment

```sh
helm install kairos oci://ghcr.io/colliery-io/charts/kairos --version 0.2.0 \
  -f my-values.yaml
```

The chart bundles no identity provider — that is always yours (KAIROS-A-0016).
It stands an evaluation PostgreSQL up inside the release unless you name your
own. See
[Install with Helm](https://colliery-io.github.io/kairos/how-to/install-with-helm.html)
for the values it requires and
[Configure an OIDC issuer](https://colliery-io.github.io/kairos/how-to/configure-an-oidc-issuer.html)
for the identity half.

The image is multi-arch from v0.1.1 (`linux/amd64` and `linux/arm64`), so it
runs on an ARM node — including a local `kind` cluster on an Apple Silicon Mac.

## Development

Development tasks run through [angreal](https://pypi.org/project/angreal/), so
agents, humans and CI share one entry point:

```sh
angreal services up          # Postgres + Dex via docker compose
angreal db migrate           # create the schema
angreal db seed              # fill the `demo` tenant with sample data
angreal web build            # compile the GUI to WebAssembly
angreal dev serve            # run the server on :41080

angreal test all             # the static gate plus every tier
angreal test unit            # pure cargo unit tests, no services
angreal test integration     # real Postgres + Dex
angreal test e2e             # API + MCP + GUI smoke
angreal test uat             # persona journeys, with a readable report
angreal docs serve           # the documentation book, live-reloading
```

`angreal tree` lists everything. Once the server is up, sign in at
<http://localhost:41080> as `alice@kairos.test` / `alice-password`.

Documentation for changing Kairos lives next to the code it describes, rather
than in the book:

| | |
|---|---|
| [`uat/README.md`](uat/README.md) | the user-acceptance journeys and the surface drift gate |
| [`e2e/README.md`](e2e/README.md) | the end-to-end smoke |
| [`docs/gui-conventions.md`](docs/gui-conventions.md) | GUI conventions — design tokens, never raw colours |
| [`plugin/README.md`](plugin/README.md) | the Claude Code plugin |
| [`.metis/`](.metis/) | the Flight Levels work record: vision, initiatives, ADRs, specifications |

Architecture decisions are in [`.metis/adrs/`](.metis/adrs/); the testing and
verification strategy is [`KAIROS-A-0012`](.metis/adrs/KAIROS-A-0012.md).

## CI

`.github/workflows/ci.yml` runs the KAIROS-A-0012 gates on every push to `main`
and every pull request: `cargo fmt --check`, `clippy --workspace --all-targets
-D warnings`, `angreal test unit`, `angreal test integration`, and a check that
the generated REST reference still matches the OpenAPI spec.

Two workflows publish, deliberately on different triggers:

- **`release.yml`** on a `v*` tag — the four CLI tarballs, the
  `ghcr.io/colliery-io/kairos` image, and the Helm chart to
  `oci://ghcr.io/colliery-io/charts`.
- **`docs.yml`** on pushes touching `docs/**` — the documentation book to
  GitHub Pages, so prose ships without waiting for a version bump.

## Licence

Apache License 2.0 — see [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE). Both
travel with every artefact: the CLI tarballs, the image (at
`/usr/share/doc/kairos/`), and the source.
