"""
Database management tasks for Kairos.
"""

import os
import re
import shutil
import subprocess
import sys

import angreal  # type: ignore

from utils import run_psql, docker_up, MIGRATIONS_DIR, PROJECT_ROOT

db = angreal.command_group(name="db", about="database management commands")

# diesel CLI acquisition (KAIROS-A-0009 schema-sync, no-Homebrew rule):
# cargo-native `cargo install diesel_cli` pinned, with the postgres-bundled
# feature so libpq is compiled from source instead of expected on the host.
# Installed into target/tools so it never clobbers a user-global diesel.
DIESEL_CLI_VERSION = "2.3.6"
DIESEL_TOOLS_ROOT = PROJECT_ROOT / "target" / "tools"
DATABASE_URL = "postgres://kairos:kairos@localhost:41432/kairos"
SCHEMA_RS = PROJECT_ROOT / "crates" / "kairos-db" / "src" / "schema.rs"
# Throwaway tenant provisioned (and dropped) by schema-sync so diesel
# print-schema has a migrated tenant schema to introspect (KAIROS-T-0009).
SCHEMA_SYNC_TENANT = "schema_sync_template"
SCHEMA_SYNC_SCHEMA = f"org_{SCHEMA_SYNC_TENANT}"
NO_MIGRATIONS_MSG = (
    "no migrations/database schema yet - run after KAIROS-T-0007 "
    "lands the initial migrations in crates/kairos-db/migrations/"
)


def _run_server_subcommand(*args: str) -> int:
    """Invoke a `kairos-server` subcommand with the dev DATABASE_URL.

    Wiring choice (KAIROS-T-0007/T-0008): dev tooling calls the server
    binary's subcommands rather than psql/diesel CLI, so `angreal db *`,
    server startup, and the integration tests all exercise the single
    embedded migration trees (crates/kairos-db/migrations/{public,tenant},
    KAIROS-A-0009) and the kairos_db::tenant provisioning code path.
    """
    env = os.environ.copy()
    env.setdefault("DATABASE_URL", DATABASE_URL)
    result = subprocess.run(
        ["cargo", "run", "--quiet", "-p", "kairos-server", "--", *args],
        cwd=str(PROJECT_ROOT),
        env=env,
    )
    return result.returncode


@db()
@angreal.command(name="migrate", about="run pending public schema migrations")
def migrate():
    """Run pending public schema migrations.

    Invokes the server's `migrate` subcommand (see _run_server_subcommand).
    Idempotent: already-applied migrations are skipped.
    """
    print("Running public schema migrations (kairos-server migrate)...")
    exit_code = _run_server_subcommand("migrate")
    if exit_code != 0:
        print("Migrations failed", file=sys.stderr)
        return exit_code

    print("All migrations completed successfully")
    return 0


@db()
@angreal.command(name="reset", about="drop and recreate database")
def reset():
    """Drop and recreate the database."""
    print("Dropping and recreating database...")

    # Drop all connections and recreate
    run_psql("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")

    # Run migrations
    return migrate()


@db()
@angreal.command(
    name="create-tenant",
    about="provision a new tenant (org row + org_{slug} schema + defaults)",
    tool=angreal.ToolDescription(
        """
        Provision a tenant via `kairos-server create-tenant` (KAIROS-T-0008,
        app-level provisioning per KAIROS-A-0001): inserts the
        public.organizations row, creates the org_{slug} schema, runs the
        embedded tenant migration tree (KAIROS-S-0004) with search_path
        pinned to that schema, and seeds defaults (A-0002 board configs;
        A-0003 system templates + metadata copied into the tenant).
        Everything runs in one transaction - a failed provision leaves no
        partial state. Provisioning an existing slug fails cleanly.

        Delivery boards are per-team and are NOT created at provision time;
        the tenant starts with the strategy, initiative, and ADR boards.

        ## Preconditions
        - Compose postgres running (`angreal services up`)

        ## Examples
        ```
        angreal db create-tenant --slug acme
        angreal db create-tenant --slug acme --name "Acme Inc"
        ```

        ## Output
        Provision report (migrations applied, boards created, defaults
        copied). Non-zero exit with a typed error message on failure
        (invalid slug, already exists, unreachable database).
        """,
        risk_level="safe",
    ),
)
@angreal.argument(
    name="slug",
    long="slug",
    takes_value=True,
    required=True,
    help="organization slug (schema becomes org_{slug}; ^[a-z][a-z0-9_-]{1,62}$)",
)
@angreal.argument(
    name="name",
    long="name",
    takes_value=True,
    required=False,
    help="organization display name (defaults to slug)",
)
def create_tenant(slug: str, name: str = None):
    """Provision a tenant through the kairos-server create-tenant subcommand."""
    args = ["create-tenant", "--slug", slug]
    if name:
        args += ["--name", name]
    return _run_server_subcommand(*args)


@db()
@angreal.command(
    name="drop-tenant",
    about="DESTRUCTIVE: drop a tenant schema (CASCADE) and its org row",
    tool=angreal.ToolDescription(
        """
        Destroy a tenant via `kairos-server drop-tenant` (KAIROS-T-0008):
        drops the org_{slug} schema WITH CASCADE (all tenant data,
        unrecoverable) and deletes the public.organizations row, in one
        transaction.

        Refuses to act without --confirm; without it the command exits
        non-zero and changes nothing.

        ## When to use
        - Removing scratch/demo tenants in development
        - NEVER against data anyone cares about without explicit human
          sign-off

        ## Examples
        ```
        angreal db drop-tenant --slug acme            # refused (no --confirm)
        angreal db drop-tenant --slug acme --confirm  # actually drops
        ```
        """,
        risk_level="destructive",
    ),
)
@angreal.argument(
    name="slug",
    long="slug",
    takes_value=True,
    required=True,
    help="organization slug to drop",
)
@angreal.argument(
    name="confirm",
    long="confirm",
    takes_value=False,
    is_flag=True,
    help="required acknowledgement that all tenant data will be destroyed",
)
def drop_tenant(slug: str, confirm=False):
    """Drop a tenant through the kairos-server drop-tenant subcommand."""
    args = ["drop-tenant", "--slug", slug]
    if confirm:
        args.append("--confirm")
    return _run_server_subcommand(*args)


@db()
@angreal.command(
    name="list-tenants",
    about="list provisioned tenants (org rows + schema existence)",
    tool=angreal.ToolDescription(
        """
        List tenants via `kairos-server list-tenants` (KAIROS-T-0008):
        every public.organizations row with its slug, display name, and
        whether the org_{slug} schema actually exists (drift shows up as
        MISSING). Read-only.

        ## Preconditions
        - Compose postgres running (`angreal services up`)
        """,
        risk_level="safe",
    ),
)
def list_tenants():
    """List tenants through the kairos-server list-tenants subcommand."""
    return _run_server_subcommand("list-tenants")


@db()
@angreal.command(
    name="migrate-tenants",
    about="run pending tenant migrations across all tenant schemas",
    tool=angreal.ToolDescription(
        """
        Fleet migration via `kairos-server migrate-tenants` (KAIROS-T-0008):
        iterates public.organizations, pins search_path to each org_{slug}
        schema, and runs the pending embedded tenant migrations there
        (each schema tracks its own __diesel_schema_migrations). Idempotent:
        up-to-date tenants report "up to date" and are untouched.

        ## When to use
        - After adding a migration to crates/kairos-db/migrations/tenant/

        ## Output
        Per-tenant results; non-zero exit naming the tenant on the first
        failure.
        """,
        risk_level="safe",
    ),
)
def migrate_tenants():
    """Run tenant migrations through the kairos-server migrate-tenants subcommand."""
    return _run_server_subcommand("migrate-tenants")


@db()
@angreal.command(name="psql", about="open psql shell")
def psql():
    """Open an interactive psql shell."""
    subprocess.run([
        "docker", "exec", "-it", "kairos-postgres",
        "psql", "-U", "kairos", "-d", "kairos"
    ])
    return 0


@db()
@angreal.command(
    name="seed",
    about="seed the demo tenant fixture (kairos-server seed-demo)",
    tool=angreal.ToolDescription(
        """
        KAIROS-A-0012 seed-demo fixture (KAIROS-T-0035): invokes
        `kairos-server seed-demo` to provision the `demo` tenant with the
        Dex fixture users (alice = org admin, bob, carol - external_ids
        match the subs Dex mints, so first login upserts onto the seeded
        rows), teams `platform`/`web` with their delivery boards, the
        `customer-portal` delivery stream, a strategy -> two initiatives
        (one with an attached PRD document from the prd template), standing
        bug/tech-debt bucket initiatives, eight tasks across boards and
        columns with blocks edges and priority metadata, and a superseded
        ADR pair. Integration tests and skills verification consume the
        same fixture: "a running Kairos with known data" is this one
        command.

        Idempotent: if the demo tenant already exists the run is a polite
        no-op (exit 0) telling you to pass --force; with --force the demo
        tenant (and ONLY the demo tenant) is dropped and reseeded in one
        transaction.

        ## Preconditions
        - Compose postgres running (`angreal services up`)

        ## Examples
        ```
        angreal db seed            # no-op if demo already exists
        angreal db seed --force    # drop + reseed the demo tenant
        ```

        ## Output
        The seed report (counts + short codes), or the polite no-op
        notice. Non-zero exit on any failure.
        """,
        risk_level="destructive",
    ),
)
@angreal.argument(
    name="force",
    long="force",
    takes_value=False,
    is_flag=True,
    help="drop and recreate the demo tenant if it already exists",
)
def seed(force=False):
    """Seed the demo tenant through the kairos-server seed-demo subcommand."""
    args = ["seed-demo"]
    if force:
        args.append("--force")
    return _run_server_subcommand(*args)


def _diesel_supports_postgres(diesel_bin: str) -> bool:
    """Check whether a diesel binary was built with the postgres backend."""
    try:
        result = subprocess.run(
            [diesel_bin, "--version"], capture_output=True, text=True
        )
    except OSError:
        return False
    if result.returncode != 0:
        return False
    # `diesel --version` prints "Supported Backends: postgres sqlite ..."
    return "postgres" in result.stdout.lower().replace("postgresql", "postgres")


def _acquire_diesel_cli():
    """Locate or install a postgres-capable diesel CLI (no Homebrew).

    Preference order:
    1. project-local install at target/tools/bin/diesel
    2. a `diesel` already on PATH, if built with the postgres backend
    3. `cargo install diesel_cli` (pinned, postgres-bundled) into
       target/tools - rustup/cargo-native, compiles libpq from source

    Returns the path to a usable binary, or None on failure.
    """
    local_diesel = DIESEL_TOOLS_ROOT / "bin" / "diesel"
    if local_diesel.exists() and _diesel_supports_postgres(str(local_diesel)):
        return str(local_diesel)

    path_diesel = shutil.which("diesel")
    if path_diesel and _diesel_supports_postgres(path_diesel):
        return path_diesel

    print(
        f"No postgres-capable diesel CLI found; installing diesel_cli "
        f"{DIESEL_CLI_VERSION} (postgres-bundled) into {DIESEL_TOOLS_ROOT} "
        f"via cargo (this compiles libpq from source and can take a few minutes)..."
    )
    result = subprocess.run(
        [
            "cargo", "install", "diesel_cli",
            "--version", DIESEL_CLI_VERSION,
            "--no-default-features", "--features", "postgres-bundled",
            "--root", str(DIESEL_TOOLS_ROOT),
            "--locked",
        ],
        cwd=str(PROJECT_ROOT),
    )
    if result.returncode != 0:
        print("cargo install diesel_cli failed", file=sys.stderr)
        return None
    if local_diesel.exists() and _diesel_supports_postgres(str(local_diesel)):
        return str(local_diesel)
    print("installed diesel CLI is unusable", file=sys.stderr)
    return None


def _print_schema(diesel_bin: str, schema: str):
    """Run `diesel print-schema --schema {schema}`; return stdout or None."""
    result = subprocess.run(
        [diesel_bin, "print-schema", "--database-url", DATABASE_URL,
         "--schema", schema],
        cwd=str(PROJECT_ROOT),
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        print(result.stderr, file=sys.stderr)
        print(f"diesel print-schema --schema {schema} failed", file=sys.stderr)
        return None
    return result.stdout


def _unwrap_schema_module(output: str, module: str):
    """Extract + dedent the body of print-schema's `pub mod {module} {{ ... }}`.

    diesel print-schema with an explicit --schema wraps everything in a
    `pub mod {schema} { ... }` block; the combined single schema.rs
    (KAIROS-A-0009) is flat, so the wrapper is removed and the body
    dedented by one level. Returns a list of lines, or None if the wrapper
    is missing (unexpected print-schema output).
    """
    lines = output.splitlines()
    try:
        start = lines.index(f"pub mod {module} {{")
    except ValueError:
        return None
    end = len(lines) - 1
    while end > start and lines[end].strip() != "}":
        end -= 1
    if end <= start:
        return None
    return [
        line[4:] if line.startswith("    ") else line
        for line in lines[start + 1:end]
    ]


def _qualify_public_tables(lines):
    """Prefix each `diesel::table!` header ident with `public.`.

    diesel omits the schema qualifier for the connection's default schema
    ('public'), but the combined schema.rs declares public tables
    schema-qualified so they resolve regardless of the tenant search_path
    pinned on a pooled connection (KAIROS-A-0009/T-0009).
    """
    out = []
    prev_was_table_open = False
    for line in lines:
        if prev_was_table_open:
            match = re.match(r"^(\s*)([A-Za-z_][A-Za-z0-9_]*) \(", line)
            if match:
                line = f"{match.group(1)}public.{line.lstrip()}"
        out.append(line)
        prev_was_table_open = line.strip() == "diesel::table! {"
    return out


@db()
@angreal.command(
    name="schema-sync",
    about="regenerate crates/kairos-db/src/schema.rs via diesel print-schema",
    tool=angreal.ToolDescription(
        """
        Regenerate `crates/kairos-db/src/schema.rs` from the migrated dev
        database using `diesel print-schema` (KAIROS-A-0009: schema.rs is
        generated, never hand-edited).

        Two-pass generation (KAIROS-T-0009): a throwaway tenant
        (schema_sync_template) is provisioned through the real
        kairos_db::tenant provisioning code, then
        (1) `print-schema --schema org_schema_sync_template` produces the
        tenant tables - module wrapper and schema qualification stripped so
        they are UNQUALIFIED and resolve via the connection search_path -
        and (2) `print-schema --schema public` produces the public tables,
        rewritten to be SCHEMA-QUALIFIED (`public.users` style) so they
        resolve regardless of the pinned tenant search_path. The two parts
        are concatenated deterministically and the throwaway tenant is
        dropped.

        Lifecycle: verify migrations exist -> compose up (postgres) ->
        apply all migrations -> provision schema_sync_template ->
        print-schema x2 -> overwrite crates/kairos-db/src/schema.rs ->
        drop schema_sync_template. Services are left running.

        diesel CLI acquisition is cargo-native (no Homebrew): a pinned
        `cargo install diesel_cli --features postgres-bundled` into
        target/tools/ on first use; an existing postgres-capable diesel on
        PATH is reused.

        ## When to use
        - After adding or editing a migration in crates/kairos-db/migrations/
        - When schema.rs drifts from the migrated database

        ## Preconditions
        - Migrations exist (KAIROS-T-0007+). Until then this task fails
          with a clear "no migrations/database schema yet" message.
        - Docker available for the compose postgres

        ## Output
        Rewrites crates/kairos-db/src/schema.rs and reports the tables
        found. The output is deterministic: re-running against unchanged
        migrations reproduces the committed file byte-for-byte (zero
        diff). Non-zero exit on any failure; it never writes an empty or
        partial schema file.
        """,
        risk_level="destructive",
    ),
)
def schema_sync():
    """Two-pass diesel print-schema against the migrated dev DB -> schema.rs."""
    # Migrations live in nested trees (migrations/public/, migrations/tenant/),
    # each containing <version>_<name>/up.sql pairs.
    migration_dirs = (
        sorted(up.parent for up in MIGRATIONS_DIR.rglob("up.sql"))
        if MIGRATIONS_DIR.is_dir()
        else []
    )
    if not migration_dirs:
        print(f"SCHEMA-SYNC FAILED: {NO_MIGRATIONS_MSG}", file=sys.stderr)
        return 1

    print("Starting docker services...")
    exit_code = docker_up()
    if exit_code != 0:
        return exit_code

    print("Applying migrations...")
    exit_code = migrate()
    if exit_code != 0:
        return exit_code

    diesel_bin = _acquire_diesel_cli()
    if diesel_bin is None:
        return 1

    # Fresh throwaway tenant so the tenant pass introspects a fully migrated
    # tenant schema produced by the REAL provisioning code path. Drop first
    # in case a previous run left it behind (ignore "does not exist").
    print(f"Provisioning throwaway tenant '{SCHEMA_SYNC_TENANT}'...")
    _run_server_subcommand("drop-tenant", "--slug", SCHEMA_SYNC_TENANT, "--confirm")
    exit_code = _run_server_subcommand("create-tenant", "--slug", SCHEMA_SYNC_TENANT)
    if exit_code != 0:
        print("failed to provision schema-sync template tenant", file=sys.stderr)
        return exit_code

    try:
        print(f"Running diesel print-schema ({diesel_bin})...")
        tenant_output = _print_schema(diesel_bin, SCHEMA_SYNC_SCHEMA)
        public_output = _print_schema(diesel_bin, "public")
        if tenant_output is None or public_output is None:
            return 1

        tenant_lines = _unwrap_schema_module(tenant_output, SCHEMA_SYNC_SCHEMA)
        public_lines = _unwrap_schema_module(public_output, "public")
        if tenant_lines is None or public_lines is None:
            print(
                "SCHEMA-SYNC FAILED: unexpected print-schema output "
                "(missing `pub mod` wrapper)",
                file=sys.stderr,
            )
            return 1

        # Tenant tables: strip the template-schema qualification so they are
        # unqualified and resolve through the connection's search_path.
        tenant_lines = [
            line.replace(f"{SCHEMA_SYNC_SCHEMA}.", "") for line in tenant_lines
        ]
        # Public tables: diesel omits the default-schema qualifier; add it.
        public_lines = _qualify_public_tables(public_lines)

        tenant_part = "\n".join(tenant_lines).strip("\n")
        public_part = "\n".join(public_lines).strip("\n")
        if not tenant_part or not public_part:
            print(
                f"SCHEMA-SYNC FAILED: a print-schema pass returned no tables - "
                f"{NO_MIGRATIONS_MSG}",
                file=sys.stderr,
            )
            return 1

        header = (
            "// @generated by `angreal db schema-sync` "
            "(diesel print-schema, two passes).\n"
            "// Do not edit by hand - edit migrations and re-run schema-sync.\n"
            "//\n"
            "// Layout (KAIROS-A-0009 single schema.rs, KAIROS-T-0009):\n"
            "//   1. Tenant tables: UNQUALIFIED - resolved at runtime through the\n"
            "//      connection's search_path (`org_{slug}, public`; see\n"
            "//      kairos_db::pool). Generated against a throwaway migrated\n"
            "//      tenant schema (org_schema_sync_template).\n"
            "//   2. Public tables: SCHEMA-QUALIFIED (`public.*`) so they resolve\n"
            "//      no matter which tenant search_path is pinned.\n"
            "\n"
        )
        schema = f"{header}{tenant_part}\n\n{public_part}\n"
        SCHEMA_RS.write_text(schema)
        tables = schema.count("diesel::table!")
        print(
            f"Wrote {SCHEMA_RS} ({tables} table(s): "
            f"{tenant_part.count('diesel::table!')} tenant, "
            f"{public_part.count('diesel::table!')} public)."
        )
        return 0
    finally:
        print(f"Dropping throwaway tenant '{SCHEMA_SYNC_TENANT}'...")
        cleanup = _run_server_subcommand(
            "drop-tenant", "--slug", SCHEMA_SYNC_TENANT, "--confirm"
        )
        if cleanup != 0:
            print(
                f"WARNING: failed to drop {SCHEMA_SYNC_TENANT}; "
                f"`angreal db drop-tenant --slug {SCHEMA_SYNC_TENANT} --confirm` "
                "to clean up",
                file=sys.stderr,
            )


@db()
@angreal.command(
    name="backfill-embeddings",
    about="bring every tenant's retrieval vectors up to date",
    tool=angreal.ToolDescription(
        """
        Run the embedding backfill (KAIROS-T-0190, A-0021 rules 3 and 4).

        Asks the database what is still stale and embeds a bounded page at a
        time, so it is resumable by construction: interrupting it loses at most
        one page and re-running continues from wherever it actually got to.
        There is no cursor to persist and none to get out of step with reality.

        Unchanged text is never sent to a model — every text is hashed and
        compared with what is stored — so a second run over a current tenant
        does no work at all.

        ## When to use
        - After enabling embeddings on a deployment that already holds work
        - After changing KAIROS_EMBED_PROVIDER or the model, which invalidates
          every stored vector: they are in a different vector space
        - To catch up a tenant whose writes outran the embedder

        ## Related tasks
        - `dev fetch-model` - populate the local model cache first, from source
        - `db seed` - the demo tenant this is usually run against locally

        ## Output
        Per tenant: items updated, texts embedded, elapsed time, and how many
        items are now current. Embeddings switched off is reported and succeeds:
        search falls back to lexical by design.
        """,
        risk_level="safe",
    ),
)
@angreal.argument(
    name="tenant", long="tenant", takes_value=True,
    help="only this tenant slug (default: every tenant)",
)
@angreal.argument(
    name="batch", long="batch", takes_value=True, python_type="int",
    help="items per page (default 100)",
)
@angreal.argument(
    name="max_batches", long="max-batches", takes_value=True, python_type="int",
    help="stop after this many pages, to take a bite rather than the whole thing",
)
@angreal.argument(
    name="pause_ms", long="pause-ms", takes_value=True, python_type="int",
    help="sleep between pages, to throttle a shared or metered provider",
)
def backfill_embeddings(tenant=None, batch=None, max_batches=None, pause_ms=None):
    """Embed whatever is stale, a bounded page at a time."""
    args = ["embed-backfill"]
    for flag, value in (
        ("--tenant", tenant),
        ("--batch", batch),
        ("--max-batches", max_batches),
        ("--pause-ms", pause_ms),
    ):
        if value is not None:
            args += [flag, str(value)]

    env = os.environ.copy()
    env.setdefault("DATABASE_URL", DATABASE_URL)
    # A source build has no baked model layer, so point at the cache
    # `dev fetch-model` fills and let this one path download if asked.
    env.setdefault("KAIROS_EMBED_CACHE", str(PROJECT_ROOT / "target" / "embed-cache"))

    print("Building kairos-server...", flush=True)
    code = subprocess.run(
        ["cargo", "build", "--quiet", "-p", "kairos-server", "--bin", "kairos-server"],
        cwd=str(PROJECT_ROOT),
    ).returncode
    if code:
        return code
    return subprocess.run(
        [str(PROJECT_ROOT / "target" / "debug" / "kairos-server"), *args],
        cwd=str(PROJECT_ROOT),
        env=env,
    ).returncode


@db()
@angreal.command(
    name="index-embeddings",
    about="pin the vector columns and build the approximate-search indexes",
    tool=angreal.ToolDescription(
        """
        Pin each tenant's vector columns to the configured model's width and
        build HNSW indexes on them (KAIROS-T-0190).

        This is deliberately NOT a migration. pgvector refuses to index a column
        of unspecified width, so the columns must be pinned first — but the width
        is a deployment fact, not a schema constant: 384 for the bundled local
        model, 1536 for OpenAI's text-embedding-3-small. A migration pinning 384
        would refuse every write on any deployment that brought its own endpoint,
        which is the option A-0021 rule 1 exists to preserve.

        Refuses, rather than converting, if stored rows disagree with the
        configured width. Re-embed first.

        ## When to use
        - After the first backfill, once vectors exist to index
        - After changing model, once `db backfill-embeddings` has re-embedded

        ## Related tasks
        - `db backfill-embeddings` - produce the vectors this indexes

        ## Output
        Per tenant: what was pinned or built, and how long it took. Idempotent —
        a second run reports "already pinned and indexed".
        """,
        risk_level="caution",
    ),
)
@angreal.argument(
    name="tenant", long="tenant", takes_value=True,
    help="only this tenant slug (default: every tenant)",
)
def index_embeddings(tenant=None):
    """Pin vector columns to the configured width and build HNSW indexes."""
    args = ["embed-index"]
    if tenant is not None:
        args += ["--tenant", tenant]

    env = os.environ.copy()
    env.setdefault("DATABASE_URL", DATABASE_URL)
    env.setdefault("KAIROS_EMBED_CACHE", str(PROJECT_ROOT / "target" / "embed-cache"))

    print("Building kairos-server...", flush=True)
    code = subprocess.run(
        ["cargo", "build", "--quiet", "-p", "kairos-server", "--bin", "kairos-server"],
        cwd=str(PROJECT_ROOT),
    ).returncode
    if code:
        return code
    return subprocess.run(
        [str(PROJECT_ROOT / "target" / "debug" / "kairos-server"), *args],
        cwd=str(PROJECT_ROOT),
        env=env,
    ).returncode
