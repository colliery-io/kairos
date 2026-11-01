// A fake of the GitHub REST API for the e2e stack (KAIROS-T-0744).
//
// The e2e server reads KAIROS_GITHUB_API_URL at boot, and `angreal test
// e2e` points it at http://127.0.0.1:41091 (FAKE_GITHUB_PORT). A spec that
// needs GitHub starts the fake on that port with `startFakeGithub()` and
// stops it at the end. The server calls the fake only when a spec asks for
// a write-back or an import, so a server that boots before the fake is no
// problem.
//
// The fake answers the calls of the write-back of a pull request and of an
// issue (crates/kairos-server/src/forge/writeback.rs, issue_writeback.rs)
// and of the import of open issues (forge/import.rs). It keeps each
// request, so a spec can check what the server sent. A request with the
// token REFUSED_TOKEN gets 401, as GitHub answers a bad token.

import http from 'node:http';

export const FAKE_GITHUB_PORT = Number(process.env.E2E_FAKE_GITHUB_PORT ?? '41091');
export const REFUSED_TOKEN = 'refused-token';

export interface FakeRequest {
  method: string;
  path: string;
  token: string;
  body: any;
}

export interface FakeIssue {
  number: number;
  title: string;
  body?: string;
  login?: string;
}

export interface FakeGithub {
  url: string;
  requests: FakeRequest[];
  /** The open issues of `owner/repo` that the import reads. */
  setIssues(repo: string, issues: FakeIssue[]): void;
  close(): Promise<void>;
}

function send(res: http.ServerResponse, status: number, body: unknown): void {
  res.writeHead(status, { 'content-type': 'application/json' });
  res.end(JSON.stringify(body));
}

export async function startFakeGithub(port = FAKE_GITHUB_PORT): Promise<FakeGithub> {
  const requests: FakeRequest[] = [];
  const issues = new Map<string, FakeIssue[]>();
  let nextComment = 1000;

  const server = http.createServer((req, res) => {
    let raw = '';
    req.on('data', (chunk) => (raw += chunk));
    req.on('end', () => {
      const url = new URL(req.url ?? '/', 'http://fake');
      const token = (req.headers.authorization ?? '').replace(/^Bearer\s+/i, '');
      let body: any = null;
      try {
        body = raw ? JSON.parse(raw) : null;
      } catch {
        body = raw;
      }
      requests.push({ method: req.method ?? 'GET', path: url.pathname, token, body });
      if (token === REFUSED_TOKEN || token === '') {
        send(res, 401, { message: 'Bad credentials' });
        return;
      }
      const parts = url.pathname.split('/').filter(Boolean);
      // /repos/{owner}/{repo}/...
      if (parts[0] !== 'repos' || parts.length < 3) {
        send(res, 404, { message: 'Not Found' });
        return;
      }
      const repo = `${parts[1]}/${parts[2]}`;
      const rest = parts.slice(3);
      const method = req.method ?? 'GET';
      if (method === 'GET' && rest.length === 1 && rest[0] === 'issues') {
        const page = Number(url.searchParams.get('page') ?? '1');
        const list = page === 1 ? (issues.get(repo) ?? []) : [];
        send(
          res,
          200,
          list.map((issue) => ({
            id: 900000 + issue.number,
            number: issue.number,
            title: issue.title,
            body: issue.body ?? '',
            html_url: `https://github.com/${repo}/issues/${issue.number}`,
            user: { login: issue.login ?? 'robin' },
            created_at: '2026-10-01T09:00:00Z',
          })),
        );
        return;
      }
      if (method === 'POST' && rest.length === 3 && rest[0] === 'issues' && rest[2] === 'comments') {
        send(res, 201, { id: nextComment++ });
        return;
      }
      if (method === 'PATCH' && rest.length === 3 && rest[0] === 'issues' && rest[1] === 'comments') {
        send(res, 200, { id: Number(rest[2]) });
        return;
      }
      if (method === 'PATCH' && rest.length === 2 && rest[0] === 'issues') {
        send(res, 200, { number: Number(rest[1]), state: body?.state ?? 'open' });
        return;
      }
      if (method === 'GET' && rest.length === 2 && rest[0] === 'pulls') {
        send(res, 200, { number: Number(rest[1]), head: { sha: 'a'.repeat(40) } });
        return;
      }
      if (method === 'POST' && rest.length === 2 && rest[0] === 'statuses') {
        send(res, 201, { state: body?.state ?? 'pending' });
        return;
      }
      if (method === 'GET' && rest.length === 0) {
        send(res, 200, { full_name: repo, permissions: { pull: true } });
        return;
      }
      send(res, 404, { message: 'Not Found' });
    });
  });

  await new Promise<void>((resolve, reject) => {
    server.once('error', reject);
    server.listen(port, '127.0.0.1', () => resolve());
  });

  return {
    url: `http://127.0.0.1:${port}`,
    requests,
    setIssues: (repo, list) => issues.set(repo, list),
    close: () => new Promise<void>((resolve) => server.close(() => resolve())),
  };
}
