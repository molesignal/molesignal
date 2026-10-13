import type { Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const HOUR = 3_600_000_000;
const DAY = 24 * HOUR;
/** A tenth of a second, in the nanoseconds a CPU profile counts in. */
const S = 100_000_000;

// Both periods summed, and how the second differs from the first. The baseline adds up to 4 s and
// the period to 6 s; of their own, database.query went from 1.5 s to 4.5 s, cache.lookup from
// 1.6 s to 400 ms, json.parse from 400 ms to 600 ms. runtime.main itself did not change.
const DIFF_FLAME = {
  names: ['total', 'runtime.main', 'database.query', 'cache.lookup', 'json.parse'],
  // offset, both summed, self summed, name, change
  levels: [
    [0, 100 * S, 0, 0, 20 * S],
    [0, 100 * S, 10 * S, 1, 20 * S],
    [0, 60 * S, 60 * S, 2, 30 * S, 0, 20 * S, 20 * S, 3, -12 * S, 0, 10 * S, 10 * S, 4, 2 * S],
  ],
  numTicks: 100 * S,
  maxSelf: 60 * S,
  maxAbsDelta: 30 * S,
  units: 'nanoseconds',
};

type Asked = Record<string, string>;

/** Serves the comparison and records what each one was asked for. */
async function serveCompare(
  page: Page,
  options: { baselineCount?: number; services?: string[] } = {},
): Promise<Asked[]> {
  const asked: Asked[] = [];
  const services = options.services ?? ['checkout', 'payments'];
  const profiles = services.map((service, index) => ({
    id: `cpu-${index}`,
    service,
    profile_type: 'cpu',
    timestamp: NOW - (index + 1) * 60_000_000,
    total_value: 25 * S,
    sample_count: 100,
    duration_nanos: 60_000_000_000,
    unsymbolized: false,
  }));

  await page.route(/\/api\/v1\/profiles(?:\?.*)?$/, (route) => route.fulfill({ json: { profiles } }));
  await page.route(/\/api\/v1\/profiles\/diff(?:\?.*)?$/, (route) => {
    asked.push(Object.fromEntries(new URL(route.request().url()).searchParams));

    return route.fulfill({
      json: {
        flamebearer: DIFF_FLAME,
        truncated: false,
        baseline_count: options.baselineCount ?? 12,
        comparison_count: 12,
      },
    });
  });

  return asked;
}

const span = (asked: Asked | undefined, from: string, to: string) =>
  asked ? Number(asked[to]) - Number(asked[from]) : Number.NaN;

test.describe('Profile comparison', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('compares the period with the one before it as soon as it opens', async ({ page }) => {
    const asked = await serveCompare(page);
    await page.goto('/profiles/compare');

    const statement = page.locator('[data-compare-statement]');
    await expect(statement.locator('[data-compare-overall]')).toHaveText('Overall change+2 s · +50%');
    // Nothing to fill in and run first: every control has a value to start from.
    await expect(page.getByRole('button', { name: 'Run diff' })).toHaveCount(0);

    // The last hour, against the hour before it, of the first type there are profiles of.
    const first = asked.at(-1)!;
    expect(span(first, 'from', 'to')).toBe(HOUR);
    expect(first.baseline_to).toBe(first.from);
    expect(span(first, 'baseline_from', 'baseline_to')).toBe(HOUR);
    expect(first.type).toBe('cpu');

    // What each period holds is said beside it.
    await expect(statement.locator('[data-compare-period="comparison"]')).toContainText('12 profiles');
    await expect(statement.locator('[data-compare-period="comparison"]')).toContainText('6 s');
    await expect(statement.locator('[data-compare-period="baseline"]')).toContainText('4 s');

    // The answer comes before the graph: which functions changed, the largest change first.
    const rows = page.locator('[data-compare-function]');
    await expect(rows).toHaveCount(3);
    await expect(rows.nth(0)).toHaveText('database.query1.5 s4.5 s+3 s+200%');
    await expect(rows.nth(1)).toHaveText('cache.lookup1.6 s400 ms-1.2 s-75%');
    await expect(rows.nth(2)).toHaveText('json.parse400 ms600 ms+200 ms+50%');
    await expect(page.locator('[data-profile-view="flame"]')).toBeVisible();
  });

  test('a function picked in the table keeps its row in its columns and is marked in the graph', async ({
    page,
  }) => {
    await serveCompare(page);
    await page.goto('/profiles/compare');
    const rows = page.locator('[data-compare-function]');
    const lefts = (selector: string) =>
      page
        .locator(selector)
        .evaluateAll((cells) => cells.map((cell) => Math.round(cell.getBoundingClientRect().left)));

    await rows.nth(0).click();

    await expect(rows.nth(0)).toHaveAttribute('data-state', 'selected');
    // The mark of the picked row is part of its first cell. As a box of its own in the row it was
    // given a cell by the table, and every cell of the picked row stood one column to the right.
    const picked = await lefts('[data-compare-function][data-state="selected"] td');
    expect(picked).toEqual(await lefts('[data-compare-changed] thead th'));
    expect(picked).toEqual(await lefts('[data-compare-function]:not([data-state]) >> nth=0 >> td'));
    await expect(
      page.locator('[data-profile-view] button[aria-label="Analyze function database.query"]'),
    ).toHaveClass(/ring-indigo/);
  });

  test('both periods are set freely, from controls of one width', async ({ page }) => {
    const asked = await serveCompare(page);
    await page.goto('/profiles/compare');
    await expect(page.locator('[data-compare-overall]')).toBeVisible();

    const controls = page.locator('[data-compare-control] button');
    const baseline = page.locator('[data-compare-baseline]');
    const panel = page.locator('[data-compare-baseline-panel]');
    const widths = () =>
      controls.evaluateAll((elements) => elements.map((element) => element.getBoundingClientRect().width));
    const sameWidth = async () => {
      const [period, against] = await widths();
      expect(period).toBe(against);
    };

    await expect(controls).toHaveCount(2);
    await sameWidth();

    // A baseline that follows the period: the same hour a week earlier.
    await baseline.click();
    await expect(panel.getByRole('button', { name: 'The period before' })).toHaveAttribute('aria-pressed', 'true');
    await panel.getByRole('button', { name: 'Same time a week earlier' }).click();
    await expect.poll(() => span(asked.at(-1), 'baseline_from', 'from')).toBe(7 * DAY);
    expect(span(asked.at(-1), 'baseline_from', 'baseline_to')).toBe(HOUR);
    await sameWidth();

    // The quick ranges the period itself is set with.
    await baseline.click();
    await panel.getByRole('button', { name: 'Last 6 hours' }).click();
    await expect(baseline).toHaveText('Last 6 hours');
    await expect.poll(() => span(asked.at(-1), 'baseline_from', 'baseline_to')).toBe(6 * HOUR);
    // The last six hours hold the last hour: said, since what is in both counts on both sides.
    await expect(page.locator('[data-compare-overlap]')).toBeVisible();
    await sameWidth();

    // A start and an end of its own. The fields begin from the baseline as it stands.
    await baseline.click();
    await panel.getByRole('button', { name: 'Apply' }).click();
    await expect(baseline).toHaveText('A period of its own');
    await expect(baseline).toHaveAttribute('data-compare-baseline', 'window');
    await sameWidth();

    // The period, with the time range every page has.
    await controls.first().click();
    await page.locator('[data-time-picker-surface]').last().getByRole('button', { name: 'Last 24 hours' }).click();
    await expect.poll(() => span(asked.at(-1), 'from', 'to')).toBe(24 * HOUR);
    await expect(controls.first()).toHaveText('Last 24 hours');
    await sameWidth();
  });

  test('says there is nothing to compare against, and draws no graph of it', async ({ page }) => {
    await serveCompare(page, { baselineCount: 0 });
    await page.goto('/profiles/compare');

    const missing = page.locator('[data-compare-missing="baseline"]');
    await expect(missing).toContainText('Nothing to compare against');
    // With an empty baseline every function is new: a graph of that is one colour and says nothing.
    await expect(page.locator('[data-profile-view]')).toHaveCount(0);
    await expect(page.locator('[data-compare-changed]')).toHaveCount(0);
    await expect(page.locator('[data-compare-overall]')).toHaveCount(0);
    await expect(page.locator('[data-compare-period="baseline"]')).toContainText('0 profiles');
    await expect(missing.getByRole('link', { name: 'Open this period in the analysis' })).toHaveAttribute(
      'href',
      '/profiles',
    );
  });

  test('the breadcrumb stands as far from the top bar as from its title', async ({ page }) => {
    await serveCompare(page);
    await page.goto('/profiles/compare');
    await expect(page.locator('[data-compare-statement]')).toBeVisible();

    const gaps = await page.evaluate(() => {
      const header = document.querySelector('[data-testid="page-header"]')!;
      const crumb = document.createRange();
      crumb.selectNodeContents(header.querySelector('nav a')!);
      const line = crumb.getBoundingClientRect();

      return {
        above: line.top - document.querySelector('header')!.getBoundingClientRect().bottom,
        below: header.querySelector('[data-testid="page-header-module-icon"]')!.getBoundingClientRect().top - line.bottom,
      };
    });

    // At 4px above and 12px below it sat against the bar and looked dropped there.
    expect(Math.abs(gaps.above - gaps.below)).toBeLessThanOrEqual(1);
    expect(gaps.above).toBeGreaterThanOrEqual(6);
  });

  test('a filter is as wide as its longest choice, and its list as wide as the filter', async ({ page }) => {
    await serveCompare(page);
    await page.goto('/profiles/compare');
    const service = page.locator('[data-profile-service-select]');
    const list = page.locator('[data-fitted-select-list]');
    await expect(service).toHaveText('All services');

    const width = (await service.boundingBox())!.width;
    // Short names leave the row their room: the filter is not held at its widest.
    expect(width).toBeLessThan(200);

    await service.click();
    await expect(list.getByRole('option')).toHaveText(['All services', 'checkout', 'payments']);
    // The list opens with a scale that a box read mid-way would carry: its own width is asked for.
    expect(await list.evaluate((element) => Number.parseFloat(getComputedStyle(element).width))).toBeCloseTo(width, 0);
    expect(
      await list
        .locator('[data-fitted-label]')
        .evaluateAll((labels) => labels.filter((label) => label.scrollWidth > label.clientWidth).length),
    ).toBe(0);
  });

  test('a filter stops at its widest, and says a name it cuts under the pointer', async ({ page }) => {
    const long = 'payments-gateway-settlement-reconciliation-worker-eu-central-1';
    await serveCompare(page, { services: ['checkout', long] });
    await page.goto(`/profiles/compare?service=${long}`);
    const service = page.locator('[data-profile-service-select]');
    await expect(service).toContainText('payments-gateway');

    expect((await service.boundingBox())!.width).toBe(240);
    await service.hover();
    await expect(service).toHaveAttribute('title', long);

    await service.click();
    const list = page.locator('[data-fitted-select-list]');
    expect(await list.evaluate((element) => Number.parseFloat(getComputedStyle(element).width))).toBe(240);
    const row = list.getByRole('option', { name: long });
    await row.hover();
    await expect(row).toHaveAttribute('title', long);
  });
});
