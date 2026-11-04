import { expect, test, type Page } from '@playwright/test';

import { explorerRead, explorerReadBody } from '../fixtures/explorerReads';
import { installMockShellSession } from '../fixtures/mockSession';

/** More rows than the largest page holds, so every size has a full page to show. */
const AVAILABLE = 6_000;
const SIZES = ['100', '200', '500', '1,000', '2,000', '5,000'];

const field = (name: string, dataType = 'utf8') => ({ name, dataType: dataType, nullable: true, indexed: false });
const stream = (name: string, type: 'logs' | 'traces', fields: ReturnType<typeof field>[]) => ({
  id: `${type}-stream`,
  name,
  streamType: type,
  schema: { fields },
  retention: { days: 7 },
  effectiveRetention: { days: 7 },
  settings: {
    description: null,
    indexRules: [],
    retentionFilter: null,
    keepConditions: [],
    maxQueryRangeHours: null,
    flattenLevel: null,
    useStreamStatsForPartitioning: false,
    storeOriginalData: false,
    enableDistinctValues: true,
    queryable: true,
  },
  createdAt: 1,
  updatedAt: 1,
});

async function choosePageSize(page: Page, size: string) {
  await page.locator('[data-page-size]').click();
  await expect(page.getByRole('option')).toHaveText(SIZES);
  await page.getByRole('option', { name: size, exact: true }).click();
}

test.describe('Explorer page sizes', () => {
  test.beforeEach(async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await installMockShellSession(page);
  });

  test('Logs reads a hundred rows to a page, and five thousand when asked', async ({ page }) => {
    const now = Date.now();
    // One log every 100 ms, newest first: six thousand of them are ten minutes.
    const items = Array.from({ length: AVAILABLE }, (_, index) => ({
      _timestamp: (now - 5_000 - index * 100) * 1_000,
      level: index % 50 === 7 ? 'ERROR' : 'INFO',
      'service.name': 'checkout-api',
      message: `request ${index} completed`,
    }));
    const asked: number[] = [];
    await page.route('**/api/v1/streams**', (route) =>
      route.fulfill({
        json: [
          stream('application_logs', 'logs', [
            { name: '_timestamp', dataType: 'timestamp', nullable: false, indexed: true },
            field('level'),
            field('service.name'),
            field('message'),
          ]),
        ],
      }),
    );
    await page.route(explorerRead('web/logs'), (route) => {
      const body = explorerReadBody<{ limit?: number }>(route.request());
      const limit = body.limit ?? 20;
      asked.push(limit);

      return route.fulfill({
        json: { items: items.slice(0, limit), nextCursor: 'older', previousCursor: null, hasMore: true },
      });
    });
    await page.route('**/api/v1/field_masking/effective/**', (route) => route.fulfill({ json: { fields: [] } }));
    await page.goto('/logs');

    const rows = page.locator('[data-log-result-row="logs"]');
    await expect(rows).toHaveCount(100);
    await expect(page.locator('[data-page-size]')).toHaveText('100');
    expect(asked).toEqual([100]);

    await choosePageSize(page, '5,000');
    await expect(rows).toHaveCount(5_000, { timeout: 30_000 });
    await expect(page.locator('[data-page-size]')).toHaveText('5,000');
    await expect(page.locator('[data-log-result-summary]')).toContainText('5,000 events');
    expect(asked).toEqual([100, 5_000]);

    // Every row is in the page, the last as reachable as the first; the scrollbar is of five thousand rows.
    const list = rows.first().locator('xpath=..');
    const height = await list.evaluate((element) => element.scrollHeight);
    expect(height).toBeGreaterThan(5_000 * 32);
    expect(height).toBeLessThan(5_000 * 32 + 64);
    await rows.last().scrollIntoViewIfNeeded();
    await expect(rows.last()).toContainText('request 4999 completed');

    // A log far down the page opens like any other.
    await rows.nth(4_990).click();
    const drawer = page.getByRole('complementary', { name: 'Log detail drawer' });
    await expect(drawer.locator('[data-log-detail-message]')).toHaveText('request 4990 completed');
    await expect(drawer.getByLabel('4991 of 5000')).toBeVisible();
    await expect(rows.nth(4_990)).toHaveAttribute('aria-pressed', 'true');
  });

  test('Traces reads a hundred traces to a page, and two thousand when asked', async ({ page }) => {
    const now = Date.now();
    const traces = Array.from({ length: AVAILABLE }, (_, index) => ({
      traceId: `7c1f0e${String(index).padStart(6, '0')}a94b4d2e8f3a5b6c7d8e`,
      operation: index % 3 === 0 ? 'POST /api/v1/checkout' : 'GET /api/v1/cart',
      service: 'gateway',
      startNs: (now - 5_000 - index * 100) * 1_000_000,
      durationMs: 12 + (index % 400),
      spanCount: 3 + (index % 9),
      errorCount: index % 40 === 3 ? 1 : 0,
    }));
    const asked: number[] = [];
    await page.route('**/api/v1/streams**', (route) =>
      route.fulfill({
        json: [stream('default', 'traces', [field('trace_id'), field('span_id'), field('name'), field('service.name')])],
      }),
    );
    await page.route('**/api/v1/web/topology**', (route) => route.fulfill({ json: { nodes: [], edges: [] } }));
    await page.route(/\/api\/v1\/web\/traces(\?.*)?$/, (route) => {
      const limit = Number(new URL(route.request().url()).searchParams.get('limit') ?? 20);
      asked.push(limit);

      return route.fulfill({
        json: { items: traces.slice(0, limit), nextCursor: 'older', previousCursor: null, hasMore: true },
      });
    });
    await page.route(/\/api\/v1\/web\/trace\/[^/?]+(\?.*)?$/, (route) =>
      route.fulfill({
        json: {
          traceId: 'any',
          rootSpanId: 'root',
          spans: [],
          partial: false,
          partialReasons: [],
          samplingReasons: [],
          lateSpanCount: 0,
          duplicateSpanCount: 0,
          conflictSpanCount: 0,
        },
      }),
    );
    await page.goto('/traces');

    const results = page.locator('[data-workspace-pane="trace-results"]');
    const rows = results.locator('[data-trace-row]');
    await expect(rows).toHaveCount(100);
    await expect(results.locator('[data-page-size]')).toHaveText('100');
    expect(asked.at(-1)).toBe(100);

    await choosePageSize(page, '2,000');
    await expect(rows).toHaveCount(2_000, { timeout: 30_000 });
    await expect(results.locator('[data-page-size]')).toHaveText('2,000');
    expect(asked.at(-1)).toBe(2_000);

    // The scrollbar is of two thousand rows of thirty-two under the head, and the last of them is reachable.
    const height = await rows.first().locator('xpath=..').evaluate((element) => element.scrollHeight);
    const head = await results.locator('[data-trace-table-head]').evaluate((element: HTMLElement) => element.offsetHeight);
    expect(height).toBe(head + 2_000 * 32);
    await rows.last().scrollIntoViewIfNeeded();
    await rows.last().click();
    await expect(rows.last()).toHaveAttribute('aria-pressed', 'true');
  });
});
