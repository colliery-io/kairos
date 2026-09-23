// KAIROS-T-0110 — the KAIROS-I-0010 repository-scoped work smoke
// (decision KAIROS-A-0019): boards and streams plan the work; a ticket is
// issued against ONE repository and executed inside it.
//
// Flow (login as alice, org admin; carol is a web-team member with no
// platform membership — the cross-team filer):
//   1. the seeded repositories show on the platform team page
//   2. the delivery board carries repo chips, the Repository lens narrows
//      the cards and lives in the URL, "Group by repository" re-lanes
//   3. cross-team: carol files a task against platform's repo over the
//      API with no board → it lands in platform's Backlog; carol cannot
//      transition it; bob (a platform member, NOT an admin) can — the team
//      gate, not the admin bypass; carol links her own web task to it with
//      a `blocks` edge (collaborative relationship); the card shows the chip
//   4. the item page's repository picker re-homes a task within the team,
//      and its Board select moves a task to the OTHER delivery board
//      (KAIROS-I-0012): the repo-bound task is refused inline, an unbound
//      one lands in the target's entry column, and both boards react to
//      the `item_moved` events live
//   5. a PR opened in the repo naming the cross-team task links back to it
//      (forge webhook against a freshly connected repo)
//   6. the admin Repositories page registers a repo and connects its
//      webhook, showing the secret once
//
// Conventions match the other specs: visible-text/role selectors plus the
// stable `.kairos-*`/`.cl-*` classes and data-testid hooks, in-app
// navigation only. Everything the test REGISTERS carries a per-run suffix
// so a retry (or a stack that was not re-seeded) never trips the unique
// slug / (forge, full name) indexes.

import { test, expect, type Page } from '@playwright/test';
import { mintToken } from '../helpers/auth';
import {
  createForgeConnection,
  createRepository,
  createTask,
  deliverGithubWebhook,
  githubPullRequest,
  listRepositories,
  loadPlatformDelivery,
  moveTask,
  tryCreateRelationship,
  tryTransitionTask,
} from '../helpers/api';

const GUI = process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
// Per-run suffix for everything the test registers (see the header).
const RUN = Date.now().toString(36);

const panel = (page: Page, title: string) =>
  page.locator('.cl-panel', {
    has: page.locator('.cl-panel__title', { hasText: title }),
  });

async function login(page: Page, email: string, password: string) {
  await page.goto('/');
  await page.waitForSelector('#login', { timeout: 30_000 });
  await page.fill('#login', email);
  await page.fill('#password', password);
  await page.click('#submit-login');
  await page.waitForURL((url) => url.pathname.startsWith('/boards'), {
    timeout: 30_000,
  });
}

test('repositories: team panel → board lens → cross-team filing → picker → PR link-back → admin', async ({
  page,
}) => {
  // API tokens FIRST: Dex keeps one refresh token per user+client, so a
  // headless mint for alice after the browser login would invalidate the
  // browser's silent-restore token and full navigations would bounce to Dex.
  const alice = await mintToken({ server: GUI, email: 'alice@kairos.test' });
  const bob = await mintToken({
    server: GUI,
    email: 'bob@kairos.test',
    password: 'bob-password',
  });
  const carol = await mintToken({
    server: GUI,
    email: 'carol@kairos.test',
    password: 'carol-password',
  });

  await test.step('login via Dex as alice', async () => {
    await login(page, 'alice@kairos.test', 'alice-password');
  });

  // 1. The seed's repositories on the team page ------------------------------
  await test.step('platform team page lists its repositories', async () => {
    await page.goto('/teams/platform');
    const repos = panel(page, 'Repositories');
    await expect(repos).toBeVisible();
    await expect(repos.locator('[data-repo="payments-api"]')).toBeVisible();
    await expect(repos.locator('[data-repo="platform-infra"]')).toBeVisible();
    await expect(repos.locator('[data-repo="portal-web"]')).toHaveCount(0);
    await expect(repos.locator('[data-repo="payments-api"]')).toContainText('webhooks');
    // KAIROS-T-0124 #6a: the "how to work here" blurb agents read is shown
    // to the humans on the team page too.
    await expect(
      repos.locator('[data-repo="payments-api"] .kairos-team__repo-description'),
    ).toContainText('Rust/axum service');
  });

  // 2. Board chips + lens + group-by ----------------------------------------
  await test.step('board cards carry repo chips; the lens narrows and lives in the URL', async () => {
    await page.goto('/boards/platform-delivery');
    await expect(page.locator('.kairos-card__repo[data-repo="payments-api"]').first()).toBeVisible();
    const lens = page.locator('[data-testid="repo-lens"]');
    await expect(lens).toBeVisible();
    // Two platform repos are on the board → the group-by toggle is offered.
    await expect(page.locator('[data-testid="group-by-repo"]')).toBeVisible();

    const chipsBefore = await page.locator('article.kairos-card').count();
    await lens.locator('.kairos-board__lens-chip[data-repo="platform-infra"]').click();
    await expect(page).toHaveURL(/repo=platform-infra/);
    await expect(
      lens.locator('.kairos-board__lens-chip[data-repo="platform-infra"]'),
    ).toHaveClass(/--on/);
    // Only infra-bound tasks (plus non-task cards) remain.
    await expect(page.locator('.kairos-card__repo[data-repo="payments-api"]')).toHaveCount(0);
    await expect(page.locator('.kairos-card__repo[data-repo="platform-infra"]').first()).toBeVisible();
    await expect
      .poll(() => page.locator('article.kairos-card').count())
      .toBeLessThan(chipsBefore);

    // The selection survives a reload — it is URL state.
    await page.reload();
    await expect(
      page.locator('[data-testid="repo-lens"] .kairos-board__lens-chip[data-repo="platform-infra"]'),
    ).toHaveClass(/--on/);
    await expect(page.locator('.kairos-card__repo[data-repo="payments-api"]')).toHaveCount(0);

    // Clear it, then group by repository: one lane per repo + the remainder.
    await page.locator('[data-testid="repo-lens"] .kairos-board__lens-chip[data-repo="platform-infra"]').click();
    await expect(page).not.toHaveURL(/repo=/);
    await page.locator('[data-testid="group-by-repo"]').click();
    await expect(page).toHaveURL(/by_repo=1/);
    await expect(page.locator('.kairos-board__lane--repo[data-repo-lane="payments-api"]')).toBeVisible();
    await expect(page.locator('.kairos-board__lane--repo[data-repo-lane="platform-infra"]')).toBeVisible();
    await page.locator('[data-testid="group-by-repo"]').click();
    await expect(page).not.toHaveURL(/by_repo/);
  });

  // 3. Cross-team filing ------------------------------------------------------
  let filedCode = '';
  await test.step('carol (web) files a task against platform\'s repo → platform Backlog', async () => {
    const filed = await createTask(GUI, carol, {
      title: 'Cross-team: export endpoint for the portal',
      repository: 'payments-api',
    });
    filedCode = filed.short_code;
    const board = await loadPlatformDelivery(GUI, alice);
    expect(filed.board_id).toBe(board.boardId);
    expect(board.columnName.get(filed.column_id)).toBe('Backlog');
    expect(filed.repository.slug).toBe('payments-api');

    // Carol cannot move it (file_backlog stops at the entry column); bob —
    // a platform member with no admin bypass — can: the team gate proper.
    const todo = [...board.columnName.entries()].find(([, name]) => name === 'Todo')![0];
    expect(await tryTransitionTask(GUI, carol, filedCode, todo)).toBe(403);
    expect(await tryTransitionTask(GUI, bob, filedCode, todo)).toBe(200);

    // Coordination across the seam: carol files the web-side half against
    // her own repo and links the platform task as blocking it. `blocks` is
    // collaborative (A-0019 §D6) — authoring the source is enough, no
    // platform-board power needed. A `supports` edge is not, so it is refused.
    const webSide = await createTask(GUI, carol, {
      title: 'Portal: consume the export endpoint',
      repository: 'portal-web',
    });
    expect(webSide.repository.slug).toBe('portal-web');
    expect(
      await tryCreateRelationship(GUI, carol, {
        source: filedCode,
        target: webSide.short_code,
        relationship: 'blocks',
      }),
    ).toBe(201);
    expect(
      await tryCreateRelationship(GUI, carol, {
        source: filedCode,
        target: webSide.short_code,
        relationship: 'supports',
      }),
    ).toBe(403);

    // It renders on the board with its repo chip.
    await page.goto('/boards/platform-delivery');
    const card = page.locator('article.kairos-card', { hasText: filedCode });
    await expect(card).toBeVisible();
    await expect(card.locator('.kairos-card__repo[data-repo="payments-api"]')).toBeVisible();
  });

  // 3b. The New task modal's Repository select (KAIROS-T-0124 #6b) ----------
  await test.step('the New task modal offers the team\'s repositories; the card carries the chip at once', async () => {
    const title = `Picked in the modal ${RUN}`;
    await page.getByRole('button', { name: 'New task', exact: true }).click();
    const modal = page.locator('.cl-modal');
    await expect(modal.locator('.cl-modal__title')).toHaveText('New task');
    await modal.locator('input.cl-input').first().fill(title);
    const picker = modal.locator('[data-testid="create-repository"] select');
    await expect(picker).toBeVisible();
    // "(none)" first, then the platform team's repositories (and only theirs).
    await expect(picker.locator('option').first()).toHaveText('(none)');
    await expect(picker.locator('option', { hasText: 'payments-api' })).toHaveCount(1);
    await expect(picker.locator('option', { hasText: 'platform-infra' })).toHaveCount(1);
    await expect(picker.locator('option', { hasText: 'portal-web' })).toHaveCount(0);
    await picker.selectOption('payments-api');
    await modal.getByRole('button', { name: 'Create' }).click();
    const created = page.locator('article.kairos-card', { hasText: title });
    await expect(created).toBeVisible({ timeout: 10_000 });
    await expect(created.locator('.kairos-card__repo[data-repo="payments-api"]')).toBeVisible();
  });

  // 4. The picker re-homes within the team ------------------------------------
  await test.step('the item page picker re-homes the task to another platform repo', async () => {
    await page.goto(`/items/${filedCode}`);
    await expect(page.locator('.cl-pill', { hasText: 'repo: payments-api' })).toBeVisible();
    const control = page.locator('[data-testid="repository-control"]');
    await expect(control).toBeVisible();
    await control.locator('select').selectOption('platform-infra');
    await control.getByRole('button', { name: 'Set repository' }).click();
    await expect(page.locator('.cl-pill', { hasText: 'repo: platform-infra' })).toBeVisible({
      timeout: 10_000,
    });
    // …and back, so the PR link-back below targets payments-api.
    await control.locator('select').selectOption('payments-api');
    await control.getByRole('button', { name: 'Set repository' }).click();
    await expect(page.locator('.cl-pill', { hasText: 'repo: payments-api' })).toBeVisible({
      timeout: 10_000,
    });
  });

  // 4b. Moving a task to another delivery board (KAIROS-I-0012) ------------
  await test.step('the item page moves a task to another delivery board; both boards react live', async () => {
    const platform = await loadPlatformDelivery(GUI, alice);

    // The T-0104 repository rule is the refusal people will actually hit:
    // the cross-team task is bound to payments-api (platform's repo), so
    // the web board is refused — inline, under the picker.
    await page.goto(`/items/${filedCode}`);
    const mover = page.locator('[data-testid="move-board"]');
    await expect(mover).toBeVisible();
    const picker = mover.locator('select');
    // Default is "stay put"; the board the task is already on is never an
    // option, the other delivery board alice manages is.
    await expect(picker.locator('option').first()).toHaveText('(this board)');
    await expect(picker.locator('option', { hasText: 'Web Delivery' })).toHaveCount(1);
    await expect(picker.locator('option', { hasText: 'Platform Delivery' })).toHaveCount(0);
    await picker.selectOption('web-delivery');
    await mover.getByRole('button', { name: 'Move board' }).click();
    await expect(mover).toContainText('REPOSITORY_OWNER_MISMATCH', { timeout: 10_000 });
    await expect(mover).toContainText('payments-api');

    // An UNBOUND task moves: the Board panel re-renders on the target
    // board, in its entry column, without a reload.
    const moving = await createTask(GUI, alice, {
      title: `Moves to web ${RUN}`,
      boardId: platform.boardId,
    });
    await page.goto(`/items/${moving.short_code}`);
    const boardPanel = panel(page, 'Board');
    await expect(boardPanel).toContainText('Platform Delivery');
    await boardPanel.locator('[data-testid="move-board"] select').selectOption('web-delivery');
    await boardPanel.getByRole('button', { name: 'Move board' }).click();
    await expect(page.locator('.kairos-item__notice')).toContainText('Moved to Web Delivery.', {
      timeout: 10_000,
    });
    await expect(boardPanel.getByRole('link', { name: 'Web Delivery' })).toBeVisible();
    await expect(boardPanel.locator('.cl-pill', { hasText: 'Backlog' })).toBeVisible();

    // It arrived on the web board and left the platform board.
    await page.goto('/boards/web-delivery');
    await expect(
      page.locator('article.kairos-card', { hasText: moving.short_code }),
    ).toBeVisible({ timeout: 10_000 });
    await page.goto('/boards/platform-delivery');
    await expect(
      page.locator('article.kairos-card', { hasText: moving.short_code }),
    ).toHaveCount(0);

    // Live, SOURCE side: a second writer moves another platform task away
    // and this board drops the card over the socket. The page-scoped
    // marker proves no reload happened (the smoke spec's technique).
    const leaving = await createTask(GUI, alice, {
      title: `Leaves platform ${RUN}`,
      boardId: platform.boardId,
    });
    await page.reload();
    await expect(
      page.locator('article.kairos-card', { hasText: leaving.short_code }),
    ).toBeVisible({ timeout: 10_000 });
    await page.evaluate(() => ((window as any).__noReload = 'alive'));
    await moveTask(GUI, alice, leaving.short_code, 'web-delivery');
    await expect(
      page.locator('article.kairos-card', { hasText: leaving.short_code }),
    ).toHaveCount(0, { timeout: 20_000 });
    expect(await page.evaluate(() => (window as any).__noReload)).toBe('alive');

    // Live, TARGET side: watching the web board, a task moved in from
    // platform shows up — again with no reload.
    const arriving = await createTask(GUI, alice, {
      title: `Arrives on web ${RUN}`,
      boardId: platform.boardId,
    });
    await page.goto('/boards/web-delivery');
    await expect(
      page.locator('article.kairos-card', { hasText: leaving.short_code }),
    ).toBeVisible({ timeout: 10_000 });
    await page.evaluate(() => ((window as any).__noReload = 'alive'));
    await moveTask(GUI, alice, arriving.short_code, 'web-delivery');
    await expect(
      page.locator('article.kairos-card', { hasText: arriving.short_code }),
    ).toBeVisible({ timeout: 20_000 });
    expect(await page.evaluate(() => (window as any).__noReload)).toBe('alive');

    // Back to the platform board for the steps that follow.
    await page.goto(`/items/${filedCode}`);
  });

  // 5. A PR in the repo links back to the cross-team task -------------------
  await test.step('a signed PR naming the filed task links back to it', async () => {
    // A fresh repo + connection (the seeded payments-api connection's secret
    // is not known to the test).
    const billing = `billing-worker-${RUN}`;
    const fresh = await createRepository(GUI, alice, {
      slug: billing,
      repoFullName: `acme/${billing}`,
      team: 'platform',
    });
    const connection = await createForgeConnection(GUI, alice, fresh.slug);
    const opened = githubPullRequest({
      number: 77,
      code: filedCode,
      repoFullName: `acme/${billing}`,
      state: 'open',
      updatedAt: '2026-09-22T10:00:00Z',
      title: `Export endpoint for ${filedCode}`,
    });
    expect(await deliverGithubWebhook(GUI, connection, 'pull_request', opened)).toBe(200);
    const dev = panel(page, 'Development');
    await expect(dev.getByText('#77', { exact: false })).toBeVisible({ timeout: 20_000 });
    await expect(dev.getByText(`acme/${billing}`, { exact: false })).toBeVisible();
    const directory = await listRepositories(GUI, alice, 'platform');
    expect(directory.find((r) => r.slug === billing)?.hasWebhook).toBe(true);
  });

  // 6. Admin page: register + connect, secret shown once ----------------------
  await test.step('admin registers a repo and connects its webhook', async () => {
    await page.goto('/admin/repositories');
    await expect(page.locator('[data-repo="payments-api"]').first()).toBeVisible();
    const form = panel(page, 'Register repository');
    const field = (label: string) =>
      form.locator('.cl-field', {
        has: page.locator('.cl-field__label', { hasText: label }),
      });
    const notifier = `notifier-${RUN}`;
    await field('Full name').locator('input').fill(`acme/${notifier}`);
    await field('URL').locator('input').fill(`https://github.com/acme/${notifier}`);
    await field('Slug (optional)').locator('input').fill(notifier);
    await field('Owning team').locator('select').selectOption({ label: 'web' });
    await form.getByRole('button', { name: 'Register repository' }).click();
    const row = page.locator(`[data-repo="${notifier}"]`).first();
    await expect(row).toBeVisible({ timeout: 10_000 });
    await expect(row).toContainText('owner: web');
    await row.getByRole('button', { name: 'Connect webhook' }).click();
    const secret = page.locator('[data-testid="webhook-secret"]');
    await expect(secret).toBeVisible({ timeout: 10_000 });
    // The delivery URL names the forge and the tenant — whichever tenant
    // the stack was seeded with.
    await expect(secret).toContainText('/webhooks/github/');
    await secret.getByRole('button', { name: 'I have copied them' }).click();
    await expect(secret).toHaveCount(0);
    await expect(page.locator(`[data-repo="${notifier}"]`).first()).toContainText('webhooks');
  });
});
