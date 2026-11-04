import type { Locator, Page } from '@playwright/test';
import { expect, test } from '@playwright/test';

import { installMockShellSession } from '../fixtures/mockSession';

const SETTINGS = {
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
};

const field = (name: string, dataType: 'utf8' | 'int64' = 'utf8') => ({
  name,
  dataType: dataType,
  nullable: true,
  indexed: name === 'trace_id',
});

const TRACE_STREAM = {
  id: 'trace-menus',
  name: 'default',
  streamType: 'traces',
  schema: {
    fields: [
      field('trace_id'),
      field('span_id'),
      field('parent_span_id'),
      field('name'),
      field('service.name'),
      field('duration_ns', 'int64'),
    ],
  },
  retention: { days: 7 },
  effectiveRetention: { days: 7 },
  settings: SETTINGS,
  createdAt: 1,
  updatedAt: 1,
};

const START_NS = 1_786_704_600_000_000_000;
const TRACE_ID = 'trace-menus';

const LISTED = {
  traceId: TRACE_ID,
  service: 'checkout-api',
  operation: 'POST /api/orders',
  startNs: START_NS,
  durationMs: 48,
  spanCount: 4,
  errorCount: 0,
};

const span = (
  spanId: string,
  parentSpanId: string | null,
  service: string,
  operation: string,
  fromMs: number,
  toMs: number,
) => ({
  spanId: spanId,
  parentSpanId: parentSpanId,
  service,
  operation,
  startNs: START_NS + fromMs * 1_000_000,
  endNs: START_NS + toMs * 1_000_000,
  status: 'OK',
  resource: { attributes: {} },
  attributes: {},
  events: [],
});

/** The request, the service it called, and two calls under that: four rows of the waterfall. */
const TRACE = {
  traceId: TRACE_ID,
  rootSpanId: 'root',
  truncated: false,
  spans: [
    span('root', null, 'checkout-api', 'POST /api/orders', 0, 48),
    span('stock', 'root', 'inventory', 'GET /stock', 4, 34),
    span('query', 'stock', 'postgres', 'SELECT stock', 6, 20),
    span('cache', 'stock', 'redis', 'GET stock:42', 22, 28),
  ],
};

/** Every reference menu in the page, open or still closing: one has the button that adds a filter. */
const menus = (page: Page) => page.locator('[role="dialog"]:has([data-testid="signal-pin-filter"])');

/** The reference that names a span of the waterfall, by the span's operation. */
const spanReference = (waterfall: Locator, operation: string) =>
  waterfall
    .getByTestId('trace-span-row')
    .filter({ hasText: operation })
    .locator('button[data-signal-type="span_id"]');

/**
 * A reference the open menu does not lie over: the menu stands over or under its own reference,
 * as there is room, and covers the rows on that side.
 */
async function referenceBeside(page: Page, waterfall: Locator) {
  const menu = menus(page);
  // Placed once it is in the window: until then Radix keeps it out of sight.
  await expect(menu).toBeInViewport();
  const operation = (await menu.getAttribute('data-side')) === 'top' ? 'GET stock:42' : 'POST /api/orders';
  return { reference: spanReference(waterfall, operation), operation };
}

/**
 * Two references opened in turn from their context menus leave the menu of the second alone
 * open, and select no span.
 */
async function expectOneMenuAtATime(page: Page, waterfall: Locator) {
  const first = spanReference(waterfall, 'GET /stock');
  await first.click({ button: 'right' });
  await expect(menus(page)).toHaveCount(1);
  await expect(menus(page)).toContainText('GET /stock');

  const { reference: second, operation } = await referenceBeside(page, waterfall);
  await second.click({ button: 'right' });
  await expect(menus(page)).toHaveCount(1);
  await expect(menus(page)).toContainText(operation);
  await expect(menus(page)).not.toContainText('GET /stock');
  await expect(first).toHaveAttribute('aria-expanded', 'false');
  await expect(second).toHaveAttribute('aria-expanded', 'true');

  // The menu is the reference's and not the row's: the span it names is not selected.
  const stockRow = waterfall.getByTestId('trace-span-row').filter({ hasText: 'GET /stock' });
  await expect(stockRow.getByTestId('trace-span-select')).toHaveAttribute('aria-pressed', 'false');
}

test.beforeEach(async ({ page }) => {
  await installMockShellSession(page);
  await page.route('**/api/v1/streams**', (route) => route.fulfill({ json: [TRACE_STREAM] }));
  await page.route('**/api/v1/web/topology**', (route) =>
    route.fulfill({ json: { nodes: [], edges: [] } }),
  );
  await page.route(/\/api\/v1\/web\/traces(\?.*)?$/, (route) =>
    route.fulfill({
      json: { items: [LISTED], nextCursor: null, previousCursor: null, hasMore: false },
    }),
  );
  await page.route('**/api/v1/web/traces/histogram**', (route) =>
    route.fulfill({ json: { bucketTime: 60_000_000, originTime: 0, buckets: [], split: true } }),
  );
  // The trace is asked for over the window it was listed in.
  await page.route(new RegExp(`/api/v1/web/trace/${TRACE_ID}(\\?.*)?$`), (route) => route.fulfill({ json: TRACE }));
});

/** The trace open in the drawer over the Traces view. */
async function openDrawer(page: Page) {
  await page.goto(`/traces?q=${encodeURIComponent("service.name = 'checkout-api'")}`);
  await page.locator(`[data-trace-row][data-trace-id="${TRACE_ID}"]`).click();
  const drawer = page.getByRole('dialog', { name: 'Trace detail' });
  await expect(drawer.getByTestId('trace-span-row')).toHaveCount(4);
  return drawer;
}

test('keeps one reference menu open in the waterfall, and a click on a reference selects its span', async ({ page }) => {
  const drawer = await openDrawer(page);

  await expectOneMenuAtATime(page, drawer);

  // A click beside the menu, on the trace's head, closes it.
  await drawer.getByTestId('trace-total-duration').click();
  await expect(menus(page)).toHaveCount(0);

  // A left click on a reference is the span's: it selects it, and opens no menu.
  const stock = spanReference(drawer, 'GET /stock');
  await stock.click();
  await expect(menus(page)).toHaveCount(0);
  const stockRow = drawer.getByTestId('trace-span-row').filter({ hasText: 'GET /stock' });
  await expect(stockRow.getByTestId('trace-span-select')).toHaveAttribute('aria-pressed', 'true');
  await expect(drawer.getByTestId('trace-span-panel').getByRole('heading', { name: /GET \/stock/ })).toBeVisible();
});

test('opens a reference menu from the context menu and closes it on Escape', async ({ page }) => {
  const drawer = await openDrawer(page);
  const first = spanReference(drawer, 'GET /stock');

  await first.click({ button: 'right' });
  await expect(menus(page)).toHaveCount(1);
  await expect(menus(page)).toContainText('GET /stock');

  await page.keyboard.press('Escape');
  await expect(menus(page)).toHaveCount(0);
  await expect(first).toBeFocused();
  await expect(drawer).toBeVisible();

  // Opened from the context menu, a menu closes on a click on another reference, which selects its span.
  await first.click({ button: 'right' });
  await expect(menus(page)).toContainText('GET /stock');
  const { reference: other, operation } = await referenceBeside(page, drawer);
  await other.click();
  await expect(menus(page)).toHaveCount(0);
  await expect(first).toHaveAttribute('aria-expanded', 'false');
  const otherRow = drawer.getByTestId('trace-span-row').filter({ hasText: operation });
  await expect(otherRow.getByTestId('trace-span-select')).toHaveAttribute('aria-pressed', 'true');
});

test('closes the open menu first on Escape, and the drawer on the next', async ({ page }) => {
  const drawer = await openDrawer(page);

  await expectOneMenuAtATime(page, drawer);

  await page.keyboard.press('Escape');
  await expect(menus(page)).toHaveCount(0);
  await expect(drawer).toBeVisible();

  await page.keyboard.press('Escape');
  await expect(drawer).toHaveCount(0);
});
