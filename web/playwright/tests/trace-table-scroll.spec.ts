/**
 * The trace and span tables wider than their pane.
 *
 * A reader can put fields of each trace's root span beside the columns a trace carries. Past the
 * pane's width a table scrolls sideways as one piece, its head and rows together: every column can
 * be reached, none is cut off at the edge of a row, and the head stays over the rows. The columns
 * the trace table opens with fit a 1280px window without scrolling, the rows' scrollbar beside them.
 */
import type { Locator, Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { expectExplorerResults, serveExplorerResults } from '../fixtures/mockExplorers';

// Scrollbars drawn as a person's browser draws them: the app's own take room beside the rows.
test.use({ viewport: { width: 1280, height: 720 }, launchOptions: { ignoreDefaultArgs: ['--hide-scrollbars'] } });

const NOW_NS = Date.parse(FROZEN_NOW_ISO) * 1_000_000;
const ROOT_FIELDS = { 'http.route': '/api/orders/{order_id}/items', 'http.request.method': 'POST', kind: 'server' };

/** Forty traces, each with the fields of its root span a reader can add as columns. */
async function serveTraces(page: Page) {
  await page.route(/\/api\/v1\/web\/traces(\?.*)?$/, (route) =>
    route.fulfill({
      json: {
        items: Array.from({ length: 40 }, (_, nth) => ({
          traceId: (nth + 1).toString(16).padStart(32, '0'),
          operation: 'POST /api/orders',
          service: 'checkout-api',
          startNs: NOW_NS - nth * 60_000_000_000,
          durationMs: 40 + nth,
          spanCount: 6,
          errorCount: 0,
          httpStatusCode: 201,
          fields: ROOT_FIELDS,
        })),
        nextCursor: null,
        previousCursor: null,
        hasMore: false,
      },
    }),
  );
}

/** Forty spans of one service, for the Span tab. */
async function serveSpans(page: Page) {
  await page.route(/\/api\/v1\/web\/traces\/spans(\?.*)?$/, (route) =>
    route.fulfill({
      json: {
        items: Array.from({ length: 40 }, (_, nth) => ({
          traceId: (nth + 1).toString(16).padStart(32, '0'),
          spanId: `span-${nth}`,
          service: 'querier',
          operation: 'SELECT physical_datasets',
          startNs: NOW_NS - nth * 60_000_000_000,
          durationNs: (nth + 1) * 7_000_000,
          status: 'OK',
          kind: 3,
        })),
        nextCursor: null,
        previousCursor: null,
        hasMore: false,
      },
    }),
  );
}

/** How far the cells of a row stand from those of the head, at most: the outcome's edge is no cell. */
const misalignment = async (row: Locator, head: Locator) =>
  row.evaluate((element, other) => {
    const edges = (of: Element) =>
      [...of.children]
        .filter((cell) => getComputedStyle(cell).position !== 'absolute')
        .flatMap((cell) => {
          const box = cell.getBoundingClientRect();
          return [box.left, box.right];
        });
    const [mine, theirs] = [edges(element), edges(other!)];
    if (mine.length !== theirs.length) return Infinity;
    return Math.max(...mine.map((edge, index) => Math.abs(edge - theirs[index]!)));
  }, await head.elementHandle());

/** How much of what a box holds lies past its edge: a row cuts it off there. */
const spill = (locator: Locator) => locator.evaluate((element) => element.scrollWidth - element.clientWidth);

test('opens with columns that fit a 1280px window, beside the scrollbar of its rows', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await serveExplorerResults(page);
  await page.goto('/traces');
  await expectExplorerResults(page, 'traces');

  const scroller = page.locator('[data-trace-table-head]').locator('xpath=..');
  const room = await scroller.evaluate((element: HTMLElement) => ({ bar: element.offsetWidth - element.clientWidth, sideways: element.scrollWidth - element.clientWidth }));
  // A page of rows scrolls down, so their scrollbar takes its room; what is left holds every column.
  expect(room.bar).toBeGreaterThan(0);
  expect(room.sideways).toBe(0);
});

test('scrolls sideways as one piece once fields of the root span are added', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await serveExplorerResults(page);
  await serveTraces(page);
  await page.goto('/traces');
  await expectExplorerResults(page, 'traces');

  const head = page.locator('[data-trace-table-head]');
  const columns = head.locator(':scope > *');
  await page.getByRole('button', { name: 'Columns' }).click();
  for (const [index, field] of Object.keys(ROOT_FIELDS).entries()) {
    await page.getByRole('menuitemcheckbox', { name: field, exact: true }).click();
    await expect(columns).toHaveCount(10 + index);
  }
  await page.keyboard.press('Escape');

  const scroller = head.locator('xpath=..');
  const rows = page.locator('[data-trace-row]');
  const last = rows.first().locator(':scope > *').last();
  // Wider than its pane, the table scrolls; nothing lies past the edge of the head or of a row.
  expect(await scroller.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeGreaterThan(0);
  expect(await spill(head)).toBe(0);
  for (const index of [0, 1, 2]) {
    expect(await spill(rows.nth(index))).toBe(0);
  }
  expect(await misalignment(rows.first(), head)).toBeLessThanOrEqual(0.5);
  await expect(last).not.toBeInViewport({ ratio: 1 });

  // Scrolled to its end, the last column is in the pane under its name, and the head still stands over the rows.
  await scroller.evaluate((element) => {
    element.scrollLeft = element.scrollWidth;
  });
  await expect(columns.last()).toBeInViewport({ ratio: 1 });
  await expect(last).toBeInViewport({ ratio: 1 });
  await expect(last).toHaveText('server');
  expect(await misalignment(rows.first(), head)).toBeLessThanOrEqual(0.5);

  const top = (await head.boundingBox())!.y;
  await scroller.evaluate((element) => {
    element.scrollTop = 400;
  });
  expect((await head.boundingBox())!.y).toBe(top);
  expect(await misalignment(rows.nth(15), head)).toBeLessThanOrEqual(0.5);
});

test('walks its rows from the keyboard, none left under the head', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await serveExplorerResults(page);
  await page.goto('/traces');
  await expectExplorerResults(page, 'traces');

  const head = page.locator('[data-trace-table-head]');
  const rows = page.locator('[data-trace-row]');
  await rows.first().focus();
  for (let step = 0; step < 20; step++) {
    await page.keyboard.press('ArrowDown');
  }
  await expect(rows.nth(20)).toBeFocused();

  // The row above it scrolled halfway under the head, the arrow up brings it out from under it.
  await head.locator('xpath=..').evaluate((scroller) => {
    const band = scroller.querySelector('[data-trace-table-head]')!;
    const row = scroller.querySelectorAll('[data-trace-row]')[19]!;
    scroller.scrollTop += row.getBoundingClientRect().top - band.getBoundingClientRect().bottom + 16;
  });
  await page.keyboard.press('ArrowUp');
  await expect(rows.nth(19)).toBeFocused();
  const [band, row] = await Promise.all([head.boundingBox(), rows.nth(19).boundingBox()]);
  expect(row!.y).toBeGreaterThanOrEqual(band!.y + band!.height - 0.5);
});

test('the span table scrolls sideways the same way where its pane is narrower than its columns', async ({ page, mockServer }) => {
  await page.setViewportSize({ width: 1024, height: 720 });
  await mountMockRoutes(page, mockServer.port);
  await serveExplorerResults(page);
  await serveSpans(page);
  await page.goto('/traces?tab=spans');

  const head = page.locator('[data-span-table-head]');
  const scroller = head.locator('xpath=..');
  const rows = page.locator('[data-span-list-row]');
  await expect(rows).toHaveCount(40, { timeout: 15_000 });

  expect(await scroller.evaluate((element) => element.scrollWidth - element.clientWidth)).toBeGreaterThan(0);
  expect(await spill(rows.first())).toBe(0);
  await scroller.evaluate((element) => {
    element.scrollLeft = element.scrollWidth;
  });
  const trace = rows.first().locator(':scope > *').last();
  await expect(trace).toBeInViewport({ ratio: 1 });
  await expect(trace).toHaveText('00000000000000000000000000000001');
  expect(await misalignment(rows.first(), head)).toBeLessThanOrEqual(0.5);
});
