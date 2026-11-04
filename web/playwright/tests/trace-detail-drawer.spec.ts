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
  id: 'trace-drawer',
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

/** Enough traces that the list scrolls. */
const TRACES = Array.from({ length: 60 }, (_, index) => ({
  traceId: `trace-${String(index).padStart(2, '0')}`,
  service: 'checkout-api',
  operation: index % 2 === 0 ? 'POST /api/orders' : 'GET /api/orders',
  startNs: START_NS - index * 1_000_000_000,
  durationMs: 40 + index,
  spanCount: 3,
  errorCount: 0,
}));

/** A trace as its own page loads it: the request, the service it called and the query under that. */
function traceDetail(traceId: string) {
  const listed = TRACES.find((trace) => trace.traceId === traceId) ?? TRACES[0]!;
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
    startNs: listed.startNs + fromMs * 1_000_000,
    endNs: listed.startNs + toMs * 1_000_000,
    status: 'OK',
    resource: { attributes: {} },
    attributes: {},
    events: [],
  });

  return {
    traceId: traceId,
    rootSpanId: 'root',
    truncated: false,
    spans: [
      span('root', null, listed.service, listed.operation, 0, listed.durationMs),
      span('call', 'root', 'inventory', 'GET /stock', 4, 20),
      span('query', 'call', 'postgres', 'SELECT stock', 6, 12),
    ],
  };
}

async function boundingBox(locator: Locator) {
  const box = await locator.boundingBox();
  expect(box).not.toBeNull();
  return box!;
}

/** The Traces view, scrolled a long way down the list to `trace-30`, whose row opens it in the drawer. */
async function openTrace(page: Page) {
  await page.goto(`/traces?q=${encodeURIComponent("service.name = 'checkout-api'")}`);

  const rows = page.locator('[data-trace-row]');
  await expect(rows).toHaveCount(TRACES.length);
  const row = page.locator('[data-trace-row][data-trace-id="trace-30"]');
  await row.scrollIntoViewIfNeeded();
  await row.click();

  const list = row.locator('xpath=..');
  const scrollTop = await list.evaluate((element) => element.scrollTop);
  // The row is far enough down that the list had to scroll to it.
  expect(scrollTop).toBeGreaterThan(0);

  const drawer = page.getByRole('dialog', { name: 'Trace detail' });
  await expect(drawer.getByTestId('trace-id')).toHaveText('trace-30');

  return { row, list, scrollTop, drawer };
}

test.beforeEach(async ({ page }) => {
  await installMockShellSession(page);
  await page.route('**/api/v1/streams**', (route) => route.fulfill({ json: [TRACE_STREAM] }));
  await page.route('**/api/v1/web/topology**', (route) =>
    route.fulfill({ json: { nodes: [], edges: [] } }),
  );
  await page.route(/\/api\/v1\/web\/traces(\?.*)?$/, (route) =>
    route.fulfill({
      json: { items: TRACES, nextCursor: null, previousCursor: null, hasMore: false },
    }),
  );
  await page.route('**/api/v1/web/traces/histogram**', (route) =>
    route.fulfill({ json: { bucketTime: 60_000_000, originTime: 0, buckets: [], split: true } }),
  );
  await page.route('**/api/v1/web/trace/*', (route) => {
    const traceId = decodeURIComponent(new URL(route.request().url()).pathname.split('/').pop() ?? '');
    return route.fulfill({ json: traceDetail(traceId) });
  });
});

test('opens a trace from its row in a drawer most of the page wide, and closes back to the list as it was', async ({ page }) => {
  const address = page.url();
  const { row, list, scrollTop, drawer } = await openTrace(page);

  // The trace's waterfall, as on its own page.
  await expect(drawer.getByTestId('trace-span-row')).toHaveCount(3);
  await expect(row).toHaveAttribute('aria-pressed', 'true');
  // A click on a span's row opens the detail of the span beside the waterfall.
  const spanRow = drawer.getByTestId('trace-span-row').filter({ hasText: 'SELECT stock' });
  const spanRowBox = await boundingBox(spanRow);
  await spanRow.click({ position: { x: spanRowBox.width - 24, y: spanRowBox.height / 2 } });
  await expect(drawer.getByTestId('trace-span-panel').getByRole('heading', { name: /SELECT stock/ })).toBeVisible();
  // The trace's own page is a link away.
  await expect(drawer.getByRole('link', { name: "Open the trace's own page" })).toHaveAttribute('href', /^\/traces\/trace-30\?/);

  // Four fifths of the window from its right edge, under the top bar; measured once it has slid in.
  const viewport = page.viewportSize()!;
  await expect.poll(async () => Math.round((await boundingBox(drawer)).x)).toBe(Math.round(viewport.width * 0.2));
  const box = await boundingBox(drawer);
  expect(Math.abs(box.width - viewport.width * 0.8)).toBeLessThanOrEqual(1);
  expect(Math.abs(box.x + box.width - viewport.width)).toBeLessThanOrEqual(1);
  const topbar = await page.evaluate(() =>
    Number.parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--topbar-h')),
  );
  expect(Math.abs(box.y - topbar)).toBeLessThanOrEqual(1);
  expect(Math.abs(box.y + box.height - viewport.height)).toBeLessThanOrEqual(1);

  await page.keyboard.press('Escape');

  await expect(drawer).toHaveCount(0);
  await expect(row).toBeFocused();
  expect(await list.evaluate((element) => element.scrollTop)).toBe(scrollTop);
  expect(page.url()).not.toBe(address);
  await expect(page).toHaveURL(/\/traces\?/);
});

test('steps to the traces beside it, closes from its button, and leads to the trace page', async ({ page }) => {
  const { row, drawer } = await openTrace(page);

  // J and K walk the list, as the arrows of the head do.
  await drawer.getByTestId('trace-span-row').first().click({ position: { x: 200, y: 8 } });
  await page.keyboard.press('j');
  await expect(drawer.getByTestId('trace-id')).toHaveText('trace-31');
  await page.keyboard.press('k');
  await expect(drawer.getByTestId('trace-id')).toHaveText('trace-30');
  await drawer.getByRole('button', { name: 'Previous in the list (K)' }).click();
  await expect(drawer.getByTestId('trace-id')).toHaveText('trace-29');
  await drawer.getByRole('button', { name: 'Next in the list (J)' }).click();
  await expect(drawer.getByTestId('trace-id')).toHaveText('trace-30');

  await drawer.getByRole('button', { name: 'Close trace details' }).click();
  await expect(drawer).toHaveCount(0);
  await expect(row).toBeFocused();

  await row.click();
  await drawer.getByRole('link', { name: "Open the trace's own page" }).click();

  await expect(page).toHaveURL(/\/traces\/trace-30\?/);
  await expect(page.getByTestId('trace-id')).toHaveText('trace-30');
  await expect(page.getByRole('dialog')).toHaveCount(0);
});

test('takes the width its left edge is dragged to, and keeps it for the next trace', async ({ page }) => {
  const { row, drawer } = await openTrace(page);
  const edge = drawer.getByRole('separator', { name: 'Resize the trace drawer' });
  const before = await boundingBox(drawer);

  // From the keyboard, a step at a time: the drawer grows to the left.
  await edge.focus();
  await page.keyboard.press('ArrowLeft');
  await expect.poll(async () => Math.round((await boundingBox(drawer)).width)).toBeGreaterThan(Math.round(before.width));
  const widened = Math.round((await boundingBox(drawer)).width);

  await page.keyboard.press('Escape');
  await expect(drawer).toHaveCount(0);
  await row.click();
  await expect(drawer.getByTestId('trace-id')).toHaveText('trace-30');
  await expect.poll(async () => Math.round((await boundingBox(drawer)).width)).toBe(widened);
});
