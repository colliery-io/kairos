// The MCP surface an agent persona holds: one streamable-HTTP session
// against /mcp (initialize → notifications/initialized → tools/call),
// the same wire the plugin's skills speak. Tool results come back as the
// text the skills read; small parsers below pick out the lines they key on.
import { runContext } from '../run/context';
import { recordSurface } from '../run/coverage';

function rpcMessage(body: string): any {
  try {
    const value = JSON.parse(body);
    if (value && value.jsonrpc) return value;
  } catch {
    /* SSE framing */
  }
  for (const line of body.split('\n')) {
    if (line.startsWith('data:')) {
      try {
        const value = JSON.parse(line.slice(5).trim());
        if (value && value.jsonrpc) return value;
      } catch {
        /* keep looking */
      }
    }
  }
  throw new Error(`no JSON-RPC message in MCP response: ${body.slice(0, 300)}`);
}

export class McpToolError extends Error {
  constructor(public readonly tool: string, public readonly text: string) {
    super(`${tool}: ${text}`);
  }
}

export class McpSession {
  // The server keeps no sessions (KAIROS-T-0392): an id is sent back only
  // when a server issues one.
  private sessionId: string | undefined;
  private initialized = false;
  private nextId = 1;

  constructor(
    private readonly token: string,
    private readonly clientName: string,
    private readonly server = runContext().server,
    private readonly tenant = runContext().tenant,
  ) {}

  private async post(body: unknown): Promise<{ status: number; session: string | null; text: string }> {
    const res = await fetch(`${this.server}/mcp?profile=all`, {
      method: 'POST',
      headers: {
        authorization: `Bearer ${this.token}`,
        accept: 'application/json, text/event-stream',
        'content-type': 'application/json',
        'x-tenant': this.tenant,
        ...(this.sessionId ? { 'mcp-session-id': this.sessionId } : {}),
      },
      body: JSON.stringify(body),
    });
    return { status: res.status, session: res.headers.get('mcp-session-id'), text: await res.text() };
  }

  async initialize(): Promise<void> {
    if (this.initialized) return;
    const init = await this.post({
      jsonrpc: '2.0',
      id: 0,
      method: 'initialize',
      params: {
        protocolVersion: '2025-06-18',
        capabilities: {},
        clientInfo: { name: `kairos-uat/${this.clientName}`, version: '0.0.0' },
      },
    });
    if (init.status !== 200) throw new Error(`MCP initialize HTTP ${init.status}: ${init.text}`);
    this.sessionId = init.session ?? undefined;
    const message = rpcMessage(init.text);
    if (message.result?.serverInfo?.name !== 'kairos') {
      throw new Error(`unexpected MCP serverInfo: ${init.text}`);
    }
    const notified = await this.post({ jsonrpc: '2.0', method: 'notifications/initialized' });
    if (notified.status !== 202) throw new Error(`MCP initialized notification HTTP ${notified.status}`);
    this.initialized = true;
  }

  /**
   * Call a tool and return its text. A tool-level error (`isError`) throws
   * `McpToolError` carrying the text, so a journey can assert on a refusal.
   */
  async call(name: string, args: Record<string, unknown> = {}): Promise<string> {
    // KAIROS-T-0792 (KAIROS-A-0087): `create_item` takes no content. A
    // journey that gives one gets the create, then `update_item` of
    // version 1 with the text; the answer is the answer of the create.
    if (name === 'create_item' && 'content' in args) {
      const { content, ...rest } = args;
      const text = await this.call(name, rest);
      const code = text.match(/^Created \w+ ([A-Z][A-Z0-9]*-[A-Z]-\d{4,})/)?.[1];
      if (code && typeof content === 'string' && content.trim() !== '') {
        await this.call('update_item', { short_code: code, content, version: 1 });
      }
      return text;
    }
    recordSurface('mcp', name);
    await this.initialize();
    const res = await this.post({
      jsonrpc: '2.0',
      id: this.nextId++,
      method: 'tools/call',
      params: { name, arguments: args },
    });
    if (res.status !== 200) throw new Error(`${name} HTTP ${res.status}: ${res.text}`);
    const message = rpcMessage(res.text);
    if (message.error) throw new Error(`${name} protocol error: ${JSON.stringify(message.error)}`);
    const result = message.result ?? {};
    const text = (result.content ?? [])
      .filter((c: any) => c.type === 'text')
      .map((c: any) => c.text)
      .join('\n');
    if (result.isError) throw new McpToolError(name, text);
    return text;
  }

  /**
   * The tool names this deployment offers (`tools/list`). The coverage
   * gate asks the server rather than hard-coding a list, so a tool added
   * to the product shows up here the day it ships.
   */
  async listTools(): Promise<string[]> {
    await this.initialize();
    const res = await this.post({ jsonrpc: '2.0', id: this.nextId++, method: 'tools/list' });
    if (res.status !== 200) throw new Error(`tools/list HTTP ${res.status}: ${res.text}`);
    const message = rpcMessage(res.text);
    if (message.error) throw new Error(`tools/list error: ${JSON.stringify(message.error)}`);
    return ((message.result?.tools ?? []) as any[]).map((t) => t.name as string).sort();
  }

  /** `call` that returns the refusal text instead of throwing. */
  async refused(name: string, args: Record<string, unknown> = {}): Promise<string> {
    try {
      const text = await this.call(name, args);
      throw new Error(`${name} was expected to be refused but returned: ${text.slice(0, 200)}`);
    } catch (err) {
      if (err instanceof McpToolError) return err.text;
      throw err;
    }
  }
}

/** Short codes like DEMO-T-0042 found in tool text, in order, de-duplicated. */
export function shortCodes(text: string): string[] {
  return [...new Set(text.match(/\b[A-Z][A-Z0-9]*-[A-Z]-\d{4}\b/g) ?? [])];
}

/** The `key: value` line a tool prints (e.g. `repository: payments-api (owner: platform)`). */
export function field(text: string, key: string): string | undefined {
  const re = new RegExp(`^\\s*${key}:\\s*(.+)$`, 'm');
  return text.match(re)?.[1]?.trim();
}
