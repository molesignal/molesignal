/**
 * A trace with a span open: what the keyboard reaches, and an axe gate.
 *
 * `a11y-routes` scans a route as it loads, in the mock user's default theme.
 * A trace shows its waterfall rows once it is open and its span panel once a
 * span is picked, and text that reads on dark can fall short of AA on light,
 * so both trace views are scanned here with a span picked, in both themes.
 */
import AxeBuilder from '@axe-core/playwright';
import type { Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';

/** The rules a trace view once broke: each is a kind of fault a new row or panel can bring back. */
const RULES = ['color-contrast', 'nested-interactive', 'heading-order', 'definition-list', 'dlitem'];

/** Room for a page to load: a full load from the dev server takes several seconds when the suite runs. */
const LOADED = { timeout: 15_000 };

/** A trace for each tone an HTTP method is set in. */
const OPERATIONS = [
  'GET /users',
  'POST /checkout',
  'PUT /cart',
  'DELETE /cart/1',
  'OPTIONS /health',
  'TRACE /debug',
];

async function serveTraceList(page: Page) {
  const now = Date.parse(FROZEN_NOW_ISO);
  await page.route(/\/api\/v1\/web\/traces(\?.*)?$/, (route) =>
    route.fulfill({
      json: {
        items: OPERATIONS.map((operation, index) => ({
          trace_id: `trace-${index}`,
          operation,
          service: 'checkout-api',
          start_ns: (now - (index + 1) * 20 * 60 * 1000) * 1_000_000,
          duration_ms: 40 + index,
          span_count: 4,
          error_count: index % 4 === 0 ? 1 : 0,
        })),
        next_cursor: null,
        previous_cursor: null,
        has_more: false,
      },
    }),
  );
}

/** Picks the second span of the mock trace from the keyboard, which opens its panel. */
async function pickSpan(page: Page) {
  await expect(page.getByTestId('trace-span-bar')).toHaveCount(3, LOADED);
  const select = page.getByTestId('trace-span-select').nth(1);
  await select.focus();
  await page.keyboard.press('Enter');
  await expect(select).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByRole('heading', { level: 2, name: 'GET /users' })).toBeVisible();
}

async function violations(page: Page) {
  // The page's own content: the shell around it is scanned by `a11y-routes`.
  const results = await new AxeBuilder({ page }).include('#main').withRules(RULES).analyze();
  return results.violations.map((violation) => ({
    rule: violation.id,
    nodes: violation.nodes.map((node) => node.target.join(' ')),
  }));
}

test.describe('a11y: trace with a span open', () => {
  for (const theme of ['light', 'dark'] as const) {
    test(`trace detail has no contrast, nesting, heading or list violation (${theme})`, async ({
      page,
      mockServer,
    }) => {
      await mountMockRoutes(page, mockServer.port, { theme });
      await page.goto('/traces/demo-trace');
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
      await pickSpan(page);

      expect(await violations(page)).toEqual([]);
    });

    test(`trace list with a trace open has no contrast, nesting, heading or list violation (${theme})`, async ({
      page,
      mockServer,
    }) => {
      await mountMockRoutes(page, mockServer.port, { theme });
      await serveTraceList(page);
      await page.goto('/traces');
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme);

      // The open trace's row is the one fill a method is hardest to read on.
      const rows = page
        .locator('[data-workspace-pane="trace-results"]')
        .getByRole('button', { name: /^Inspect trace / });
      await expect(rows).toHaveCount(OPERATIONS.length, LOADED);
      await rows.nth(1).click();
      await expect(rows.nth(1)).toHaveAttribute('aria-pressed', 'true');
      await pickSpan(page);

      expect(await violations(page)).toEqual([]);
    });
  }

  test('a span is selected and folded from the keyboard, and its references still open', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/traces/demo-trace');
    const bars = page.getByTestId('trace-span-bar');
    await expect(bars).toHaveCount(3, LOADED);

    // Tab leaves the waterfall's head for the first row: the button that selects its span,
    // then its fold mark, then the references to the span and to its service.
    const selects = page.getByTestId('trace-span-select');
    const folds = page.getByRole('button', { name: 'Toggle span children' });
    await page.getByRole('button', { name: 'Collapse', exact: true }).focus();
    await page.keyboard.press('Tab');
    await expect(selects.nth(0)).toBeFocused();
    await page.keyboard.press('Tab');
    await expect(folds.nth(0)).toBeFocused();
    await page.keyboard.press('Tab');
    await expect(page.getByRole('button', { name: 'GET /', exact: true })).toBeFocused();
    await page.keyboard.press('Tab');
    await expect(page.getByRole('button', { name: 'web', exact: true })).toBeFocused();
    await page.keyboard.press('Tab');
    await expect(selects.nth(1)).toBeFocused();

    // Enter selects the span: its panel opens.
    await page.keyboard.press('Enter');
    await expect(selects.nth(1)).toHaveAttribute('aria-pressed', 'true');
    const panelTitle = page.getByRole('heading', { level: 2 });
    await expect(panelTitle).toHaveText(/GET \/users/);

    // The mark of the span above folds what is under it and selects nothing.
    await folds.nth(0).focus();
    await expect(folds.nth(0)).toHaveAttribute('aria-expanded', 'true');
    await page.keyboard.press('Space');
    await expect(folds.nth(0)).toHaveAttribute('aria-expanded', 'false');
    await expect(bars).toHaveCount(1);
    await expect(selects.nth(0)).toHaveAttribute('aria-pressed', 'false');
    await expect(panelTitle).toHaveText(/GET \/users/);
    await page.keyboard.press('Enter');
    await expect(bars).toHaveCount(3);

    // A span with nothing under it has a mark that is no stop: after its row's button come its references.
    await selects.nth(2).focus();
    await page.keyboard.press('Tab');
    await expect(page.getByRole('button', { name: 'SELECT users', exact: true })).toBeFocused();

    // A reference opens its own menu and selects nothing either.
    await page.keyboard.press('Enter');
    await expect(page.getByRole('link', { name: /View span logs/ })).toBeVisible();
    await expect(selects.nth(2)).toHaveAttribute('aria-pressed', 'false');
  });
});
