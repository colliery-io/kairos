// The CLI surface a persona holds: the REAL `kairos` binary the harness
// built, run with a private KAIROS_CONFIG_DIR so personas never share a
// credential cache. "Logging in" writes the credential store the way
// `kairos login` does (crates/kairos-cli/src/credentials.rs) with the
// token already minted — the device grant is interactive and is not what
// UAT is about. A service-account API key is a bearer token too, so the
// agent persona gets the same file with the key as its access token.
import { execFile } from 'node:child_process';
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
        env: { ...process.env, KAIROS_CONFIG_DIR: this.configDir, NO_COLOR: '1' },
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
   * The top-level nouns this binary offers, read from `kairos --help`.
   * The coverage gate asks the binary rather than hard-coding a list, so
   * a new noun shows up the day it ships. `login`/`logout`/`help` are
   * excluded: the journeys authenticate by writing the credential store
   * (the device grant is interactive), so no persona can run them.
   */
  async nouns(): Promise<string[]> {
    const { stdout } = await execFileAsync(runContext().kairosBin, ['--help'], {
      env: { ...process.env, NO_COLOR: '1' },
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
