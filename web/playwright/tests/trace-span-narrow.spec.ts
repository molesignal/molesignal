/**
 * Traces where room is short, and the Span tab.
 *
 * The Span tab lists the spans a query matches, and a span opens its trace at that span. Beside a
 * trace in the drawer, the detail of a span leaves the names of the spans their room; where the
 * drawer has no room for both, the detail lies over the waterfall. The trace's id is said once.
 */
import type { Locator, Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { serveExplorerResults } from '../fixtures/mockExplorers';

/** Room for a page to load: a full load from the dev server takes several seconds when the suite runs. */
const LOADED = { timeout: 15_000 };

const START_NS = (Date.parse(FROZEN_NOW_ISO) - 10 * 60 * 1000) * 1_000_000;

/** A request into a second service, its spans named as a database and a query engine name them, three deep. */
function trace(traceId: string) {
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
    attributes: { 'db.system': 'postgresql' },
    events: [],
  });

  return {
    traceId: traceId,
    rootSpanId: 'request',
    truncated: false,
    spans: [
      span('request', null, 'router', 'GET /api/v1/web/trace/{trace_id}', 0, 419),
      span('catalog', 'request', 'router', 'SELECT iam_catalog_versions', 16, 34),
      span('organizations', 'request', 'router', 'SELECT organizations', 36, 48),
      span('federated', 'request', 'querier', 'query.federated', 60, 317),
      span('datafusion', 'federated', 'querier', 'query.datafusion', 61, 316),
      span('streams', 'datafusion', 'querier', 'SELECT logical_streams', 70, 83),
      span('datasets', 'datafusion', 'querier', 'SELECT physical_datasets', 90, 100),
    ],
  };
}

async function serveTrace(page: Page) {
  await page.route(/\/api\/v1\/web\/trace\/[^/?]+(\?.*)?$/, (route) => {
    const traceId = decodeURIComponent(new URL(route.request().url()).pathname.split('/').pop() ?? '');
    return route.fulfill({ json: trace(traceId) });
  });
}

async function box(locator: Locator) {
  const found = await locator.boundingBox();
  expect(found).not.toBeNull();
  return found!;
}

/** Opens the latest trace of the Traces view in the drawer, and waits for its spans. */
async function openTrace(page: Page) {
  const row = page.locator('[data-trace-row]').first();
  await expect(row).toBeVisible(LOADED);
  await row.click();
  const drawer = page.getByRole('dialog', { name: 'Trace detail' });
  await expect(drawer.getByTestId('trace-span-row')).toHaveCount(7, LOADED);
  return drawer;
}

/** Picks a span from the keyboard, which opens its detail. */
async function pickSpan(page: Page, scope: Locator, operation: string) {
  const select = scope.getByRole('button', { name: new RegExp(`^Select span ${operation},`) });
  await select.focus();
  await page.keyboard.press('Enter');
  await expect(select).toHaveAttribute('aria-pressed', 'true');
  const panel = scope.getByTestId('trace-span-panel');
  await expect(panel.getByRole('heading', { level: 2, name: new RegExp(operation) })).toBeVisible();
  return panel;
}

/** The spans whose names are cut short: a part of the row that ends on an ellipsis holds more than it shows. */
async function cutNames(scope: Locator) {
  await scope.page().evaluate(() => document.fonts.ready);
  return scope.getByTestId('trace-span-row').evaluateAll((rows) =>
    rows.flatMap((row) => {
      const parts = [...row.querySelectorAll<HTMLElement>('*')].filter(
        (part) => getComputedStyle(part).textOverflow === 'ellipsis',
      );
      return parts.some((part) => part.scrollWidth > part.clientWidth + 1) ? [row.textContent ?? ''] : [];
    }),
  );
}

test.describe('Traces where room is short, and the Span tab', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await serveExplorerResults(page);
    await serveTrace(page);
  });

  test('the Span tab lists the spans that match, and a span opens its trace at that span', async ({ page }) => {
    const spans = trace('trace-narrow').spans;
    const listed = ['organizations', 'datasets'].map((id) => spans.find((span) => span.spanId === id)!);
    await page.route('**/api/v1/web/traces/spans**', (route) =>
      route.fulfill({
        json: {
          items: listed.map((span) => ({
            traceId: 'trace-narrow',
            spanId: span.spanId,
            parentSpanId: span.parentSpanId,
            service: span.service,
            operation: span.operation,
            startNs: span.startNs,
            durationNs: span.endNs - span.startNs,
            status: 'OK',
            kind: 3,
          })),
          nextCursor: null,
          previousCursor: null,
          hasMore: false,
        },
      }),
    );
    await page.goto('/traces?tab=spans');

    const rows = page.locator('[data-span-list-row]');
    await expect(rows).toHaveCount(2, LOADED);
    await expect(rows.nth(1)).toContainText('SELECT physical_datasets');
    await rows.nth(1).click();

    const drawer = page.getByRole('dialog', { name: 'Trace detail' });
    await expect(drawer.getByTestId('trace-span-row')).toHaveCount(7, LOADED);
    await expect(drawer.getByTestId('trace-span-panel').getByRole('heading', { level: 2, name: /SELECT physical_datasets/ })).toBeVisible();
    await expect(rows.nth(1)).toHaveAttribute('aria-pressed', 'true');
  });

  test('beside a trace in the drawer, the detail of a span leaves the names their room', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/traces');
    const drawer = await openTrace(page);

    await pickSpan(page, drawer, 'SELECT organizations');
    await expect(drawer.locator('[data-span-panel-place]')).toHaveAttribute('data-span-panel-place', 'beside');
    expect(await cutNames(drawer)).toEqual([]);
  });

  test('where the drawer has no room for both, the detail of a span lies over the waterfall', async ({ page }) => {
    await page.setViewportSize({ width: 1024, height: 720 });
    await page.goto('/traces');
    const drawer = await openTrace(page);
    const viewport = drawer.getByTestId('trace-waterfall-viewport');
    const before = (await box(viewport)).width;

    const panel = await pickSpan(page, drawer, 'SELECT organizations');
    await expect(drawer.locator('[data-span-panel-place]')).toHaveAttribute('data-span-panel-place', 'over');
    // The waterfall keeps its width under the detail.
    expect(Math.abs((await box(viewport)).width - before)).toBeLessThanOrEqual(1);

    await panel.getByRole('button', { name: 'Hide span detail' }).click();
    await expect(drawer.getByTestId('trace-span-panel')).toHaveCount(0);
  });

  test("the trace's own page says its id once", async ({ page }) => {
    await page.goto('/traces/trace-narrow');
    await expect(page.getByTestId('trace-span-row')).toHaveCount(7, LOADED);

    await expect(page.getByText('trace-narrow', { exact: true })).toHaveCount(1);
    await expect(page.getByTestId('trace-id')).toHaveText('trace-narrow');
  });
});
