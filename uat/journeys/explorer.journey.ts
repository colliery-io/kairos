// J9 — "Someone asks where a piece of work came from" (KAIROS-I-0014).
//
// carol is handed a task and wants to know why it exists. She finds it by
// text on `/search`, narrowed to her own team's board; opens it; reads its
// parent off the item page; then walks the flight-level graph OUT to the
// initiative and the strategy above that, and back DOWN to a sibling task.
// Her last two steps ask the same question two more ways — the search
// page's traversal scope, and `kairos search --from` in a terminal — and
// require the same answer.
//
// The journey writes nothing: every claim is "the product tells one story
// about this item, whichever surface you ask". That is also what it
// catches — a graph explorer that drifts from the search page, or a
// traverse that returns something the picture never drew.
import { expect, type Locator, type Page } from '@playwright/test';
import { journey, step } from '../run/narrate';
import { openItem, panel } from '../surfaces/gui';

/** The chip row under a labelled heading ("Entity types", "Relationships"). */
function chips(page: Page, heading: string): Locator {
  // Stacks nest, and `has:` matches every ancestor that contains the text;
  // the innermost match is the last in document order.
  return page
    .locator('.cl-stack', { has: page.getByText(heading, { exact: true }) })
    .last();
}

/** A text field by its visible label. */
function field(page: Page, label: string): Locator {
  return page.locator('.cl-field', { has: page.getByText(label, { exact: true }) }).first();
}

/** Every short code the graph canvas is currently drawing. */
async function drawn(page: Page): Promise<string[]> {
  // SVG <text> has no innerText — read textContent.
  const codes = await page.locator('.kairos-graph__code').allTextContents();
  return [...new Set(codes.map((c) => c.trim()).filter(Boolean))].sort();
}

/** The graph node carrying a short code. */
function node(page: Page, code: string): Locator {
  return page.locator('.kairos-graph__node', {
    has: page.locator('.kairos-graph__code', { hasText: code }),
  });
}

/** Short codes anywhere in a search response DTO, whatever the group. */
function codesIn(results: any): string[] {
  const groups = results?.results ?? results ?? {};
  const all = Object.values(groups)
    .filter(Array.isArray)
    .flatMap((hits) => (hits as any[]).map((hit) => hit.short_code as string));
  return [...new Set(all.filter(Boolean))].sort();
}

journey(
  'explorer',
  'Someone asks where a piece of work came from',
  { humans: ['carol'] },
  async ({ cast }) => {
    const carol = cast.human('carol');
    // Plumbing, not story: which board is "carol's team's board", and
    // which of its tasks has a parent worth asking about. The journey
    // never hard-codes a seeded short code, so it reads the same against
    // a real deployment.
    const api = await carol.api();
    const me = await api.whoami();
    const teamSlug = (me.teams ?? [])[0]?.slug as string;
    expect(teamSlug, 'carol is on a team').toBeTruthy();
    const board = await api.boardBySlug(`${teamSlug}-delivery`);
    const onBoard: any[] = ((await api.get(`/api/boards/${board.id}/items`)).columns ?? []).flatMap(
      (c: any) => c.tasks ?? [],
    );
    let subject = '';
    let subjectTitle = '';
    let initiative = '';
    for (const task of onBoard) {
      const rels = await api.get(`/api/tasks/${task.short_code}/relationships`);
      const parent = (rels.incoming ?? [])
        .filter((g: any) => g.relationship === 'parent')
        .flatMap((g: any) => g.items)
        .find((item: any) => item.entity_type === 'initiative');
      if (parent) {
        subject = task.short_code;
        subjectTitle = task.title;
        initiative = parent.short_code;
        break;
      }
    }
    expect(subject, `a task on ${board.slug} has a parent initiative`).toBeTruthy();
    // The word carol half-remembers from the title.
    const phrase = [...subjectTitle.matchAll(/[A-Za-z]{5,}/g)]
      .map((m) => m[0])
      .sort((a, b) => b.length - a.length)[0];
    expect(phrase, `"${subjectTitle}" has a searchable word`).toBeTruthy();

    let strategy = '';
    let canvas: string[] = [];
    let fromSearchPage: string[] = [];

    await step(carol, `searches for "${phrase}" and narrows it to her own team's board`, async () => {
      const page = await carol.gui();
      await page.goto('/search');
      await field(page, 'Text query').locator('input').fill(phrase);
      await chips(page, 'Entity types').getByRole('button', { name: 'task', exact: true }).click();
      await field(page, 'Board').locator('select').selectOption(board.slug);
      await field(page, 'Page size').locator('select').selectOption('50');
      await page.getByRole('button', { name: 'Search', exact: true }).click();
      const tasks = panel(page, 'Tasks');
      await expect(tasks).toBeVisible({ timeout: 15_000 });
      await expect(tasks.getByRole('link', { name: subject, exact: true })).toBeVisible();
      const hits = await tasks.locator('tbody tr').count();
      return { query: phrase, board: board.slug, entity_type: 'task', hits, found: subject };
    });

    await step(carol, 'opens the one she was asked about and reads what it hangs off', async () => {
      const page = await carol.gui();
      await panel(page, 'Tasks').getByRole('link', { name: subject, exact: true }).click();
      await page.waitForURL(new RegExp(`/items/${subject}`));
      const relationships = panel(page, 'Relationships');
      await expect(relationships).toBeVisible({ timeout: 15_000 });
      const group = relationships.locator('.kairos-relationships__group', { hasText: 'parent' });
      const parentLink = group.getByRole('link', { name: new RegExp(`^${initiative} — `) });
      await expect(parentLink).toBeVisible();
      return {
        task: subject,
        title: subjectTitle,
        parent: (await parentLink.innerText()).trim(),
      };
    });

    await step(carol, 'switches to the graph and refocuses on the initiative above it', async () => {
      const page = await carol.gui();
      await panel(page, 'Relationships').getByRole('link', { name: 'Open the graph explorer' }).click();
      await page.waitForURL(new RegExp(`/search/relationships/${subject}`));
      await expect(page.locator('.kairos-graph__node--focus .kairos-graph__code')).toHaveText(subject, {
        timeout: 20_000,
      });
      const before = await drawn(page);
      expect(before, 'the initiative is on the task\'s canvas').toContain(initiative);
      // Clicking a non-focus node's body refocuses and extends `?trail=`.
      await node(page, initiative).locator('.kairos-graph__box').click();
      await page.waitForURL(new RegExp(`/search/relationships/${initiative}\\?trail=${subject}`));
      await expect(page.locator('.kairos-graph__node--focus .kairos-graph__code')).toHaveText(initiative, {
        timeout: 20_000,
      });
      return { from: subject, now_focused: initiative, trail: subject, canvas_had: before.length };
    });

    await step(carol, 'goes one level further out, to the strategy the initiative serves', async () => {
      const page = await carol.gui();
      const above = (await drawn(page)).filter((code) => /-S-\d{4}$/.test(code));
      expect(above, 'a strategy sits above the initiative').not.toHaveLength(0);
      strategy = above[0];
      await node(page, strategy).locator('.kairos-graph__box').click();
      await page.waitForURL(new RegExp(`/search/relationships/${strategy}\\?trail=`));
      await expect(page.locator('.kairos-graph__node--focus .kairos-graph__code')).toHaveText(strategy, {
        timeout: 20_000,
      });
      await expect(page.getByText('trail:')).toBeVisible();
      canvas = await drawn(page);
      expect(canvas).toContain(initiative);
      expect(canvas).toContain(subject);
      return {
        now_focused: strategy,
        trail: `${subject} › ${initiative}`,
        canvas_draws: canvas.length,
      };
    });

    await step(carol, 'walks back down the picture to a sibling of the task she started from', async () => {
      const page = await carol.gui();
      const children = await api.get(`/api/initiatives/${initiative}/relationships`);
      const siblings = (children.outgoing ?? [])
        .filter((g: any) => g.relationship === 'parent')
        .flatMap((g: any) => g.items)
        .map((item: any) => item.short_code as string)
        .filter((code: string) => code !== subject && canvas.includes(code));
      expect(siblings, `${initiative} has another child on the canvas`).not.toHaveLength(0);
      const sibling = siblings[0];
      // The node's CODE text is the link to the item; the box refocuses.
      await node(page, sibling).locator('.kairos-graph__code').click();
      await page.waitForURL(new RegExp(`/items/${sibling}`));
      await openItem(page, sibling);
      return { sibling, shares_parent_with: subject, under: initiative };
    });

    await step(carol, 'asks the search page for everything the strategy reaches, rather than a picture of it', async () => {
      const page = await carol.gui();
      await page.goto('/search');
      await page.getByText('Limit results to items reachable from…').click();
      await field(page, 'From (short code)').locator('input').fill(strategy);
      await field(page, 'Page size').locator('select').selectOption('50');
      // `parent` is the relationship chip that is on by default; depth 2
      // is what the canvas drew, so the two answers are comparable.
      await expect(
        chips(page, 'Relationships').locator('button.cl-chip--active', { hasText: 'parent' }),
      ).toBeVisible();
      await field(page, 'Depth (1–10)').locator('input').fill('2');
      await page.getByRole('button', { name: 'Search', exact: true }).click();
      await expect(panel(page, 'Initiatives')).toBeVisible({ timeout: 15_000 });
      const links = await page.locator('.cl-panel tbody td a .cl-mono').allInnerTexts();
      fromSearchPage = [...new Set(links.map((t) => t.trim()).filter(Boolean))].sort();
      expect(fromSearchPage).toContain(initiative);
      expect(fromSearchPage).toContain(subject);
      // Nothing the list names is missing from the picture she just walked.
      expect(fromSearchPage.filter((code) => !canvas.includes(code))).toEqual([]);
      return {
        from: strategy,
        relationships: 'parent',
        depth: 2,
        listed: fromSearchPage.length,
        all_of_them_were_on_the_canvas: true,
      };
    });

    await step(carol, 'asks the same question from the terminal and gets the same answer', async () => {
      const cli = await carol.cli();
      const traversed = await cli.json([
        'search',
        '--from', strategy,
        '--relationships', 'parent',
        '--direction', 'outbound',
        '--depth', '2',
        '--limit', '100',
      ]);
      const codes = codesIn(traversed);
      expect(codes).toEqual(fromSearchPage);
      return {
        command: `kairos search --from ${strategy} --relationships parent --depth 2`,
        returned: codes.length,
        same_as_the_search_page: true,
        same_as_the_graph: codes.every((code) => canvas.includes(code)),
      };
    });
  },
);
