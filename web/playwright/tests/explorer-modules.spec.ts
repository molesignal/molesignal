import { expect, test, type Locator, type Page } from '@playwright/test';

import { explorerRead, explorerReadBody } from '../fixtures/explorerReads';
import { serveLogCounts, serveTraceCounts } from '../fixtures/histogramCounts';
import { installMockShellSession } from '../fixtures/mockSession';

const LOG_STREAM = {
  id: 'application-logs',
  name: 'application_logs',
  streamType: 'logs',
  schema: {
    fields: [
      { name: '_timestamp', dataType: 'timestamp', nullable: false, indexed: true },
      { name: 'level', dataType: 'utf8', nullable: true, indexed: true },
      { name: 'message', dataType: 'utf8', nullable: true, indexed: false },
    ],
  },
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
};

const LOG_MESSAGE = 'checkout completed';

async function serveLogs(page: Page, rows: number) {
  await installMockShellSession(page);
  const now = Date.now();
  await page.route('**/api/v1/streams**', (route) => route.fulfill({ json: [LOG_STREAM] }));
  await page.route(explorerRead('web/logs'), (route) =>
    route.fulfill({
      json: {
        // The newest rows of the last hour, a second apart, one in four an error.
        items: Array.from({ length: rows }, (_, index) => ({
          _timestamp: (now - (index + 2) * 1_000) * 1_000,
          level: index % 4 === 3 ? 'ERROR' : 'INFO',
          message: LOG_MESSAGE,
        })),
        nextCursor: null,
        previousCursor: null,
        hasMore: false,
      },
    }),
  );
  await page.route('**/api/v1/field_masking/effective/**', (route) =>
    route.fulfill({ json: { fields: [] } }),
  );
  // A statement gets a result, if an empty one: the page is not built to read the
  // catch-all's `{}` as rows, and a test of the query bar should not turn on that.
  await page.route(explorerRead('query'), (route) =>
    route.fulfill({ json: { columns: [], rows: [], scannedRows: 0, tookMs: 0 } }),
  );
}

/** `spacingMs` apart, newest first; by default spread over the last few hours of the 24h window. */
async function serveTraces(page: Page, count: number, spacingMs = 20 * 60 * 1000) {
  await installMockShellSession(page);
  const now = Date.now();
  await page.route(/\/api\/v1\/web\/traces(\?.*)?$/, (route) =>
    route.fulfill({
      json: {
        items: Array.from({ length: count }, (_, index) => ({
          traceId: `trace-${index}`,
          operation: 'POST /checkout',
          service: 'checkout-api',
          startNs: (now - (index + 1) * spacingMs) * 1_000_000,
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

/** The spans of the Spans view, and their count over time as the RED figures give it, a bar for every bar asked. */
async function serveSpans(page: Page, count: number) {
  const now = Date.now();
  const asked: URLSearchParams[] = [];
  await page.route('**/api/v1/web/traces/spans**', (route) =>
    route.fulfill({
      json: {
        items: Array.from({ length: count }, (_, index) => ({
          traceId: `trace-${index}`,
          spanId: `span-${index}`,
          service: 'checkout-api',
          operation: 'POST /checkout',
          startNs: (now - (index + 1) * 60_000) * 1_000_000,
          durationNs: 40_000_000,
          status: 'OK',
        })),
        nextCursor: null,
        previousCursor: null,
        hasMore: false,
      },
    }),
  );
  await page.route(/\/api\/v1\/web\/traces\/red(\?.*)?$/, (route) => {
    const params = new URL(route.request().url()).searchParams;
    asked.push(params);
    const to = Number(params.get('to'));
    const bucket = Number(params.get('bucketTime'));
    const origin = Number(params.get('originTime'));
    const figures = { requests: 20, errors: 1, p50Ns: 30_000_000, p95Ns: 90_000_000, p99Ns: 120_000_000 };
    const buckets = Array.from({ length: Math.ceil((to - origin) / bucket) }, (_, index) => ({ startTime: origin + index * bucket, ...figures }));
    return route.fulfill({ json: { bucketTime: bucket, originTime: origin, scope: 'all', total: figures, buckets, latency: [] } });
  });
  return asked;
}

/** Where the frame of a histogram begins and ends, and how long a bar of it is, in microseconds. */
async function frameOf(histogram: Locator) {
  const frame = histogram.locator('[data-histogram-frame]');
  const [start, end, bar] = await Promise.all(
    ['data-frame-start', 'data-frame-end', 'data-bar-micros'].map(async (name) =>
      Number(await frame.getAttribute(name)),
    ),
  );

  return { start: start!, end: end!, bar: bar! };
}

const SECOND = 1_000_000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;

const centreY = async (locator: Locator) => {
  const box = (await locator.boundingBox())!;
  return box.y + box.height / 2;
};

test.describe('Logs explorer modules', () => {
  test('the histogram is a module: closed from the results row, remembered, reopened', async ({
    page,
  }) => {
    await serveLogs(page, 3);
    await page.goto('/logs');
    await expect(page.getByText(LOG_MESSAGE).first()).toBeVisible();

    const actions = page.locator('[data-log-result-actions]');
    const histogram = page.locator('[data-explorer-module="histogram"]');
    await expect(histogram).toBeVisible();
    await expect(actions.getByRole('button', { name: 'Hide histogram' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );

    await actions.getByRole('button', { name: 'Hide histogram' }).click();
    await expect(histogram).toHaveCount(0);
    const reopen = actions.getByRole('button', { name: 'Show histogram' });
    await expect(reopen).toHaveAttribute('aria-pressed', 'false');
    // Closed is still a gray button, not a different colour: a sign without a word, like its neighbours.
    await expect(reopen).toHaveText('');

    // The choice outlives the visit.
    await page.reload();
    await expect(page.getByText(LOG_MESSAGE).first()).toBeVisible();
    await expect(histogram).toHaveCount(0);

    await actions.getByRole('button', { name: 'Show histogram' }).click();
    await expect(histogram).toBeVisible();
  });

  test('counts every row the query matches across the queried range, and reads as a histogram', async ({
    page,
  }) => {
    await serveLogs(page, 12);
    // Forty rows a bar, four of them errors, and a burst in the bar at the middle.
    const asked = await serveLogCounts(page, (index, bars) => ({
      total: index === Math.floor(bars / 2) ? 950 : 40,
      errors: 4,
    }));
    await page.goto('/logs');
    const histogram = page.locator('[data-explorer-module="histogram"]');
    await expect(histogram).toHaveAttribute('data-histogram-counting', 'all');

    // The count was asked once, of the query the list ran, for the bars the chart had settled on.
    expect(asked).toHaveLength(1);
    const { start, end, bar } = await frameOf(histogram);
    expect(asked[0]).toMatchObject({
      stream: 'application_logs',
      filters: [],
      freeText: [],
      bucketTime: bar,
      originTime: start,
    });
    expect(asked[0]!.to - asked[0]!.from).toBe(HOUR);
    // The frame is the hour that was queried, widened to whole bars of a length a reader
    // counts in, on the clock: not the twelve seconds the loaded rows span.
    expect([30 * SECOND, MINUTE]).toContain(bar);
    expect(start % bar).toBe(0);
    expect(end % bar).toBe(0);
    expect(asked[0]!.from - start).toBeGreaterThanOrEqual(0);
    expect(asked[0]!.from - start).toBeLessThan(bar);
    expect(end - asked[0]!.to).toBeGreaterThanOrEqual(0);
    expect(end - asked[0]!.to).toBeLessThan(bar);

    // A bar for every stretch that holds something, and the count of them all said once.
    const bars = (end - start) / bar;
    await expect(histogram.locator('[data-histogram-bar]')).toHaveCount(bars);
    await expect(histogram.locator('[data-histogram-scope]')).toHaveText(
      `All ${(40 * (bars - 1) + 950).toLocaleString('en-US')} results that match`,
    );
    // Whole counts from zero, to a line the burst does not pass.
    expect(await histogram.locator('[data-histogram-axis="count"] span').allInnerTexts()).toEqual([
      '0',
      '500',
      '1K',
    ]);
    // Times of day on the edges of bars; the day and the length of a bar are said once under them.
    const times = await histogram.locator('[data-histogram-axis="time"] .type-micro').allInnerTexts();
    expect(times.length).toBeGreaterThanOrEqual(3);
    for (const time of times) expect(time).toMatch(/^(\d{2}:\d{2}(:\d{2})?|\d{2}-\d{2})$/);
    const frame = histogram.locator('[data-histogram-frame]');
    await expect(frame.locator('time').first()).toHaveText(/^\d{4}-\d{2}-\d{2}$/);
    await expect(frame).toContainText(/(30 s|1 min) per bar$/);

    // Closing it is the results row's job; a control on the chart would sit on a bar. What a
    // bar holds is read out above the plot, not on it.
    const plot = histogram.locator('[data-histogram-plot]');
    await histogram.locator('[data-histogram-bar]').nth(3).hover();
    await expect(histogram.getByRole('button')).toHaveCount(0);
    const readout = page.getByRole('tooltip');
    await expect(readout).toContainText('Non-error');
    await expect(readout).toContainText('Errors');
    const [plotBox, readoutBox] = await Promise.all([plot.boundingBox(), readout.boundingBox()]);
    expect(readoutBox!.y + readoutBox!.height).toBeLessThanOrEqual(plotBox!.y);
  });

  test('agrees with the list when the count is of the rows the list holds', async ({ page }) => {
    await serveLogs(page, 0);
    const now = Date.now();
    // Eight rows seven minutes apart, all inside the hour that is queried.
    const items = Array.from({ length: 8 }, (_, index) => ({
      _timestamp: (now - (index * 7 + 1) * 60_000) * 1_000,
      level: index % 4 === 3 ? 'ERROR' : 'INFO',
      message: LOG_MESSAGE,
    }));
    await page.route(explorerRead('web/logs'), (route) =>
      route.fulfill({ json: { items, nextCursor: null, previousCursor: null, hasMore: false } }),
    );
    await serveLogCounts(page, {
      events: items.map((item) => ({ micros: item._timestamp, error: item.level === 'ERROR' })),
    });
    await page.goto('/logs');

    const histogram = page.locator('[data-explorer-module="histogram"]');
    await expect(histogram).toHaveAttribute('data-histogram-counting', 'all');
    await expect(page.getByText(LOG_MESSAGE)).toHaveCount(8);
    // The same eight, a bar each, across the hour instead of the fifty minutes they span.
    await expect(histogram.locator('[data-histogram-scope]')).toHaveText('All 8 results that match');
    await expect(histogram.locator('[data-histogram-bar]')).toHaveCount(8);
    const { start, end } = await frameOf(histogram);
    expect((end - start) / HOUR).toBeGreaterThanOrEqual(1);
  });

  test('draws the loaded rows, and says why, when everything cannot be counted', async ({ page }) => {
    await serveLogs(page, 12);
    await serveLogCounts(page, { unavailable: 'time_budget' });
    await page.goto('/logs');
    const histogram = page.locator('[data-explorer-module="histogram"]');
    await expect(histogram).toHaveAttribute('data-histogram-counting', 'loaded');

    await expect(histogram.locator('[data-histogram-scope]')).toHaveText(
      'The 12 results that are loaded: counting all that match took too long',
    );
    // The bars span the rows that are loaded, not the hour they were asked from: twelve rows
    // a second apart, in bars of a second or less.
    const { start, end, bar } = await frameOf(histogram);
    expect((end - start) / SECOND).toBeLessThan(20);
    expect(start % bar).toBe(0);
    expect(end % bar).toBe(0);
    await expect(histogram.locator('[data-histogram-bar]')).toHaveCount(12);
    expect(await histogram.locator('[data-histogram-axis="count"] span').allInnerTexts()).toEqual(['0', '1']);
    await expect(histogram.locator('[data-histogram-frame]')).toContainText(/\d+ (ms|s) per bar$/);
  });

  test('keeps to the rows a statement returned: a statement is not counted', async ({ page }) => {
    await serveLogs(page, 3);
    const asked = await serveLogCounts(page, () => ({ total: 40, errors: 0 }));
    const now = Date.now();
    await page.route(explorerRead('query'), (route) =>
      route.fulfill({
        json: {
          columns: ['_timestamp', 'level', 'message'],
          rows: Array.from({ length: 5 }, (_, index) => [
            (now - (index + 2) * 1_000) * 1_000,
            'INFO',
            LOG_MESSAGE,
          ]),
          scannedRows: 5,
          tookMs: 1,
        },
      }),
    );
    await page.goto(
      '/logs?sql=' + encodeURIComponent('SELECT * FROM "application_logs" ORDER BY _timestamp DESC LIMIT 5'),
    );

    const histogram = page.locator('[data-explorer-module="histogram"]');
    await expect(histogram).toHaveAttribute('data-histogram-counting', 'loaded');
    await expect(histogram.locator('[data-histogram-scope]')).toHaveText('The 5 results that are loaded');
    await expect(histogram.locator('[data-histogram-bar]')).toHaveCount(5);
    expect(asked).toHaveLength(0);
  });

  test('counts a query once: another page of it is not counted again, running it again is', async ({
    page,
  }) => {
    await serveLogs(page, 3);
    const asked = await serveLogCounts(page, () => ({ total: 40, errors: 4 }));
    const now = Date.now();
    // Two pages of the same query, the second reached by its cursor.
    await page.route(explorerRead('web/logs'), (route) => {
      const second = Boolean(explorerReadBody(route.request()).cursor);

      return route.fulfill({
        json: {
          items: Array.from({ length: 3 }, (_, index) => ({
            _timestamp: (now - (index + (second ? 40 : 2)) * 1_000) * 1_000,
            level: 'INFO',
            message: second ? 'older rows' : LOG_MESSAGE,
          })),
          nextCursor: second ? null : 'page-2',
          previousCursor: second ? 'page-1' : null,
          hasMore: !second,
        },
      });
    });
    await page.goto('/logs');
    const histogram = page.locator('[data-explorer-module="histogram"]');
    const scope = histogram.locator('[data-histogram-scope]');
    await expect(histogram).toHaveAttribute('data-histogram-counting', 'all');
    const counted = await scope.innerText();
    expect(asked).toHaveLength(1);

    await page.getByRole('button', { name: 'Next' }).click();
    await expect(page.getByText('older rows').first()).toBeVisible();
    // Other rows under the same chart: what the query matches has not changed.
    await expect(histogram).toHaveAttribute('data-histogram-counting', 'all');
    await expect(scope).toHaveText(counted);
    expect(asked).toHaveLength(1);

    await page.getByRole('button', { name: 'Run', exact: true }).click();
    await expect.poll(() => asked.length).toBe(2);
    await expect(histogram).toHaveAttribute('data-histogram-counting', 'all');
  });

  test('draws no empty histogram band when the query returns no rows', async ({ page }) => {
    await serveLogs(page, 0);
    const asked = await serveLogCounts(page, () => ({ total: 40, errors: 0 }));
    await page.goto('/logs');
    await expect(page.locator('[data-log-result-summary]')).toBeVisible();

    // The module is on; there is simply nothing to chart, and so nothing to count.
    await expect(
      page.locator('[data-log-result-actions]').getByRole('button', { name: 'Hide histogram' }),
    ).toHaveAttribute('aria-pressed', 'true');
    await expect(page.locator('[data-explorer-module="histogram"]')).toHaveCount(0);
    expect(asked).toHaveLength(0);
  });

  test('the field list folds from its own head, and the results have no second switch for it', async ({ page }) => {
    await serveLogs(page, 3);
    await page.goto('/logs');
    const panel = page.locator('aside[data-variant="utility"]');

    await expect(page.locator('[data-log-result-actions]').getByRole('button', { name: /fields/i })).toHaveCount(0);
    await panel.getByRole('button', { name: 'Collapse fields' }).click();
    await expect(panel.locator('[data-log-field-row]')).toHaveCount(0);
    await panel.getByRole('button', { name: 'Expand fields' }).click();
    await expect(panel.locator('[data-log-field-row]').first()).toBeVisible();
  });
});

test.describe('Traces explorer modules', () => {
  test('draws a histogram that can be closed, remembered and reopened', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await serveTraces(page, 12);
    await page.goto('/traces');

    const head = page.locator('[data-trace-results-head]');
    const histogram = head.locator('[data-explorer-module="histogram"]');
    await expect(head.getByText('12 traces loaded')).toBeVisible();
    await expect(histogram).toBeVisible();

    const hide = head.getByRole('button', { name: 'Hide histogram' });
    await expect(hide).toHaveAttribute('aria-pressed', 'true');
    await hide.click();
    await expect(histogram).toHaveCount(0);
    await expect(head.getByRole('button', { name: 'Show histogram' })).toHaveAttribute(
      'aria-pressed',
      'false',
    );

    // The choice outlives the visit, and it is Traces' own: Logs keeps its histogram.
    await page.reload();
    await expect(head.getByText('12 traces loaded')).toBeVisible();
    await expect(histogram).toHaveCount(0);

    await head.getByRole('button', { name: 'Show histogram' }).click();
    await expect(histogram).toBeVisible();
  });

  test('heads both views of the results the same way, the histogram across the results', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await serveTraces(page, 12);
    await serveTraceCounts(page, () => ({ total: 30, errors: 2 }));
    await serveSpans(page, 8);
    const head = page.locator('[data-trace-results-head]');
    const band = head.locator('[data-explorer-module="histogram"]');
    const box = async (path: string, pane: string) => {
      await page.goto(path);
      await expect(band).toHaveAttribute('data-histogram-counting', 'all');
      const [found, results] = await Promise.all([band.boundingBox(), page.locator(`[data-workspace-pane="${pane}"]`).boundingBox()]);
      return { band: found!, results: results! };
    };

    // The band stands in the results it heads, in the Traces view and the Spans view alike.
    const traces = await box('/traces', 'trace-results');
    expect(traces.band.x).toBeGreaterThanOrEqual(traces.results.x);
    expect(traces.band.x + traces.band.width).toBeLessThanOrEqual(traces.results.x + traces.results.width + 0.5);

    const spans = await box('/traces?tab=spans', 'span-results');
    expect([spans.band.x, spans.band.y, spans.band.width, spans.band.height].map(Math.round)).toEqual(
      [traces.band.x, traces.band.y, traces.band.width, traces.band.height].map(Math.round),
    );
  });

  test('counts each view its own way once, and again on a refresh', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await serveTraces(page, 12);
    const asked = await serveTraceCounts(page, () => ({ total: 30, errors: 2 }));
    const red = await serveSpans(page, 8);
    await page.goto('/traces');

    const head = page.locator('[data-trace-results-head]');
    const histogram = head.locator('[data-explorer-module="histogram"]');
    await expect(histogram).toHaveAttribute('data-histogram-counting', 'all');
    expect(asked).toHaveLength(1);

    // The Spans view counts spans, from the figures the RED view reads; the traces are not counted again.
    await page.getByRole('tab', { name: /^Spans/ }).click();
    await expect(page).toHaveURL(/[?&]tab=spans/);
    await expect(histogram).toHaveAttribute('data-histogram-counting', 'all');
    expect(red.length).toBeGreaterThan(0);
    expect(asked).toHaveLength(1);

    // Back on the traces, the count they had stands. It is the view /traces opens on, so its address names no tab.
    await page.getByRole('tab', { name: /^Traces/ }).click();
    await expect(page).not.toHaveURL(/[?&]tab=/);
    await expect(histogram).toHaveAttribute('data-histogram-counting', 'all');
    expect(asked).toHaveLength(1);

    await page.getByRole('button', { name: 'Refresh' }).click();
    await expect.poll(() => asked.length).toBe(2);
    await expect(histogram).toHaveAttribute('data-histogram-counting', 'all');
  });

  test('counts every trace the query matches across the day that was queried', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    // Twenty traces within sixteen seconds are listed; the day they were asked from holds many more.
    await serveTraces(page, 20, 800);
    const asked = await serveTraceCounts(page, (index) => (index % 3 === 0 ? null : { total: 60, errors: 5 }));
    const listed: URLSearchParams[] = [];
    page.on('request', (request) => {
      const url = new URL(request.url());
      if (url.pathname.endsWith('/api/v1/web/traces')) listed.push(url.searchParams);
    });
    await page.goto('/traces?service=checkout-api');

    const head = page.locator('[data-trace-results-head]');
    const histogram = head.locator('[data-explorer-module="histogram"]');
    await expect(head.getByText('20 traces loaded')).toBeVisible();
    await expect(histogram).toHaveAttribute('data-histogram-counting', 'all');

    // The count is asked once, with the range and the filter the list itself sent, for the
    // bars the chart had settled on.
    expect(asked).toHaveLength(1);
    const { start, end, bar } = await frameOf(histogram);
    const list = listed.at(-1)!;
    expect(asked[0]!.get('from')).toBe(list.get('from'));
    expect(asked[0]!.get('to')).toBe(list.get('to'));
    expect(asked[0]!.get('filters')).toBe(list.get('filters'));
    expect(JSON.parse(asked[0]!.get('filters')!)).toEqual([
      { field: 'service.name', op: '=', value: 'checkout-api' },
    ]);
    expect(Number(asked[0]!.get('bucketTime'))).toBe(bar);
    expect(Number(asked[0]!.get('originTime'))).toBe(start);

    // The frame is the day, in whole bars of a length a reader counts in, on the clock:
    // not the sixteen seconds the listed traces span.
    expect([15 * MINUTE, 30 * MINUTE]).toContain(bar);
    expect(start % bar).toBe(0);
    expect(end % bar).toBe(0);
    expect((end - start) / HOUR).toBeGreaterThanOrEqual(24);
    expect((end - start) / HOUR).toBeLessThanOrEqual(25);

    // Two bars in three hold traces, and the count of them all is said once.
    const bars = (end - start) / bar;
    const held = bars - Math.ceil(bars / 3);
    await expect(histogram.locator('[data-histogram-bar]')).toHaveCount(held);
    await expect(histogram.locator('[data-histogram-scope]')).toHaveText(
      `All ${(held * 60).toLocaleString('en-US')} results that match`,
    );
    expect(await histogram.locator('[data-histogram-axis="count"] span').allInnerTexts()).toEqual([
      '0',
      '20',
      '40',
      '60',
    ]);
    // The axis says times of day, and the day where one begins; the days are said under it.
    const times = await histogram.locator('[data-histogram-axis="time"] .type-micro').allInnerTexts();
    expect(times.length).toBeGreaterThanOrEqual(3);
    for (const time of times) expect(time).toMatch(/^(\d{2}:\d{2}|\d{2}-\d{2})$/);
    await expect(histogram.locator('[data-histogram-frame] time').first()).toHaveText(/^\d{4}-\d{2}-\d{2}$/);
    await expect(histogram.getByRole('button')).toHaveCount(0);
  });

  test('spans the traces that are listed, and says why, when everything cannot be counted', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    // Twenty traces within sixteen seconds, asked for from the last 24 hours.
    await serveTraces(page, 20, 800);
    await serveTraceCounts(page, { unavailable: 'scan_limit' });
    await page.goto('/traces');

    const head = page.locator('[data-trace-results-head]');
    const histogram = head.locator('[data-explorer-module="histogram"]');
    await expect(head.getByText('20 traces loaded')).toBeVisible();
    await expect(histogram).toHaveAttribute('data-histogram-counting', 'loaded');
    await expect(histogram.locator('[data-histogram-scope]')).toHaveText(
      'The 20 results that are loaded: the range holds too many rows to count all that match',
    );

    const { start, end, bar } = await frameOf(histogram);
    const seconds = (end - start) / SECOND;
    // The traces' own stretch and a little margin: seconds, not the day that was queried.
    expect(seconds).toBeGreaterThan(15);
    expect(seconds).toBeLessThan(20);
    // The frame is whole bars of a length a reader counts in, on the clock.
    expect([200_000, 500_000]).toContain(bar);
    expect(start % bar).toBe(0);
    expect(end % bar).toBe(0);
    // The day is said once, under the axis; the axis itself says times of day.
    await expect(histogram.locator('[data-histogram-frame] time')).toHaveText(/^\d{4}-\d{2}-\d{2}$/);
    const times = await histogram.locator('[data-histogram-axis="time"] .type-micro').allInnerTexts();
    expect(times.length).toBeGreaterThanOrEqual(3);
    for (const time of times) expect(time).toMatch(/^\d{2}:\d{2}:\d{2}$/);
    expect(await histogram.locator('[data-histogram-axis="count"] span').first().innerText()).toBe('0');
    await expect(histogram.getByRole('button')).toHaveCount(0);
  });

  test('narrows the time range to the bars a drag crosses', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await serveTraces(page, 20, 800);
    const counted = await serveTraceCounts(page, () => ({ total: 30, errors: 2 }));
    const asked: URL[] = [];
    page.on('request', (request) => {
      const url = new URL(request.url());
      if (url.pathname.endsWith('/api/v1/web/traces')) asked.push(url);
    });
    await page.goto('/traces');

    const histogram = page.locator('[data-trace-results-head] [data-explorer-module="histogram"]');
    const plot = histogram.locator('[data-histogram-plot]');
    await expect(histogram).toHaveAttribute('data-histogram-counting', 'all');
    const { start, end, bar } = await frameOf(histogram);
    const box = (await plot.boundingBox())!;
    const before = asked.length;

    // From a quarter of the way across to the middle.
    await page.mouse.move(box.x + box.width * 0.25, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width * 0.5, box.y + box.height / 2, { steps: 6 });
    await expect(histogram.locator('[data-histogram-selection]')).toBeVisible();
    // While a stretch is being marked the readout stays out of the way.
    await expect(page.getByRole('tooltip')).toHaveCount(0);
    await page.mouse.up();
    await expect(histogram.locator('[data-histogram-selection]')).toHaveCount(0);

    // The traces are asked for again, from the first bar crossed to the end of the last.
    await expect.poll(() => asked.length).toBeGreaterThan(before);
    const range = asked.at(-1)!.searchParams;
    const [from, to] = [Number(range.get('from')), Number(range.get('to'))];
    const quarter = (end - start) / 4;
    expect(from % bar).toBe(0);
    expect(to % bar).toBe(0);
    expect(Math.abs(from - (start + quarter))).toBeLessThanOrEqual(bar);
    expect(Math.abs(to - (start + quarter * 2))).toBeLessThanOrEqual(bar);
    // And they are counted again, in the narrower range.
    await expect.poll(() => counted.length).toBe(2);
    expect(Number(counted[1]!.get('from'))).toBe(from);
    expect(Number(counted[1]!.get('to'))).toBe(to);
  });

  test('lists a trace as a row from edge to edge with a line under it', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await serveTraces(page, 6);
    await page.route(/\/api\/v1\/web\/trace\/[^/?]+(\?.*)?$/, (route) =>
      route.fulfill({
        json: {
          traceId: 'trace-1',
          rootSpanId: 'root',
          truncated: false,
          spans: [
            {
              spanId: 'root',
              parentSpanId: null,
              service: 'checkout-api',
              operation: 'POST /checkout',
              startNs: 0,
              endNs: 41_000_000,
              status: 'OK',
              resource: { attributes: {} },
              attributes: {},
              events: [],
            },
          ],
        },
      }),
    );
    await page.goto('/traces');

    const list = page.getByTestId('trace-table').locator('[data-trace-row]').first().locator('xpath=..');
    const head = page.locator('[data-trace-table-head]');
    const rows = page.locator('[data-trace-row]');
    await expect(rows).toHaveCount(6);

    const [pane, band, ...boxes] = await Promise.all([list.boundingBox(), head.boundingBox(), ...[0, 1, 2].map((index) => rows.nth(index).boundingBox())]);
    const [first, second, third] = boxes.map((box) => box!);
    // No inset and no gap: a row is the list's width, the first begins under the head, and the next where it ends.
    expect(first!.x).toBe(pane!.x);
    expect(first!.width).toBe(pane!.width);
    expect(first!.y).toBe(band!.y + band!.height);
    expect(second!.y).toBe(first!.y + first!.height);
    expect(third!.y).toBe(second!.y + second!.height);
    // One line of one height, the line under it included.
    expect(first!.height).toBe(32);
    expect(second!.height).toBe(first!.height);
    for (const index of [0, 1, 5]) {
      await expect(rows.nth(index)).toHaveCSS('border-bottom-width', '1px');
    }

    // A row opens its trace in the drawer and stands out from the others while it is open.
    const fill = (row: Locator) => row.evaluate((element) => getComputedStyle(element).backgroundColor);
    await rows.nth(1).click();
    await expect(rows.nth(1)).toHaveAttribute('aria-pressed', 'true');
    await expect.poll(async () => (await fill(rows.nth(1))) !== (await fill(rows.nth(2)))).toBe(true);
    const drawer = page.getByRole('dialog', { name: 'Trace detail' });
    await expect(drawer).toBeVisible();

    // Closed, the drawer gives the row the focus back, and the row is one of the list again.
    await page.keyboard.press('Escape');
    await expect(drawer).toHaveCount(0);
    await expect(rows.nth(1)).toBeFocused();
    await expect(rows.nth(1)).toHaveAttribute('aria-pressed', 'false');
  });

  test('shows the same module in the trace table view', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await serveTraces(page, 12);
    await page.goto('/traces');

    const results = page.locator('[data-workspace-pane="trace-results"]');
    await expect(results.locator('[data-explorer-module="histogram"]')).toBeVisible();
    await expect(results.getByRole('button', { name: 'Hide histogram' }).first()).toBeVisible();
  });

  test('draws no histogram band when no traces are loaded', async ({ page }) => {
    await serveTraces(page, 0);
    const asked = await serveTraceCounts(page, () => ({ total: 30, errors: 2 }));
    await page.goto('/traces');

    const head = page.locator('[data-trace-results-head]');
    await expect(head.getByText('0 traces loaded')).toBeVisible();
    await expect(head.getByRole('button', { name: 'Hide histogram' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    await expect(head.locator('[data-explorer-module="histogram"]')).toHaveCount(0);
    expect(asked).toHaveLength(0);
  });
});

test.describe('Explorer query row', () => {
  test('Logs opens with its query folded into the toolbar row', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await serveLogs(page, 3);
    await page.goto('/logs');

    const workbench = page.locator('[data-query-workbench-appearance="surface"]');
    // Folded by default, as on Traces: one row, the summary level with the time range.
    await expect(workbench).toHaveAttribute('data-query-body', 'inline');
    const summary = workbench.getByRole('button', { name: 'Expand query' });
    const timeRange = workbench.getByRole('button', { name: /^Time range:/ });
    expect(Math.abs((await centreY(summary)) - (await centreY(timeRange)))).toBeLessThanOrEqual(2);
    const collapsedHeight = (await workbench.boundingBox())!.height;
    expect(collapsedHeight).toBeLessThanOrEqual(56);

    // Opening it gives the editor its own row; closing it folds it back.
    await summary.click();
    await expect(workbench).toHaveAttribute('data-query-body', 'stacked');
    expect((await workbench.boundingBox())!.height).toBeGreaterThan(collapsedHeight);
    await workbench.getByRole('button', { name: 'Collapse query' }).click();
    await expect(workbench).toHaveAttribute('data-query-body', 'inline');
  });

  test('Logs opens with the editor open when the link brings a query', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await serveLogs(page, 3);
    await page.goto('/logs?sql=' + encodeURIComponent('SELECT * FROM "application_logs"'));

    const workbench = page.locator('[data-query-workbench-appearance="surface"]');
    await expect(workbench).toHaveAttribute('data-query-body', 'stacked');
    await expect(workbench.getByRole('button', { name: 'Collapse query' })).toBeVisible();
  });

  test('the explorers carry their name in the bar instead of a title row', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await serveLogs(page, 3);

    const titleBox = async (path: string, name: string) => {
      await page.goto(path);
      const title = page.locator(
        '[data-query-workbench-appearance="surface"] [data-workbench-title]',
      );
      await expect(title.getByRole('heading', { level: 1, name })).toBeVisible();
      await expect(title.getByTestId('page-header-module-icon')).toBeVisible();
      await expect(page.getByTestId('page-header')).toHaveCount(0);
      return (await title.boundingBox())!;
    };

    const logs = await titleBox('/logs', 'Logs');
    // The panes start right under the single bar, which holds the query tabs.
    const workspace = (await page.locator('[data-log-workspace-layout]').boundingBox())!;
    expect(workspace.y).toBeLessThanOrEqual(166);

    const traces = await titleBox('/traces', 'Traces');
    // Same place on both pages, so switching explorers does not move the name.
    expect(Math.abs(traces.x - logs.x)).toBeLessThanOrEqual(1);
    expect(Math.abs(traces.y - logs.y)).toBeLessThanOrEqual(1);
  });

  test('Traces opens on a single-row query bar', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await installMockShellSession(page);
    await page.goto('/traces');

    const workbench = page.locator('[data-query-workbench-appearance="surface"]');
    await expect(workbench).toHaveAttribute('data-query-body', 'inline');
    const summary = workbench.getByRole('button', { name: 'Expand query' });
    const run = workbench.getByRole('button', { name: 'Query', exact: true });
    expect(Math.abs((await centreY(summary)) - (await centreY(run)))).toBeLessThanOrEqual(2);
    expect((await workbench.boundingBox())!.height).toBeLessThanOrEqual(56);
    // The folded query keeps a readable width beside its tabs; its summary does not shrink to a stub.
    const folded = workbench.locator('[data-query-editor-state="collapsed"]');
    expect((await folded.boundingBox())!.width).toBeGreaterThanOrEqual(320);
    expect((await summary.locator('code').boundingBox())!.width).toBeGreaterThanOrEqual(96);
  });

  test('falls back to a row of its own when the toolbar has no room', async ({ page }) => {
    await page.setViewportSize({ width: 1100, height: 800 });
    await installMockShellSession(page);
    await page.goto('/traces');

    const workbench = page.locator('[data-query-workbench-appearance="surface"]');
    await expect(workbench).toHaveAttribute('data-query-body', 'stacked');
    const summary = workbench.getByRole('button', { name: 'Expand query' });
    const run = workbench.getByRole('button', { name: 'Query', exact: true });
    // Below the controls, full width, as before.
    expect((await summary.boundingBox())!.y).toBeGreaterThan((await run.boundingBox())!.y + 20);
  });
});
