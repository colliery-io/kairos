// COLLIERY-T-1836 — the light and the dark theme (Aurora 0.4).
//
// One serial flow as alice (org admin):
//
//   1. /login follows the theme of the operating system: with no choice, the
//      page has no data-theme, and the computed background of <body> is
//      different for a light and a dark OS setting
//   2. REAL PKCE login via the Dex form
//   3. the ThemeToggle in the top bar sets data-theme on <html> ("light",
//      "dark"), the background of <body> changes, and the pressed button
//      follows the choice
//   4. the choice survives a reload: THEME_INIT_SCRIPT in index.html sets
//      data-theme before the app loads
//   5. the main pages (boards, a board, an item, the graph, search,
//      activity, admin) render in the two themes with no raw colour in an
//      inline style or in an SVG fill/stroke attribute (the tokens only)
//   6. "System" removes the choice: data-theme goes away again
//
// Screenshots: with AURORA_REVIEW_DIR set, each page is saved there as
// <page>-<theme>.png, for the visual review of the migration.

import { test, expect, type Page } from '@playwright/test';

const REVIEW_DIR = process.env.AURORA_REVIEW_DIR;

/** A raw colour in CSS text: the same rule as `angreal web lint`. */
const RAW_COLOUR = /#[0-9a-f]{3,8}\b|\brgba?\(|\bhsla?\(/i;

async function bodyBackground(page: Page): Promise<string> {
  return page.evaluate(() => getComputedStyle(document.body).backgroundColor);
}

async function themeAttr(page: Page): Promise<string | null> {
  return page.evaluate(() => document.documentElement.getAttribute('data-theme'));
}

/** Each inline style and each SVG fill/stroke attribute with a raw colour. */
async function rawColours(page: Page): Promise<string[]> {
  return page.evaluate((source) => {
    const re = new RegExp(source, 'i');
    const found: string[] = [];
    for (const el of Array.from(document.querySelectorAll('body [style]'))) {
      const style = el.getAttribute('style') ?? '';
      if (re.test(style)) found.push(`${el.tagName.toLowerCase()} style="${style}"`);
    }
    for (const el of Array.from(document.querySelectorAll('body [fill], body [stroke]'))) {
      for (const name of ['fill', 'stroke']) {
        const value = el.getAttribute(name);
        if (value && re.test(value)) found.push(`${el.tagName.toLowerCase()} ${name}="${value}"`);
      }
    }
    return found;
  }, RAW_COLOUR.source);
}

const toggle = (page: Page) => page.locator('.cl-appshell__header .cl-theme-toggle');

async function choose(page: Page, theme: 'Light' | 'Dark' | 'System') {
  const button = toggle(page).getByRole('button', { name: theme, exact: true });
  await button.click();
  await expect(button).toHaveAttribute('aria-pressed', 'true');
}

async function shoot(page: Page, name: string, theme: string) {
  if (!REVIEW_DIR) return;
  await page.screenshot({ path: `${REVIEW_DIR}/${name}-${theme}.png`, fullPage: true });
}

interface MainPage {
  name: string;
  open: (page: Page) => Promise<void>;
}

const PAGES: MainPage[] = [
  {
    name: 'boards',
    open: async (page) => {
      await page.goto('/boards');
      await expect(page.locator('.kairos-board-tile').first()).toBeVisible({ timeout: 20_000 });
    },
  },
  {
    name: 'board',
    open: async (page) => {
      await page.goto('/boards/platform-delivery');
      await expect(page.locator('article.kairos-card').first()).toBeVisible({ timeout: 20_000 });
    },
  },
  {
    name: 'item',
    open: async (page) => {
      await page.goto('/items/PLATFORM-T-0001');
      await expect(page.locator('.kairos-editor')).toBeVisible({ timeout: 20_000 });
      await expect(page.locator('.kairos-metadata')).toBeVisible();
    },
  },
  {
    name: 'graph',
    open: async (page) => {
      await page.goto('/items/PLATFORM-T-0001?view=graph');
      await expect(page.locator('.cl-dag__node--current')).toBeVisible({ timeout: 20_000 });
    },
  },
  {
    name: 'search',
    open: async (page) => {
      await page.goto('/search');
      const query = page.locator('.cl-field', { hasText: 'Text query' }).locator('input');
      await expect(query).toBeVisible({ timeout: 20_000 });
      await query.fill('portal');
      await page.getByRole('button', { name: 'Search', exact: true }).click();
      await expect(page.locator('.cl-pager')).toBeVisible({ timeout: 20_000 });
    },
  },
  {
    name: 'activity',
    open: async (page) => {
      await page.goto('/activity');
      await expect(page.locator('.cl-pager')).toBeVisible({ timeout: 20_000 });
    },
  },
  {
    name: 'admin',
    open: async (page) => {
      await page.goto('/admin/boards');
      await expect(page.locator('.cl-tabs')).toBeVisible({ timeout: 20_000 });
      await expect(page.locator('.cl-panel').first()).toBeVisible();
    },
  },
];

test('theme: follows the OS, the toggle switches and persists, pages use tokens only', async ({
  page,
}) => {
  // 1. With no choice, the page follows the OS --------------------------------
  await test.step('/login follows the OS setting', async () => {
    await page.emulateMedia({ colorScheme: 'light' });
    await page.goto('/login');
    await expect(page.locator('.cl-auth-card')).toBeVisible({ timeout: 30_000 });
    expect(await themeAttr(page)).toBeNull();
    const light = await bodyBackground(page);
    await shoot(page, 'login', 'light');
    await page.emulateMedia({ colorScheme: 'dark' });
    await expect.poll(() => bodyBackground(page)).not.toBe(light);
    await shoot(page, 'login', 'dark');
    expect(await rawColours(page)).toEqual([]);
  });

  // 2. Login -------------------------------------------------------------------
  await test.step('login via Dex as alice', async () => {
    await page.goto('/');
    await page.waitForSelector('#login', { timeout: 30_000 });
    await page.fill('#login', 'alice@kairos.test');
    await page.fill('#password', 'alice-password');
    await page.click('#submit-login');
    await page.waitForURL((url) => url.pathname.startsWith('/boards'), { timeout: 30_000 });
    await expect(toggle(page)).toBeVisible();
    // No choice yet: System is pressed, and the page has no data-theme.
    await expect(toggle(page).getByRole('button', { name: 'System', exact: true })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    expect(await themeAttr(page)).toBeNull();
  });

  // 3. The toggle sets data-theme --------------------------------------------
  let lightBackground = '';
  let darkBackground = '';
  await test.step('the toggle switches data-theme and the background', async () => {
    await choose(page, 'Light');
    expect(await themeAttr(page)).toBe('light');
    lightBackground = await bodyBackground(page);
    await choose(page, 'Dark');
    expect(await themeAttr(page)).toBe('dark');
    darkBackground = await bodyBackground(page);
    expect(darkBackground).not.toBe(lightBackground);
    // A forced theme wins over the OS setting.
    await page.emulateMedia({ colorScheme: 'light' });
    expect(await bodyBackground(page)).toBe(darkBackground);
  });

  // 4. The choice survives a reload -------------------------------------------
  await test.step('the choice survives a reload', async () => {
    await page.reload();
    // The init script runs before the WASM: the attribute is there at once.
    expect(await themeAttr(page)).toBe('dark');
    await expect(toggle(page)).toBeVisible({ timeout: 20_000 });
    await expect(toggle(page).getByRole('button', { name: 'Dark', exact: true })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    expect(await bodyBackground(page)).toBe(darkBackground);
  });

  // 5. The main pages in the two themes ---------------------------------------
  for (const theme of ['Light', 'Dark'] as const) {
    await test.step(`main pages in the ${theme.toLowerCase()} theme use tokens only`, async () => {
      await choose(page, theme);
      for (const main of PAGES) {
        await main.open(page);
        expect(await themeAttr(page), `${main.name}: data-theme`).toBe(theme.toLowerCase());
        expect(await bodyBackground(page), `${main.name}: body background`).toBe(
          theme === 'Light' ? lightBackground : darkBackground,
        );
        expect(await rawColours(page), `${main.name}: raw colours`).toEqual([]);
        await shoot(page, main.name, theme.toLowerCase());
      }
    });
  }

  // 6. System removes the choice ---------------------------------------------
  await test.step('System follows the OS again', async () => {
    await choose(page, 'System');
    expect(await themeAttr(page)).toBeNull();
    await page.emulateMedia({ colorScheme: 'dark' });
    await expect.poll(() => bodyBackground(page)).toBe(darkBackground);
    await page.emulateMedia({ colorScheme: 'light' });
    await expect.poll(() => bodyBackground(page)).toBe(lightBackground);
  });
});
