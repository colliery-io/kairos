// The CLI surface a persona holds: the REAL `kairos` binary the harness
// built, run with a private KAIROS_CONFIG_DIR so personas never share a
// credential cache. "Logging in" writes the credential store the way
// `kairos login` does (crates/kairos-cli/src/credentials.rs) with the
// token already minted — the device grant is interactive and is not what
// UAT is about. A service-account API key is a bearer token too, so the
// agent persona gets the same file with the key as its access token.
import { execFile, spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';
import { runContext } from '../run/context';
import { recordSurface } from '../run/coverage';

const execFileAsync = promisify(execFile);

export interface CliResult {
  code: number;
  stdout: string;
  stderr: string;
}

export class CliError extends Error {
  constructor(public readonly args: string[], public readonly result: CliResult) {
    super(`kairos ${args.join(' ')} -> exit ${result.code}: ${result.stderr || result.stdout}`);
  }
}

/**
 * The environment of a persona's `kairos`. A key or a URL in the shell
 * (`KAIROS_KEY`, `KAIROS_MCP_KEY`, `KAIROS_URL`) wins over the credential
 * store, so an operator's agent key would send each command to their own
 * deployment as them. The persona has only its credential store.
 */
export function cliEnv(configDir?: string): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = { ...process.env, NO_COLOR: '1' };
  for (const name of ['KAIROS_KEY', 'KAIROS_MCP_KEY', 'KAIROS_URL', 'KAIROS_AUTO_UPDATE']) delete env[name];
  // No daily version check from a persona (KAIROS-T-0460).
  env.KAIROS_UPDATE_CHECK = '0';
  if (configDir) env.KAIROS_CONFIG_DIR = configDir;
  return env;
}

export class Cli {
  private readonly configDir: string;

  constructor(private readonly personaName: string) {
    const ctx = runContext();
    this.configDir = fs.mkdtempSync(path.join(os.tmpdir(), `kairos-uat-${ctx.run}-${personaName}-`));
  }

  /** Write the credential cache for the target with this bearer token. */
  login(token: string, opts: { refreshToken?: string } = {}): void {
    const ctx = runContext();
    const store = {
      version: 1,
      deployments: {
        [ctx.server]: {
          access_token: token,
          ...(opts.refreshToken ? { refresh_token: opts.refreshToken } : {}),
          // Far enough out that the CLI never tries to refresh a token it
          // could not refresh (no refresh_token → the cached one is used).
          expires_at: Math.floor(Date.now() / 1000) + 6 * 3600,
          issuer: ctx.issuer,
          client_id: 'kairos-web',
          tenant: ctx.tenant,
          api_bearer: 'access_token',
        },
      },
    };
    const file = path.join(this.configDir, 'credentials.json');
    fs.writeFileSync(file, JSON.stringify(store, null, 2), { mode: 0o600 });
  }

  /** Run `kairos <args>`; resolves whatever the exit code (see `ok`). */
  async run(args: string[]): Promise<CliResult> {
    recordSurface('cli', args[0]);
    const ctx = runContext();
    try {
      const { stdout, stderr } = await execFileAsync(ctx.kairosBin, args, {
        env: cliEnv(this.configDir),
        maxBuffer: 16 * 1024 * 1024,
      });
      return { code: 0, stdout, stderr };
    } catch (err: any) {
      if (typeof err.code === 'number') {
        return { code: err.code, stdout: err.stdout ?? '', stderr: err.stderr ?? '' };
      }
      throw err;
    }
  }

  /**
   * Run `kairos <args>` with `input` on stdin. Stdin closes when `done`
   * says that stdout is complete (or at once with no `done`); the result
   * comes when the process ends (`mcp-proxy` ends at the end of its
   * input). A process that does not end in `timeoutMs` is killed, and the
   * result has the code -1.
   */
  async runWithInput(
    args: string[],
    input: string,
    done?: (stdout: string) => boolean,
    timeoutMs = 30_000,
  ): Promise<CliResult> {
    recordSurface('cli', args[0]);
    const ctx = runContext();
    return await new Promise<CliResult>((resolve, reject) => {
      const child = spawn(ctx.kairosBin, args, { env: cliEnv(this.configDir) });
      let stdout = '';
      let stderr = '';
      let closed = false;
      const close = () => {
        if (!closed) {
          closed = true;
          child.stdin.end();
        }
      };
      child.stdout.on('data', (chunk) => {
        stdout += chunk;
        if (done?.(stdout)) close();
      });
      child.stderr.on('data', (chunk) => { stderr += chunk; });
      const timer = setTimeout(() => child.kill('SIGKILL'), timeoutMs);
      child.on('error', (err) => { clearTimeout(timer); reject(err); });
      child.on('close', (code) => {
        clearTimeout(timer);
        resolve({ code: code ?? -1, stdout, stderr });
      });
      child.stdin.write(input);
      if (!done) close();
    });
  }

  /**
   * The top-level nouns this binary offers, read from `kairos --help`.
   * The coverage gate asks the binary rather than hard-coding a list, so
   * a new noun shows up the day it ships. `login`/`logout`/`help` are
   * excluded: the journeys authenticate by writing the credential store
   * (the device grant is interactive), so no persona can run them.
   */
  async nouns(): Promise<string[]> {
    const { stdout } = await execFileAsync(runContext().kairosBin, ['--help'], {
      env: cliEnv(),
    });
    const block = stdout.split(/^Commands:$/m)[1] ?? '';
    const names = block
      .split('\n')
      .map((line) => line.match(/^\s{2,}([a-z][a-z-]*)\s{2,}\S/)?.[1])
      .filter((name): name is string => !!name);
    const interactive = new Set(['login', 'logout', 'help']);
    return [...new Set(names.filter((n) => !interactive.has(n)))].sort();
  }

  /** Run and require exit 0; returns stdout. */
  async ok(args: string[]): Promise<string> {
    const result = await this.run(args);
    if (result.code !== 0) throw new CliError(args, result);
    return result.stdout;
  }

  /** Run with `--json` appended and parse stdout. */
  async json(args: string[]): Promise<any> {
    const stdout = await this.ok([...args, '--json']);
    try {
      return JSON.parse(stdout);
    } catch {
      throw new Error(`kairos ${args.join(' ')} --json did not print JSON: ${stdout.slice(0, 300)}`);
    }
  }
}
