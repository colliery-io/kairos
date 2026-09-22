// The run context every journey and the reporter share: where the target
// is, which mode we run in, the run id that namespaces everything a
// journey creates, and where the report lands. Read once from the
// environment the `angreal test uat` task (or a hand-run) sets.
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export type Mode = 'compose' | 'server';

export interface RunContext {
  /** Base URL of the Kairos under test (GUI + API + MCP on one origin). */
  server: string;
  /** OIDC issuer the headless PKCE walk logs personas in against. */
  issuer: string;
  /** Tenant slug sent as `X-Tenant` on API/MCP calls. */
  tenant: string;
  /** `compose` (fresh seed, compose-only steps run) or `server`. */
  mode: Mode;
  /** base36 timestamp; every created slug/title carries `uat-<run>-`. */
  run: string;
  /** Absolute report directory for this run. */
  reportDir: string;
  /** Path to the built `kairos` CLI binary. */
  kairosBin: string;
  headed: boolean;
}

const HERE = path.dirname(fileURLToPath(import.meta.url));
const UAT_ROOT = path.resolve(HERE, '..');
const REPO_ROOT = path.resolve(UAT_ROOT, '..');

let cached: RunContext | undefined;

export function runContext(): RunContext {
  if (cached) return cached;
  // The run id is minted once per process tree: the config, the workers
  // and the reporter must all agree, so it is pinned into the environment
  // the first time anyone asks.
  const run = process.env.UAT_RUN ?? Date.now().toString(36);
  process.env.UAT_RUN = run;
  const mode = (process.env.UAT_MODE as Mode | undefined) ?? 'compose';
  if (mode !== 'compose' && mode !== 'server') {
    throw new Error(`UAT_MODE must be compose|server, got ${mode}`);
  }
  cached = {
    server: (process.env.UAT_SERVER ?? 'http://localhost:41080').replace(/\/+$/, ''),
    issuer: process.env.UAT_ISSUER ?? 'http://localhost:41558/dex',
    tenant: process.env.UAT_TENANT ?? 'demo',
    mode,
    run,
    reportDir: path.resolve(process.env.UAT_REPORT_DIR ?? path.join(UAT_ROOT, 'reports', run)),
    kairosBin: process.env.UAT_KAIROS_BIN ?? path.join(REPO_ROOT, 'target', 'debug', 'kairos'),
    headed: process.env.UAT_HEADED === '1',
  };
  return cached;
}

/** `uat-<run>-<suffix>`: the namespace every created object carries. */
export function named(suffix: string): string {
  return `uat-${runContext().run}-${suffix}`;
}
