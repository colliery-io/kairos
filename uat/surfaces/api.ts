// The REST surface a persona holds: a bearer client over the public API.
// Thin on purpose — journeys read like the persona's actions, and the
// typed helpers below are the handful every journey needs. Task-specific
// calls live next to the journey that makes them.
import { runContext } from '../run/context';
import { readEachBoardPage } from './board-items';

export class ApiError extends Error {
  constructor(
    public readonly method: string,
    public readonly path: string,
    public readonly status: number,
    public readonly body: string,
  ) {
    super(`${method} ${path} -> ${status}: ${body}`);
  }
}

export class Api {
  constructor(
    private readonly token: string,
    private readonly server = runContext().server,
    private readonly tenant = runContext().tenant,
  ) {}

  /** Raw request; returns status + parsed body (or text). Never throws on HTTP status. */
  async raw(method: string, path: string, body?: unknown): Promise<{ status: number; body: any }> {
    const res = await fetch(this.server + path, {
      method,
      headers: {
        authorization: `Bearer ${this.token}`,
        'x-tenant': this.tenant,
        ...(body === undefined ? {} : { 'content-type': 'application/json' }),
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const text = await res.text();
    let parsed: any = text;
    try {
      parsed = text ? JSON.parse(text) : null;
    } catch {
      /* non-JSON body stays text */
    }
    return { status: res.status, body: parsed };
  }

  private async ok(method: string, path: string, body?: unknown): Promise<any> {
    const res = await this.raw(method, path, body);
    if (res.status < 200 || res.status >= 300) {
      throw new ApiError(method, path, res.status, typeof res.body === 'string' ? res.body : JSON.stringify(res.body));
    }
    return res.body;
  }

  get(path: string) { return this.ok('GET', path); }
  post(path: string, body?: unknown) { return this.ok('POST', path, body); }
  put(path: string, body?: unknown) { return this.ok('PUT', path, body); }
  patch(path: string, body?: unknown) { return this.ok('PATCH', path, body); }
  delete(path: string) { return this.ok('DELETE', path); }

  whoami() { return this.get('/api/whoami'); }

  /** Boards list (first 100). */
  async boards(): Promise<any[]> {
    return (await this.get('/api/boards?limit=100')).items;
  }
  /** One board by its slug: the read routes take a slug (COLLIERY-T-0265). */
  boardBySlug(slug: string): Promise<any> {
    return this.get(`/api/boards/${encodeURIComponent(slug)}`);
  }
  /**
   * Each item of a board, page after page (COLLIERY-T-0265). The route
   * gives 200 items by default, so one read is a part of a large board.
   * `board` is the slug or the id. `query` has the filters of the route,
   * without `limit` and `offset`.
   */
  boardItems(board: string, query: Record<string, string> = {}): Promise<any> {
    return readEachBoardPage((limit, offset) => {
      const params = new URLSearchParams({ ...query, limit: String(limit), offset: String(offset) });
      return this.get(`/api/boards/${encodeURIComponent(board)}/items?${params}`);
    });
  }
  teamBySlug(slug: string) { return this.get(`/api/teams/by-slug/${slug}`); }
  task(code: string) { return this.get(`/api/tasks/${code}`); }
}
