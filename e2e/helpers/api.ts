// Thin REST helpers for the "second writer" the smoke test needs (KAIROS-T-0045):
// the WS live-update step and the 409 conflict step both mutate through the API
// with a separately minted token while the browser watches. Endpoints per
// S-0005 / the kairos-web data layer (POST /api/{family}/{code}/transition,
// PATCH /api/{family}/{code} carrying {title,content,version}).

import crypto from 'node:crypto';

const bearer = (token: string) => ({ authorization: `Bearer ${token}` });

async function json(server: string, token: string, path: string): Promise<any> {
  const res = await fetch(server + path, { headers: bearer(token) });
  if (!res.ok) throw new Error(`GET ${path} -> ${res.status}: ${await res.text()}`);
  return res.json();
}

export interface BoardSnapshot {
  boardId: string;
  columnName: Map<string, string>; // column id -> name
  transitions: { from: string; to: string }[];
  tasksByColumnName: Map<string, { code: string; columnId: string }[]>;
}

/** Load platform-delivery's config + grouped items in one shot. */
export async function loadPlatformDelivery(
  server: string,
  token: string,
): Promise<BoardSnapshot> {
  const boards = (await json(server, token, '/api/boards?limit=100')).items as any[];
  const pd = boards.find((b) => b.slug === 'platform-delivery');
  if (!pd) throw new Error('platform-delivery board not found');
  const detail = await json(server, token, `/api/boards/${pd.id}`);
  const items = await json(server, token, `/api/boards/${pd.id}/items`);

  const columnName = new Map<string, string>();
  for (const c of detail.columns) columnName.set(c.id, c.name);
  const transitions = detail.transitions.map((t: any) => ({
    from: t.from_column_id,
    to: t.to_column_id,
  }));
  const tasksByColumnName = new Map<string, { code: string; columnId: string }[]>();
  for (const group of items.columns) {
    tasksByColumnName.set(
      group.column.name,
      group.tasks.map((t: any) => ({ code: t.short_code, columnId: group.column.id })),
    );
  }
  return { boardId: pd.id, columnName, transitions, tasksByColumnName };
}

export interface MovePick {
  code: string;
  toColumnId: string;
  toColumnName: string;
}

/**
 * Pick a task with at least one valid outgoing transition, so the WS step is
 * retry-safe regardless of the board's current state. Preference order keeps
 * the choice stable on a fresh seed (a Todo → Active move) while still working
 * on a mutated board (e.g. a Playwright retry).
 */
export async function pickMovableTask(
  server: string,
  token: string,
  exclude: string[] = [],
): Promise<MovePick> {
  const board = await loadPlatformDelivery(server, token);
  const nameById = board.columnName;
  const order = ['Todo', 'Backlog', 'Blocked', 'Active'];
  for (const colName of order) {
    const tasks = board.tasksByColumnName.get(colName) ?? [];
    for (const task of tasks) {
      if (exclude.includes(task.code)) continue;
      const transition = board.transitions.find((t) => t.from === task.columnId);
      if (transition) {
        return {
          code: task.code,
          toColumnId: transition.to,
          toColumnName: nameById.get(transition.to)!,
        };
      }
    }
  }
  throw new Error('no movable task found on platform-delivery');
}

/** POST /api/tasks/{code}/transition. */
export async function transitionTask(
  server: string,
  token: string,
  code: string,
  toColumnId: string,
): Promise<void> {
  const res = await fetch(`${server}/api/tasks/${code}/transition`, {
    method: 'POST',
    headers: { ...bearer(token), 'content-type': 'application/json' },
    body: JSON.stringify({ to_column_id: toColumnId }),
  });
  if (!res.ok) {
    throw new Error(`transition ${code} -> ${res.status}: ${await res.text()}`);
  }
}

/**
 * `POST /api/tasks/{code}/move` — re-home a task onto another delivery
 * board (KAIROS-I-0012). The competing-writer half of the live-update
 * assertions: the board a card LEAVES and the board it JOINS each get an
 * `item_moved` event, so a watching page refetches without a reload.
 */
export async function moveTask(
  server: string,
  token: string,
  code: string,
  board: string,
): Promise<any> {
  const res = await fetch(`${server}/api/tasks/${code}/move`, {
    method: 'POST',
    headers: { ...bearer(token), 'content-type': 'application/json' },
    body: JSON.stringify({ board }),
  });
  if (!res.ok) {
    throw new Error(`move ${code} -> ${res.status}: ${await res.text()}`);
  }
  return res.json();
}

export interface TaskState {
  version: number;
  title: string;
  content: string;
}

/** GET /api/tasks/{code} (the fields the merge step reads). */
export async function getTask(
  server: string,
  token: string,
  code: string,
): Promise<TaskState> {
  const t = await json(server, token, `/api/tasks/${code}`);
  return { version: t.version, title: t.title, content: t.content };
}

/**
 * PATCH /api/tasks/{code} — the competing write that forces the browser's next
 * save to 409. Returns the new version.
 */
export async function patchTask(
  server: string,
  token: string,
  code: string,
  body: { title: string; content: string; version: number },
): Promise<number> {
  const res = await fetch(`${server}/api/tasks/${code}`, {
    method: 'PATCH',
    headers: { ...bearer(token), 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    throw new Error(`patch ${code} -> ${res.status}: ${await res.text()}`);
  }
  return (await res.json()).version as number;
}

// --- team pages (KAIROS-T-0087) ---------------------------------------------

export interface TeamPageState {
  id: string;
  teamId: string;
  slug: string;
  title: string;
  content: string;
  version: number;
}

/** Resolve a team page by slug path (e.g. 'documentation/how-to-guides/deploy-kairos'). */
export async function getTeamPage(
  server: string,
  token: string,
  teamSlug: string,
  path: string,
): Promise<TeamPageState> {
  const team = await json(server, token, `/api/teams/by-slug/${teamSlug}`);
  const pages = (await json(server, token, `/api/teams/${team.id}/pages`)) as any[];
  let parent: string | null = null;
  let node: any = null;
  for (const segment of path.split('/')) {
    node = pages.find((p) => p.parent_id === parent && p.slug === segment);
    if (!node) throw new Error(`no team page at ${path} (stuck on ${segment})`);
    parent = node.id;
  }
  return {
    id: node.id,
    teamId: team.id,
    slug: node.slug,
    title: node.title,
    content: node.content,
    version: node.version,
  };
}

/** PATCH a team page's content — the competing write for the 409 walk. */
export async function patchTeamPage(
  server: string,
  token: string,
  teamId: string,
  pageId: string,
  body: { title: string; content: string; version: number },
): Promise<number> {
  const res = await fetch(`${server}/api/teams/${teamId}/pages/${pageId}`, {
    method: 'PATCH',
    headers: { ...bearer(token), 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    throw new Error(`patch team page -> ${res.status}: ${await res.text()}`);
  }
  return (await res.json()).version as number;
}

// --- forge webhooks (KAIROS-T-0102) -----------------------------------------

export interface ForgeConnection {
  id: string;
  webhookUrl: string;
  webhookSecret: string;
}

/** A registered repository (KAIROS-A-0019), as /api/repositories returns it. */
export interface Repository {
  id: string;
  slug: string;
  teamSlug: string;
  deliveryBoardId: string | null;
  openTasks: number;
  hasWebhook: boolean;
}

function toRepository(body: any): Repository {
  return {
    id: body.id,
    slug: body.slug,
    teamSlug: body.team.slug,
    deliveryBoardId: body.delivery_board_id ?? null,
    openTasks: body.open_tasks,
    hasWebhook: body.has_webhook,
  };
}

/**
 * Register a repository under its owning team (org admin, or a member of
 * that team) — KAIROS-T-0106. `team` is a slug or UUID.
 */
export async function createRepository(
  server: string,
  token: string,
  opts: { slug?: string; forge?: string; repoFullName: string; team: string; description?: string },
): Promise<Repository> {
  const res = await fetch(`${server}/api/repositories`, {
    method: 'POST',
    headers: { ...bearer(token), 'content-type': 'application/json' },
    body: JSON.stringify({
      slug: opts.slug ?? null,
      forge: opts.forge ?? 'github',
      repo_full_name: opts.repoFullName,
      repo_url: `https://github.com/${opts.repoFullName}`,
      team: opts.team,
      description: opts.description ?? null,
    }),
  });
  if (!res.ok) {
    throw new Error(`create repository -> ${res.status}: ${await res.text()}`);
  }
  return toRepository(await res.json());
}

/** The repository directory, optionally one team's. */
export async function listRepositories(
  server: string,
  token: string,
  team?: string,
): Promise<Repository[]> {
  const query = team ? `?team=${encodeURIComponent(team)}` : '';
  const res = await fetch(`${server}/api/repositories${query}`, { headers: bearer(token) });
  if (!res.ok) {
    throw new Error(`list repositories -> ${res.status}: ${await res.text()}`);
  }
  return ((await res.json()) as any[]).map(toRepository);
}

/**
 * Create a task over the API. `boardId` (slug or UUID) chooses the board;
 * `repository` is an optional link to any live repository and does not
 * choose one (COLLIERY-T-0217, COLLIERY-A-0023). A request with no board is
 * a 422. Until COLLIERY-T-0217 a repository alone routed the task to its
 * owning team's delivery board (KAIROS-T-0104).
 * Returns the raw task DTO.
 */
export async function createTask(
  server: string,
  token: string,
  opts: { title: string; boardId?: string; repository?: string; content?: string },
): Promise<any> {
  const res = await fetch(`${server}/api/tasks`, {
    method: 'POST',
    headers: { ...bearer(token), 'content-type': 'application/json' },
    body: JSON.stringify({
      board_id: opts.boardId ?? null,
      repository: opts.repository ?? null,
      title: opts.title,
      content: opts.content ?? '',
    }),
  });
  if (!res.ok) {
    throw new Error(`create task -> ${res.status}: ${await res.text()}`);
  }
  return res.json();
}

/** Raw status of a task transition — for negative assertions. */
export async function tryTransitionTask(
  server: string,
  token: string,
  shortCode: string,
  toColumnId: string,
): Promise<number> {
  const res = await fetch(`${server}/api/tasks/${shortCode}/transition`, {
    method: 'POST',
    headers: { ...bearer(token), 'content-type': 'application/json' },
    body: JSON.stringify({ to_column_id: toColumnId }),
  });
  return res.status;
}

/**
 * Connect webhooks for a REGISTERED repository (slug or UUID) and capture
 * its delivery URL + secret (shown once) — the KAIROS-T-0106 re-key.
 */
export async function createForgeConnection(
  server: string,
  token: string,
  repository: string,
): Promise<ForgeConnection> {
  const res = await fetch(`${server}/api/forge-connections`, {
    method: 'POST',
    headers: { ...bearer(token), 'content-type': 'application/json' },
    body: JSON.stringify({ repository }),
  });
  if (!res.ok) {
    throw new Error(`create connection -> ${res.status}: ${await res.text()}`);
  }
  const body = await res.json();
  return {
    id: body.id,
    webhookUrl: body.webhook_url,
    webhookSecret: body.webhook_secret,
  };
}

/**
 * POST a GitHub webhook payload with a correct `X-Hub-Signature-256`:
 * HMAC-SHA256 of the RAW body under the connection's secret — which is
 * exactly why the secret is returned at creation.
 *
 * `signWith` overrides the secret so a test can prove a bad signature is
 * rejected. Returns the HTTP status.
 */
export async function deliverGithubWebhook(
  server: string,
  connection: ForgeConnection,
  event: string,
  payload: unknown,
  signWith?: string,
): Promise<number> {
  const body = JSON.stringify(payload);
  const secret = signWith ?? connection.webhookSecret;
  const signature =
    'sha256=' + crypto.createHmac('sha256', secret).update(body).digest('hex');
  // webhook_url names the deployment's PUBLIC url, which in the e2e stack
  // is not where the test server listens — deliver to the same path on the
  // local server.
  const path = new URL(connection.webhookUrl).pathname;
  const res = await fetch(`${server}${path}`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      'x-github-event': event,
      'x-hub-signature-256': signature,
    },
    body,
  });
  return res.status;
}

/** A GitHub `pull_request` payload naming `code`. */
export function githubPullRequest(opts: {
  number: number;
  code: string;
  repoFullName: string;
  state: 'open' | 'closed';
  merged?: boolean;
  draft?: boolean;
  updatedAt: string;
  title?: string;
}): unknown {
  return {
    action: opts.state === 'closed' ? 'closed' : 'opened',
    repository: {
      full_name: opts.repoFullName,
      html_url: `https://github.com/${opts.repoFullName}`,
    },
    sender: { login: 'dylan' },
    pull_request: {
      number: opts.number,
      state: opts.state,
      merged: opts.merged ?? false,
      draft: opts.draft ?? false,
      title: opts.title ?? `Work on ${opts.code}`,
      body: '',
      html_url: `https://github.com/${opts.repoFullName}/pull/${opts.number}`,
      updated_at: opts.updatedAt,
      user: { login: 'dylan' },
      head: { ref: `dylan/${opts.code}-branch` },
    },
  };
}

/**
 * Raw status of `POST /api/relationships` — the cross-team coordination
 * edge. The link rule (COLLIERY-T-0228): a caller who may edit the item at
 * one end may write the edge, for each relationship type.
 */
export async function tryCreateRelationship(
  server: string,
  token: string,
  edge: { source: string; target: string; relationship: string },
): Promise<number> {
  const res = await fetch(`${server}/api/relationships`, {
    method: 'POST',
    headers: { ...bearer(token), 'content-type': 'application/json' },
    body: JSON.stringify({
      source_short_code: edge.source,
      target_short_code: edge.target,
      relationship: edge.relationship,
    }),
  });
  return res.status;
}

/**
 * `GET /api/tasks/{code}`, the raw task DTO (COLLIERY-T-0232). The request
 * spec reads `column_id` and `work_class` from it, to prove what the server
 * stored and not only what the board shows.
 */
export async function readTask(server: string, token: string, code: string): Promise<any> {
  return json(server, token, `/api/tasks/${code}`);
}

/** Raw status of `POST /api/tasks/{code}/work-class`, for a refusal. */
export async function trySetWorkClass(
  server: string,
  token: string,
  shortCode: string,
  workClass: 'planned' | 'support',
): Promise<number> {
  const res = await fetch(`${server}/api/tasks/${shortCode}/work-class`, {
    method: 'POST',
    headers: { ...bearer(token), 'content-type': 'application/json' },
    body: JSON.stringify({ work_class: workClass }),
  });
  return res.status;
}

/** A board of the tenant, with its columns in position order. */
export interface BoardColumns {
  id: string;
  slug: string;
  level: string;
  /** The column with the lowest position: where a new item starts. */
  entryColumnId: string;
  entryColumnName: string;
  columnId: Map<string, string>; // column name -> id
}

/**
 * The first live board that `pick` accepts, with its entry column
 * (COLLIERY-T-0232). The entry column is the column with the lowest
 * position, which is the rule of the server (`boards::entry_column`).
 */
export async function loadBoard(
  server: string,
  token: string,
  pick: (board: any) => boolean,
): Promise<BoardColumns> {
  const boards = (await json(server, token, '/api/boards?limit=100')).items as any[];
  const board = boards.find(pick);
  if (!board) throw new Error('no board matches');
  const detail = await json(server, token, `/api/boards/${board.id}`);
  const columns = [...detail.columns].sort((a: any, b: any) => a.position - b.position);
  if (columns.length === 0) throw new Error(`board ${board.slug} has no column`);
  return {
    id: board.id,
    slug: board.slug,
    level: board.board_level,
    entryColumnId: columns[0].id,
    entryColumnName: columns[0].name,
    columnId: new Map(columns.map((c: any) => [c.name, c.id])),
  };
}

/**
 * Raw status of `DELETE /api/tasks/{code}`: archive a task. The creator of
 * a task can archive it (COLLIERY-T-0228).
 */
export async function tryArchiveTask(
  server: string,
  token: string,
  shortCode: string,
): Promise<number> {
  const res = await fetch(`${server}/api/tasks/${shortCode}`, {
    method: 'DELETE',
    headers: bearer(token),
  });
  return res.status;
}
