/**
 * The waterfall and the detail of a span, each as dense as it reads best.
 *
 * A row of the waterfall is one line, and names its service only where the trace enters it; the
 * detail of a span says each fact once, a tag on a line, beside the waterfall at the width the
 * reader gives it.
 */
import type { Locator, Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';

/** Room for a page to load: a full load from the dev server takes several seconds when the suite runs. */
const LOADED = { timeout: 15_000 };

const START_NS = (Date.parse(FROZEN_NOW_ISO) - 10 * 60 * 1000) * 1_000_000;

/** Ten tags: more than the eight a span shows before the rest are asked for. */
const QUERY_TAGS = {
  'db.system': 'postgresql',
  'db.name': 'shop',
  'db.operation': 'SELECT',
  'db.statement': 'SELECT id, name FROM users WHERE id = $1',
  'service.name': 'web',
  'service.version': '1.4.2',
  'host.name': 'web-7f9c',
  'process.pid': 4821,
  'thread.id': 17,
  'code.function': 'load_user',
};

/** A request, two queries of its own service, and a call into another service with a query of its own. */
function trace(traceId: string) {
  const span = (
    spanId: string,
    parentSpanId: string | null,
    service: string,
    operation: string,
    fromMs: number,
    toMs: number,
    attributes: Record<string, unknown> = {},
  ) => ({
    spanId: spanId,
    parentSpanId: parentSpanId,
    service,
    operation,
    startNs: START_NS + fromMs * 1_000_000,
    endNs: START_NS + toMs * 1_000_000,
    status: 'OK',
    resource: { attributes: {} },
    attributes,
    events: [],
  });

  return {
    traceId: traceId,
    rootSpanId: 'request',
    truncated: false,
    spans: [
      span('request', null, 'web', 'GET /checkout', 0, 120),
      span('carts', 'request', 'web', 'SELECT carts', 4, 14),
      span('users', 'request', 'web', 'SELECT users', 16, 22, QUERY_TAGS),
      span('orders', 'request', 'orders-api', 'POST /orders', 30, 100),
      span('order-rows', 'orders', 'orders-api', 'SELECT orders', 34, 50),
    ],
  };
}

async function serveTrace(page: Page) {
  await page.route(/\/api\/v1\/web\/trace\/[^/?]+(\?.*)?$/, (route) => {
    const traceId = decodeURIComponent(new URL(route.request().url()).pathname.split('/').pop() ?? '');
    return route.fulfill({ json: trace(traceId) });
  });
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

async function box(locator: Locator) {
  const found = await locator.boundingBox();
  expect(found).not.toBeNull();
  return found!;
}

test.describe('trace waterfall density', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await serveTrace(page);
  });

  test('a row is one line, and names its service where the trace enters it', async ({ page }) => {
    await page.goto('/traces/trace-density');
    const rows = page.getByTestId('trace-span-row');
    await expect(rows).toHaveCount(5, LOADED);

    for (const row of await rows.all()) {
      expect((await box(row)).height).toBeCloseTo(32, 0);
      // The span's name, and its service where there is one, on the row's one line.
      const names = row.locator('[data-signal-type], [data-span-service]');
      const centres = await names.evaluateAll((elements) =>
        elements.map((element) => {
          const rect = element.getBoundingClientRect();
          return rect.top + rect.height / 2;
        }),
      );
      for (const centre of centres) expect(Math.abs(centre - (centres[0] ?? centre))).toBeLessThanOrEqual(1);
      // A reference reads as a name until the pointer or the keyboard is on it.
      for (const decoration of await row
        .locator('[data-signal-type]')
        .evaluateAll((elements) => elements.map((element) => getComputedStyle(element).textDecorationLine))) {
        expect(decoration).toBe('none');
      }
    }

    // The request names its service, and so does the call into another; the spans under each in
    // the same service are known by its colour.
    const services = await rows.evaluateAll((elements) =>
      elements.map((element) => element.querySelector('[data-span-service]')?.textContent ?? null),
    );
    expect(services).toEqual(['web', null, null, 'orders-api', null]);
  });

  test('the detail of a span says each fact once, a tag on a line', async ({ page }) => {
    await page.goto('/traces/trace-density');
    await expect(page.getByTestId('trace-span-row')).toHaveCount(5, LOADED);
    const panel = await pickSpan(page, page.locator('#main'), 'SELECT users');

    expect((await box(panel)).width).toBeCloseTo(420, 0);
    // How long it took, and when it started into the trace, each said once.
    const facts = panel.getByTestId('trace-span-facts');
    await expect(facts.getByText('6 ms', { exact: true })).toHaveCount(1);
    await expect(facts.getByText('+16 ms', { exact: true })).toHaveCount(1);
    // More tags than a glance takes: they can be searched.
    await expect(panel.getByRole('textbox', { name: 'Search tags' })).toBeVisible();

    // A tag is a line: its name beside its value, no taller than the controls that come over it.
    const tags = panel.getByTestId('trace-tag');
    await expect(tags).toHaveCount(10);
    for (const tag of await tags.all()) {
      const [name, value] = [await box(tag.locator('dt')), await box(tag.locator('dd'))];
      expect(Math.abs(name.y + name.height / 2 - (value.y + value.height / 2))).toBeLessThanOrEqual(1);
      expect((await box(tag)).height).toBeLessThanOrEqual(28.5);
    }
  });

  test('the detail of a span takes the width its edge is given, and keeps it for the next span', async ({ page }) => {
    await page.goto('/traces/trace-density');
    await expect(page.getByTestId('trace-span-row')).toHaveCount(5, LOADED);
    const panel = await pickSpan(page, page.locator('#main'), 'SELECT carts');
    const before = (await box(panel)).width;

    // The edge between the waterfall and the detail: the detail grows to the left.
    const edge = page.getByRole('separator', { name: 'Resize the span detail' });
    await edge.focus();
    await page.keyboard.press('ArrowLeft');
    await expect.poll(async () => Math.round((await box(panel)).width)).toBeGreaterThan(Math.round(before));
    const widened = Math.round((await box(panel)).width);

    await pickSpan(page, page.locator('#main'), 'POST /orders');
    await expect.poll(async () => Math.round((await box(panel)).width)).toBe(widened);
  });
});
