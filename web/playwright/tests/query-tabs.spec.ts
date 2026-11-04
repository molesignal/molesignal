import type { Page } from '@playwright/test';

import { explorerRead, explorerReadBody, isExplorerRead } from '../fixtures/explorerReads';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

/** The explorer in front; the ones behind stay mounted but are not shown. */
function front(page: Page) {
  return page.locator('[data-query-tab-panel][data-active="true"]');
}

/** The row of query tabs. It scrolls sideways when they do not fit. */
function strip(page: Page) {
  return page.getByRole('tablist', { name: 'Open queries' });
}

function tabs(page: Page) {
  return strip(page).getByRole('tab');
}

/** How much of each end of the strip is faded out, the hint that tabs go on beyond it. */
function fades(page: Page) {
  return strip(page).evaluate((element) => {
    const style = getComputedStyle(element);
    return {
      start: style.getPropertyValue('--tabs-fade-start').trim(),
      end: style.getPropertyValue('--tabs-fade-end').trim(),
    };
  });
}

/**
 * Waits until the strip has stopped moving: nothing in it is on its way
 * anywhere, and it is not scrolling. The running mark pulses for as long as
 * its query runs; that goes nowhere.
 */
async function stripSettled(page: Page) {
  await expect
    .poll(() =>
      strip(page).evaluate(
        (element) =>
          new Promise<boolean>((resolve) => {
            const before = element.scrollLeft;
            const moving = () =>
              element
                .getAnimations({ subtree: true })
                .some((animation) => animation.effect?.getTiming().iterations !== Infinity);
            requestAnimationFrame(() =>
              requestAnimationFrame(() => resolve(!moving() && element.scrollLeft === before)),
            );
          }),
      ),
    )
    .toBe(true);
}

/** Holds back every answer to `url` until the function it returns is called. */
async function hold(page: Page, url: RegExp) {
  let answer = () => {};
  const held = new Promise<void>((resolve) => {
    answer = resolve;
  });
  await page.route(url, async (route) => {
    await held;
    await route.fallback();
  });
  return () => answer();
}

/** How far the tab in front reaches past either edge of what the strip shows; nothing, when all of it is in view. */
function cutOff(page: Page) {
  return strip(page).evaluate((element) => {
    const shown = element.getBoundingClientRect();
    const tab = element.querySelector('[data-tab-active="true"]')!.getBoundingClientRect();
    return Math.max(0, shown.left - tab.left, tab.right - shown.right);
  });
}

const LOG_STREAM = {
  id: 'application-logs',
  name: 'application_logs',
  streamType: 'logs',
  schema: {
    fields: [
      { name: '_timestamp', dataType: 'timestamp', nullable: false, indexed: true },
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

/**
 * Opens Logs with `count` query tabs in a window too narrow for them. The
 * shared mock lists no stream; here there is one, which every tab picks and is
 * then named after.
 */
async function openLogTabs(page: Page, count: number) {
  await page.route('**/api/v1/streams**', (route) => route.fulfill({ json: [LOG_STREAM] }));
  await page.route(explorerRead('web/logs'), (route) =>
    route.fulfill({
      json: {
        items: [{ _timestamp: (Date.now() - 2_000) * 1_000, message: 'checkout completed' }],
        nextCursor: null,
        previousCursor: null,
        hasMore: false,
      },
    }),
  );
  await page.setViewportSize({ width: 640, height: 720 });
  await page.goto('/logs');

  for (let open = 2; open <= count; open += 1) {
    await page.getByRole('button', { name: 'New tab' }).click();
    await expect(tabs(page)).toHaveCount(open);
  }
  await expect(tabs(page).last()).toHaveAttribute('aria-selected', 'true');
  await expect(tabs(page).last()).toHaveAccessibleName(LOG_STREAM.name);
}

/**
 * Picks the last tab once more, now that it has its name: however it was
 * opened, it is whole at the end of the strip afterwards.
 */
async function pickLastTabAgain(page: Page) {
  await tabs(page).first().click();
  await expect(tabs(page).first()).toHaveAttribute('aria-selected', 'true');
  await tabs(page).last().click();
  await expect(tabs(page).last()).toHaveAttribute('aria-selected', 'true');
  await expect.poll(() => cutOff(page)).toBeLessThan(1);
  await stripSettled(page);
}

function runFrontQuery(page: Page) {
  return front(page)
    .locator('[data-query-toolbar]')
    .getByRole('button', { name: 'Run', exact: true })
    .click();
}

function promqlRequests(page: Page): string[] {
  const statements: string[] = [];
  page.on('request', (request) => {
    if (!isExplorerRead(request, 'query')) return;
    const body = explorerReadBody<{ language?: string; statement?: string }>(request);
    if (body.language === 'promql' && body.statement) statements.push(body.statement);
  });
  return statements;
}

async function chartMetric(page: Page, name: string) {
  await front(page).getByRole('button', { name: `Chart ${name}`, exact: true }).click();
  await expect(front(page).getByRole('img', { name: /Metrics time series/i })).toBeVisible({
    timeout: 10_000,
  });
}

test.describe('Query tabs', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('each tab runs its own query and keeps its results while another is in front', async ({
    page,
  }) => {
    const statements = promqlRequests(page);
    await page.goto('/metrics');
    await expect(tabs(page)).toHaveCount(1);

    await chartMetric(page, 'http_requests_total');
    await expect(tabs(page).first()).toContainText('rate(http_requests_total[5m])');

    await page.getByRole('button', { name: 'New tab' }).click();
    await expect(tabs(page)).toHaveCount(2);
    // A new tab starts with nothing in it: no query, no chart, the first tab untouched.
    await expect(front(page).getByTestId('metrics-start')).toBeVisible();
    await expect(page).toHaveURL(/\/metrics$/);

    await chartMetric(page, 'process_cpu_seconds_total');
    await expect(page).toHaveURL(/promql=rate%28process_cpu_seconds_total/);
    const issued = statements.length;

    // Back to the first: its chart is there as it was left, and nothing is asked again.
    await tabs(page).first().click();
    await expect(tabs(page).first()).toHaveAttribute('aria-selected', 'true');
    await expect(front(page).getByRole('img', { name: /Metrics time series/i })).toBeVisible();
    await expect(front(page).locator('[data-metrics-results-header]')).toContainText(
      'rate(http_requests_total[5m])',
    );
    await expect(page).toHaveURL(/promql=rate%28http_requests_total/);
    expect(statements.length).toBe(issued);
    expect(new Set(statements)).toEqual(
      new Set(['rate(http_requests_total[5m])', 'rate(process_cpu_seconds_total[5m])']),
    );

    // The tab behind is not painted and cannot be reached.
    const behind = page.locator('[data-query-tab-panel][data-active="false"]');
    await expect(behind).toHaveCount(1);
    await expect(behind).toHaveCSS('visibility', 'hidden');
  });

  test('each tab keeps its own time range', async ({ page }) => {
    await page.goto('/metrics');
    await page.getByRole('button', { name: 'New tab' }).click();
    await expect(tabs(page)).toHaveCount(2);

    const timeRange = () => front(page).locator('[data-time-range-trigger="true"]');
    await timeRange().click();
    await page.getByRole('button', { name: 'Last 6 hours' }).click();
    await expect(timeRange()).toContainText('Last 6 hours');
    await expect(page).toHaveURL(/time=now-6h\.\.now/);

    await tabs(page).first().click();
    await expect(timeRange()).toContainText('Last 1 hour');
    await expect(page).not.toHaveURL(/time=/);

    await tabs(page).nth(1).click();
    await expect(timeRange()).toContainText('Last 6 hours');
    await expect(page).toHaveURL(/time=now-6h\.\.now/);
  });

  test('tabs come back after a reload, and a tab mounts only when it is opened', async ({
    page,
  }) => {
    await page.goto('/metrics');
    await chartMetric(page, 'http_requests_total');
    await page.getByRole('button', { name: 'New tab' }).click();
    await chartMetric(page, 'queue_depth');

    await page.reload();

    await expect(tabs(page)).toHaveCount(2);
    await expect(tabs(page).nth(1)).toHaveAttribute('aria-selected', 'true');
    await expect(front(page).getByRole('img', { name: /Metrics time series/i })).toBeVisible({
      timeout: 10_000,
    });
    // Only the tab in front has queried; the other waits until it is asked for.
    await expect(page.locator('[data-query-tab-panel]')).toHaveCount(1);

    await tabs(page).first().click();
    await expect(page.locator('[data-query-tab-panel]')).toHaveCount(2);
    await expect(front(page).locator('[data-metrics-results-header]')).toContainText(
      'rate(http_requests_total[5m])',
    );
  });

  test('a link with a query opens in a tab of its own', async ({ page }) => {
    await page.goto('/metrics');
    await chartMetric(page, 'http_requests_total');

    await page.goto(`/metrics?promql=${encodeURIComponent('up')}`);

    await expect(tabs(page)).toHaveCount(2);
    await expect(tabs(page).nth(1)).toHaveAttribute('aria-selected', 'true');
    await expect(tabs(page).nth(1)).toContainText('up');
    // The query that was being worked on is still in its tab.
    await expect(tabs(page).first()).toContainText('rate(http_requests_total[5m])');
  });

  test('tabs are closed, renamed and kept to at least one', async ({ page }) => {
    await page.goto('/logs');
    await expect(tabs(page)).toHaveCount(1);
    // The only tab cannot be closed away.
    await expect(page.getByRole('button', { name: /^Close / })).toHaveCount(0);

    await page.getByRole('button', { name: 'New tab' }).click();
    await page.getByRole('button', { name: 'New tab' }).click();
    await expect(tabs(page)).toHaveCount(3);

    await tabs(page).nth(2).dblclick();
    const name = page.getByRole('textbox', { name: 'Tab name' });
    await name.fill('Checkout errors');
    await name.press('Enter');
    await expect(tabs(page).nth(2)).toHaveAccessibleName('Checkout errors');

    await page.getByRole('button', { name: 'Close Checkout errors' }).click();
    await expect(tabs(page)).toHaveCount(2);
    await expect(tabs(page).nth(1)).toHaveAttribute('aria-selected', 'true');
  });

  test('every explorer lists its tabs as blocks in its query box, folded or open', async ({ page }) => {
    // What the bar stands under: the top bar, or the module's tabs on RUM. It is 12px off either.
    const explorers = [
      { path: '/logs', above: page.getByRole('banner') },
      { path: '/traces', above: page.getByRole('banner') },
      { path: '/metrics', above: page.getByRole('banner') },
      { path: '/rum/explorer', above: page.locator('[data-rum-navigation="surface"]') },
    ];
    for (const { path, above } of explorers) {
      await page.goto(path);
      const workbench = front(page).locator('[data-query-workbench-appearance="surface"]');
      const folded = workbench.locator('[data-query-editor-state="collapsed"]');
      await expect(folded).toBeVisible();
      const [aboveBox, workbenchBox] = await Promise.all([above.boundingBox(), workbench.boundingBox()]);
      expect(workbenchBox!.y - (aboveBox!.y + aboveBox!.height)).toBe(12);

      // Folded, the tabs open the frame of the query; the rest of it is the button that opens it.
      await expect(folded.locator('[data-query-tab-strip]')).toBeVisible();
      const expand = folded.getByRole('button', { name: /^Expand query/ });
      await expect(expand.getByRole('tab')).toHaveCount(0);
      await page.getByRole('button', { name: 'New tab' }).click();
      await expect(tabs(page)).toHaveCount(2);
      // The tab in front is a tinted block, the other a plain one.
      const [selected, other] = await Promise.all([
        tabs(page).nth(1).evaluate((tab) => getComputedStyle(tab.closest('[data-query-tab]')!).backgroundColor),
        tabs(page).nth(0).evaluate((tab) => getComputedStyle(tab.closest('[data-query-tab]')!).backgroundColor),
      ]);
      expect(selected).not.toBe(other);
      expect(other).not.toBe('rgba(0, 0, 0, 0)');

      // Open, they stand at the start of its head, where the query's letter or name stood.
      await front(page).getByRole('button', { name: /^Expand query/ }).click();
      await expect(front(page).locator('[data-query-editor-state="expanded"]')).toBeVisible();
      await expect(workbench.locator('[data-query-editor-tabs] [data-query-tab-strip]')).toBeVisible();
      await expect(tabs(page)).toHaveCount(2);
      await expect(page.getByRole('tablist', { name: 'Open queries' })).toHaveCount(1);
    }
  });

  test('the tabs keep the focus when another tab comes forward with its own query box', async ({ page }) => {
    await page.goto('/logs');
    await page.getByRole('button', { name: 'New tab' }).click();
    await expect(tabs(page).nth(1)).toBeFocused();

    // The arrow keys go from tab to tab, each bringing its page forward.
    await page.keyboard.press('ArrowLeft');
    await expect(tabs(page).first()).toHaveAttribute('aria-selected', 'true');
    await expect(tabs(page).first()).toBeFocused();
    await page.keyboard.press('ArrowRight');
    await expect(tabs(page).nth(1)).toHaveAttribute('aria-selected', 'true');
    await expect(tabs(page).nth(1)).toBeFocused();
  });

  // What the strip measured of a tab that has become narrower, or has gone,
  // lags behind it for a moment, reaching past the last tab. That is not a tab
  // beyond the edge: no end of the strip fades for it, then or afterwards.
  test('a tab is not left faded at its end when its query finishes', async ({ page }) => {
    // The answer waits until the strip has settled with the tab's running
    // mark, so the mark goes while the tab was last measured that wide.
    const answer = await hold(page, /\/api\/v1\/web\/traces(\?.*)?$/);
    await page.goto('/traces');
    const running = strip(page).locator('[data-query-tab-running]');
    await expect(running).toBeVisible({ timeout: 10_000 });
    await stripSettled(page);

    answer();
    await expect(running).toHaveCount(0, { timeout: 10_000 });
    await stripSettled(page);
    expect(await fades(page)).toEqual({ start: '0px', end: '0px' });
  });

  test('a tab given a shorter name is not left faded at its end', async ({ page }) => {
    await page.goto('/metrics');
    await tabs(page).first().dblclick();
    // The field a name is typed into is wider than the name.
    const name = page.getByRole('textbox', { name: 'Tab name' });
    await name.fill('p95');
    await stripSettled(page);

    await name.press('Enter');
    await expect(tabs(page).first()).toHaveAccessibleName('p95');
    await stripSettled(page);
    expect(await fades(page)).toEqual({ start: '0px', end: '0px' });
  });

  test('closing the tab in front leaves no fade where it was', async ({ page }) => {
    await page.goto('/metrics');
    await page.getByRole('button', { name: 'New tab' }).click();
    await expect(tabs(page).nth(1)).toHaveAttribute('aria-selected', 'true');
    await stripSettled(page);

    // The tab in front was measured beyond the end of the one that is left.
    await page.getByRole('button', { name: 'Close Query 2' }).click();
    await expect(tabs(page)).toHaveCount(1);
    await stripSettled(page);
    expect(await fades(page)).toEqual({ start: '0px', end: '0px' });
  });

  test('the strip fades the edge that has more tabs beyond it when they do not fit', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 420, height: 800 });
    await page.goto('/logs');
    for (let count = 2; count <= 6; count += 1) {
      await page.getByRole('button', { name: 'New tab' }).click();
      await expect(tabs(page)).toHaveCount(count);
    }
    const hidden = await strip(page).evaluate((element) => element.scrollWidth - element.clientWidth);
    expect(hidden).toBeGreaterThan(0);

    // The newest tab is in front and was brought into view: the rest are beyond the start.
    await expect.poll(() => fades(page)).toEqual({ start: '28px', end: '0px' });

    // Halfway along, there are tabs beyond both ends.
    await strip(page).evaluate((element) =>
      element.scrollTo({ left: (element.scrollWidth - element.clientWidth) / 2 }),
    );
    await expect.poll(() => fades(page)).toEqual({ start: '28px', end: '28px' });

    // Back at the first tab, the others are beyond the end.
    await tabs(page).first().click();
    await expect(tabs(page).first()).toHaveAttribute('aria-selected', 'true');
    await expect.poll(() => fades(page)).toEqual({ start: '0px', end: '28px' });
  });

  // A strip that does not fit is as wide as its row whatever its tabs do. A tab
  // that grows where it stands, at the end of such a strip, grows out of view
  // unless the strip goes with it.
  test('a new tab stays in view when the name it is given makes it wider', async ({ page }) => {
    // The fifth tab is "Query 5" when the strip glides to it, and the name of
    // its stream, half as wide again, before the strip has arrived.
    await openLogTabs(page, 5);

    await expect.poll(() => cutOff(page)).toBeLessThan(1);
    // The end of the strip stops fading once its last tab is whole.
    await expect.poll(() => fades(page)).toEqual({ start: '28px', end: '0px' });
  });

  test('the tab in front stays in view while its query is running', async ({ page }) => {
    await openLogTabs(page, 5);
    await pickLastTabAgain(page);

    // The running mark makes the tab wider, at the very end of the strip.
    const answer = await hold(page, explorerRead('web/logs'));
    await runFrontQuery(page);
    await expect(strip(page).locator('[data-query-tab-running]')).toBeVisible();
    await stripSettled(page);
    await expect.poll(() => cutOff(page)).toBeLessThan(1);
    await expect.poll(() => fades(page)).toEqual({ start: '28px', end: '0px' });

    answer();
    await expect(strip(page).locator('[data-query-tab-running]')).toHaveCount(0);
  });

  test('a strip scrolled away from the tab in front is left where it was put', async ({ page }) => {
    await openLogTabs(page, 5);
    await pickLastTabAgain(page);

    // Back to the first tab with the wheel: the tab in front is beyond the end, and meant to be.
    const shown = (await strip(page).boundingBox())!;
    await page.mouse.move(shown.x + shown.width / 2, shown.y + shown.height / 2);
    await page.mouse.wheel(-2_000, 0);
    const scrolled = () => strip(page).evaluate((element) => element.scrollLeft);
    await expect.poll(scrolled).toBe(0);

    // Its query runs and its tab changes size, out of view. Once the strip has
    // settled, it has not moved.
    const answer = await hold(page, explorerRead('web/logs'));
    await runFrontQuery(page);
    await expect(strip(page).locator('[data-query-tab-running]')).toHaveCount(1);
    await stripSettled(page);
    expect(await scrolled()).toBe(0);

    answer();
    await expect(strip(page).locator('[data-query-tab-running]')).toHaveCount(0);
    await stripSettled(page);
    expect(await scrolled()).toBe(0);
  });
});
