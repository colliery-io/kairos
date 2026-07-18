// Thin REST helpers for the "second writer" the smoke test needs (KAIROS-T-0045):
// the WS live-update step and the 409 conflict step both mutate through the API
// with a separately minted token while the browser watches. Endpoints per
// S-0005 / the kairos-web data layer (POST /api/{family}/{code}/transition,
// PATCH /api/{family}/{code} carrying {title,content,version}).

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
