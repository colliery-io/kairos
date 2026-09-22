# Kairos UAT — user-acceptance journeys

KAIROS-A-0012 tier 6 (KAIROS-I-0011). Persona-driven stories that walk the
application the way people and agents use it, and end in a report a
product owner can read: `reports/<run>/report.md` (+ `report.json`).

## Running

Compose stack, fresh seed (destructive to the dev `demo` tenant):

```sh
angreal test uat                       # all journeys
angreal test uat --journey smoke,planning
angreal test uat --keep-running        # leave the stack + :41080 server up
```

Any deployment (non-destructive; everything created is `uat-<run>-…` and
deleted in teardown):

```sh
angreal test uat --server https://kairos.example.com
```

Hand-runs from this directory once a target is up (`--keep-running` above,
or a dev server):

```sh
UAT_SERVER=http://localhost:41080 UAT_MODE=server npx playwright test --grep @smoke
UAT_SERVER=http://localhost:41080 UAT_MODE=server npx playwright test --ui
```

### Environment

| Variable | Default | Meaning |
|---|---|---|
| `UAT_SERVER` | `http://localhost:41080` | GUI + API + MCP origin under test |
| `UAT_ISSUER` | `http://localhost:41558/dex` | OIDC issuer for the headless PKCE login |
| `UAT_TENANT` | `demo` | `X-Tenant` for API/MCP calls |
| `UAT_MODE` | `compose` | `compose` runs compose-only steps; `server` skips them |
| `UAT_PERSONA_<NAME>_EMAIL` / `_PASSWORD` | the seed users | credentials for `ALICE`, `BOB`, `CAROL`, `NEWHIRE` |
| `UAT_KAIROS_BIN` | `../target/debug/kairos` | the CLI binary the CLI surface executes |
| `UAT_REPORT_DIR` | `reports/<run>` | where the report lands |
| `UAT_HEADED` | unset | `1` shows the browser |

The deployment's IdP must offer a password login form (Dex, Keycloak with
direct grants) and register `<UAT_SERVER>/callback` for the `kairos-web`
client. IdPs without a password form (Google Workspace) are not supported
by this pass.

## Writing a journey

A journey is one file in `journeys/` named `<id>.journey.ts`:

```ts
journey('cross-team', 'A web engineer needs something from platform and gets it',
  { humans: ['carol', 'bob'] }, async ({ cast, ledger }) => {
    const carol = cast.human('carol');
    const code = await step(carol, 'files a task against payments-api', async () => {
      const mcp = await carol.mcp();
      const text = await mcp.call('create_item', { kind: 'task', title: named('export'), repository: 'payments-api' });
      const [code] = shortCodes(text);
      ledger.add({ kind: 'task', label: code, delete: () => carolApi.delete(`/api/tasks/${code}`) });
      return { short_code: code };
    });
  });
```

Conventions:

- **Narrate every acceptance step** through `step(persona, narration, fn)`.
  The narration is what the report prints — write it as the persona's
  action ("drags the card to Todo"), not the assertion. Return the values
  a reader needs to believe it (`{ short_code, column }`).
- **Use the persona's real surface**: `gui()` for humans in the browser,
  `cli()` for the real `kairos` binary, `mcp()` for agents, `api()` only
  where a person would script the API. Say "(API)" in the narration when a
  human persona has to fall back to it.
- **Name everything you create** with `named('…')` (`uat-<run>-…`) and
  **ledger every create** with a delete closure. Teardown runs last-in
  first-out; create parents before children.
- **Compose-only steps** go through `step.composeOnly(persona, narration,
  reason, fn)`; under `--server` they are skipped and the report says why.
- **No `data-testid`, no sleeps.** Roles and visible text first, the
  stable `.kairos-*` / `.cl-*` classes only where text is ambiguous;
  expect-polling or explicit waits only.
- **Humans in `{ humans: [...] }`** get their tokens minted before any
  browser opens — Dex keeps one refresh token per user+client.
- `retries: 0` on purpose. A journey narrates one run.

`npm run typecheck` (`tsc --noEmit`) is the static gate for this package.
