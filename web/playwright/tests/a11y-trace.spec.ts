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
          traceId: `trace-${index}`,
          operation,
          service: 'checkout-api',
          startNs: (now - (index + 1) * 20 * 60 * 1000) * 1_000_000,
          durationMs: 40 + index,
          spanCount: 4,
          errorCount: index % 4 === 0 ? 1 : 0,
        })),
        nextCursor: null,
        previousCursor: null,
        hasMore: false,
      },
    }),
  );
}

/** Picks the second span of the mock trace from the keyboard, which opens its detail. */
async function pickSpan(page: Page) {
  await expect(page.getByTestId('trace-span-row')).toHaveCount(3, LOADED);
  const select = page.getByTestId('trace-span-select').nth(1);
  await select.focus();
  await page.keyboard.press('Enter');
  await expect(select).toHaveAttribute('aria-pressed', 'true');
  await expect(page.getByTestId('trace-span-panel').getByRole('heading', { level: 2, name: /GET \/users/ })).toBeVisible();
}

async function violations(page: Page, scope = '#main') {
  // The page's own content, or the drawer over it: the shell around it is scanned by `a11y-routes`.
  const results = await new AxeBuilder({ page }).include(scope).withRules(RULES).analyze();
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
      await page.setViewportSize({ width: 1440, height: 900 });
      await page.goto('/traces');
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme);

      // Every tone an HTTP method is set in, on the list; then the trace of one of them in the drawer.
      const rows = page.locator('[data-trace-row]');
      await expect(rows).toHaveCount(OPERATIONS.length, LOADED);
      expect(await violations(page)).toEqual([]);

      await rows.nth(1).click();
      await expect(rows.nth(1)).toHaveAttribute('aria-pressed', 'true');
      await pickSpan(page);

      expect(await violations(page, '[data-trace-drawer]')).toEqual([]);
    });
  }

  test('a span is selected and folded from the keyboard, and the references of its detail open their menus', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/traces/demo-trace');
    const bars = page.getByTestId('trace-span-bar');
    await expect(bars).toHaveCount(3, LOADED);

    // The waterfall is one stop: the arrows walk its rows, and the row walked to is selected.
    const selects = page.getByTestId('trace-span-select');
    await selects.nth(0).focus();
    await page.keyboard.press('ArrowDown');
    await expect(selects.nth(1)).toBeFocused();
    await expect(selects.nth(1)).toHaveAttribute('aria-pressed', 'true');
    const panelTitle = page.getByTestId('trace-span-panel').getByRole('heading', { level: 2 });
    await expect(panelTitle).toHaveText(/GET \/users/);

    // Left folds what is under the span, right opens it again; folded, left goes up to its parent.
    const fold = page.getByTestId('trace-span-row').nth(1).getByRole('button', { name: /spans it called/ });
    await page.keyboard.press('ArrowLeft');
    await expect(fold).toHaveAttribute('aria-expanded', 'false');
    await expect(bars).toHaveCount(2);
    await page.keyboard.press('ArrowRight');
    await expect(fold).toHaveAttribute('aria-expanded', 'true');
    await expect(bars).toHaveCount(3);
    await page.keyboard.press('ArrowLeft');
    await expect(bars).toHaveCount(2);
    await page.keyboard.press('ArrowLeft');
    await expect(selects.nth(0)).toBeFocused();
    // Right goes down into an open span, and opens a folded one.
    await page.keyboard.press('ArrowRight');
    await expect(selects.nth(1)).toBeFocused();
    await page.keyboard.press('ArrowRight');
    await expect(bars).toHaveCount(3);

    // The detail of the span is reached from the keyboard too, and its references open their menus.
    await selects.nth(1).focus();
    await page.keyboard.press('Enter');
    await expect(panelTitle).toHaveText(/GET \/users/);
    const service = page.getByTestId('trace-span-panel').getByRole('button', { name: 'api', exact: true });
    await service.focus();
    await page.keyboard.press('Enter');
    await expect(page.getByRole('link', { name: /View service metrics/ })).toBeVisible();
  });
});
