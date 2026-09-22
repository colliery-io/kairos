// A persona is a person (or an agent) with the surfaces they really use.
// `Cast` creates them per journey and owns their browser contexts; every
// surface is lazy so a persona that only ever uses the CLI never opens a
// browser, and every credential is minted at most once per journey.
import type { Browser, BrowserContext, Page } from '@playwright/test';
import path from 'node:path';
import { runContext } from '../run/context';
import { Api } from '../surfaces/api';
import { mintToken } from '../surfaces/auth';
import { Cli } from '../surfaces/cli';
import { login } from '../surfaces/gui';
import { McpSession } from '../surfaces/mcp';
import { credentialsFor, ROLES, type Credentials, type Human, type PersonaName } from './credentials';

export class Persona {
  private tokenPromise: Promise<string> | undefined;
  private apiInstance: Api | undefined;
  private cliInstance: Cli | undefined;
  private mcpInstance: McpSession | undefined;
  private context: BrowserContext | undefined;
  private pageInstance: Page | undefined;

  constructor(
    readonly name: PersonaName,
    readonly role: string,
    private readonly browser: Browser,
    /** Human credentials, or an API key for a service account. */
    private readonly identity: { credentials: Credentials } | { apiKey: string },
  ) {}

  get isHuman(): boolean {
    return 'credentials' in this.identity;
  }

  get credentials(): Credentials {
    if (!('credentials' in this.identity)) throw new Error(`${this.name} is a service account`);
    return this.identity.credentials;
  }

  /** The bearer token: a minted OAuth token for humans, the key for agents. */
  token(): Promise<string> {
    if (!this.tokenPromise) {
      const ctx = runContext();
      this.tokenPromise =
        'apiKey' in this.identity
          ? Promise.resolve(this.identity.apiKey)
          : mintToken({ issuer: ctx.issuer, server: ctx.server, ...this.identity.credentials });
    }
    return this.tokenPromise;
  }

  async api(): Promise<Api> {
    if (!this.apiInstance) this.apiInstance = new Api(await this.token());
    return this.apiInstance;
  }

  async cli(): Promise<Cli> {
    if (!this.cliInstance) {
      const cli = new Cli(this.name);
      cli.login(await this.token());
      this.cliInstance = cli;
    }
    return this.cliInstance;
  }

  async mcp(): Promise<McpSession> {
    if (!this.mcpInstance) this.mcpInstance = new McpSession(await this.token(), this.name);
    return this.mcpInstance;
  }

  /** A logged-in page in this persona's own browser context (one per persona). */
  async gui(): Promise<Page> {
    if (!this.pageInstance) {
      const ctx = runContext();
      this.context = await this.browser.newContext({ baseURL: ctx.server });
      await this.context.tracing.start({ screenshots: true, snapshots: true });
      this.pageInstance = await this.context.newPage();
      await login(this.pageInstance, this.credentials);
    }
    return this.pageInstance;
  }

  /** The open page, if the persona has one (for failure screenshots). */
  get openPage(): Page | undefined {
    return this.pageInstance;
  }

  async close(keepTraceAs?: string): Promise<void> {
    if (this.context) {
      await this.context.tracing.stop(keepTraceAs ? { path: keepTraceAs } : undefined);
      await this.context.close();
      this.context = undefined;
      this.pageInstance = undefined;
    }
  }
}

export class Cast {
  private readonly members = new Map<string, Persona>();

  constructor(private readonly browser: Browser) {}

  /** A human persona from the configured credentials. */
  human(name: Human): Persona {
    const existing = this.members.get(name);
    if (existing) return existing;
    const creds = credentialsFor(name);
    if (!creds) throw new Error(`no credentials configured for persona ${name}`);
    const persona = new Persona(name, ROLES[name], this.browser, { credentials: creds });
    this.members.set(name, persona);
    return persona;
  }

  /** Whether a real human is configured for the name (else journeys substitute). */
  hasHuman(name: Human): boolean {
    return credentialsFor(name) !== null;
  }

  /** A service-account persona from an API key a journey minted. */
  agent(name: PersonaName, apiKey: string): Persona {
    const persona = new Persona(name, ROLES[name] ?? 'service account', this.browser, { apiKey });
    this.members.set(name, persona);
    return persona;
  }

  /**
   * Mint every human token up front. Dex keeps one refresh token per
   * user+client, so headless mints must precede browser logins.
   */
  async prepare(humans: Human[]): Promise<void> {
    for (const name of humans) await this.human(name).token();
  }

  all(): Persona[] {
    return [...this.members.values()];
  }

  /** Close every context; on failure keep each persona's trace in the report dir. */
  async closeAll(failedJourneyId?: string): Promise<string[]> {
    const traces: string[] = [];
    for (const persona of this.members.values()) {
      if (!persona.openPage) continue;
      let keep: string | undefined;
      if (failedJourneyId) {
        keep = path.join(runContext().reportDir, `${failedJourneyId}-${persona.name}.trace.zip`);
        traces.push(keep);
      }
      await persona.close(keep);
    }
    return traces;
  }
}
