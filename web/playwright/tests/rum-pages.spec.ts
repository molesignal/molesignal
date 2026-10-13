import type { Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const PAGE = { next_cursor: null, previous_cursor: null };

function session(index: number, overrides: Record<string, unknown> = {}) {
  return {
    session_id: `session-${index}`,
    user_id: `user-${index}`,
    application: 'storefront',
    environment: 'production',
    version: 'v2.4.0',
    browser: index % 2 === 0 ? 'Chrome' : 'Safari',
    os: 'macOS',
    country: index % 2 === 0 ? 'BR' : 'DE',
    duration_ms: 90_000 + index * 10_000,
    started_at_micros: NOW - (index + 1) * 47 * 60_000_000,
    error_count: 0,
    journey: ['/', '/products', '/checkout'],
    last_page: '/checkout',
    rage_click_count: 0,
    dead_click_count: 0,
    slow_resource_count: 0,
    failed_request_count: 0,
    crash_count: 0,
    experience: 'good',
    replay_available: index % 2 === 0,
    ...overrides,
  };
}

async function serveSessions(page: Page, items: unknown[]) {
  await page.route(/\/api\/v1\/rum\/sessions(?:\?.*)?$/, (route) => route.fulfill({ json: { ...PAGE, items } }));
}

async function useChinese(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem('molesignal-ui-prefs', JSON.stringify({ state: { language: 'zh-cn' }, version: 0 }));
  });
}

test.describe('RUM tabs', () => {
  test('every tab stands under the name of the module, with the time range in the language of the product', async ({
    page,
    mockServer,
  }) => {
    await useChinese(page);
    await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });
    await serveSessions(page, [session(0)]);

    for (const path of ['/rum/sessions', '/rum/performance/apis', '/rum/settings/sdk']) {
      await page.goto(path);
      // The tabs say which page is open: the title is the module's on every one of them.
      await expect(page.getByRole('heading', { level: 1 })).toHaveText('用户体验');
    }

    await page.goto('/rum/sessions');
    // "Last 1h" stood here, in English on a page written in Chinese.
    await expect(page.locator('[data-time-range-trigger="true"]')).toHaveText('近 1 小时');
    // How long ago is said in the product's language too, whatever the browser's is.
    await expect(page.locator('[data-rum-session="session-0"]')).toContainText('47分钟前');
  });

  test('sessions are two lines each, under one row of filters that name themselves', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    await serveSessions(page, [
      session(0),
      session(1, { journey: ['/', '/search'], last_page: '/product/sku-104', rage_click_count: 3, experience: 'poor' }),
      session(2),
    ]);
    await page.goto('/rum/sessions');

    const rows = page.locator('[data-rum-session]');
    await expect(rows).toHaveCount(3);
    // A screen holds a dozen sessions: a row had a picture every row shared and was 95px tall.
    for (const box of await rows.evaluateAll((elements) => elements.map((element) => element.getBoundingClientRect().height))) {
      expect(box).toBeLessThanOrEqual(70);
    }
    // A replay that was recorded is said by a sign on its own row.
    await expect(rows.nth(0).locator('[data-rum-session-replay]')).toHaveCount(1);
    await expect(rows.nth(1).locator('[data-rum-session-replay]')).toHaveCount(0);
    // Where a visit ended is said only where the path shown does not end there.
    await expect(rows.nth(0)).not.toContainText('Last page');
    await expect(rows.nth(1)).toContainText('Last page: /product/sku-104');
    await expect(rows.nth(0)).toContainText('47 minutes ago');

    // One row of controls, none with a caption over it; each as wide as its longest choice.
    const filters = page.locator('[data-rum-filters]');
    expect((await filters.boundingBox())!.height).toBeLessThanOrEqual(52);
    await expect(filters.locator('.type-caption')).toHaveCount(0);
    const country = filters.getByRole('combobox', { name: 'Country' });
    await expect(country).toHaveText('All countries');
    expect((await country.boundingBox())!.width).toBeLessThan(220);

    // A picked value says what it is a value of.
    await country.click();
    await page.getByRole('option', { name: 'DE' }).click();
    await expect(country).toHaveText('CountryDE');
    await expect(rows).toHaveCount(1);
  });

  test('errors open with what was counted, in one line', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    const error = (index: number, status: 'new' | 'ongoing') => ({
      fingerprint: `fp-${index}`,
      message: `Payment provider timeout ${index}`,
      count: 4 + index,
      users: 2,
      sessions: 2,
      first_seen_micros: NOW - 3_600_000_000,
      last_seen_micros: NOW - 60_000_000,
      page: '/checkout',
      version: 'v2.4.0',
      error_type: 'GatewayTimeout',
      trend_pct: 100,
      status,
      recent_sessions: [`s-${index}`, 's-shared'],
      recent_users: [`u-${index}`],
    });
    await page.route(/\/api\/v1\/rum\/errors(?:\?.*)?$/, (route) =>
      route.fulfill({ json: { ...PAGE, items: [error(0, 'new'), error(1, 'ongoing')] } }),
    );
    await page.goto('/rum/errors');

    const facts = page.locator('[data-rum-facts]');
    await expect(facts.locator('[data-fact="errors"]')).toHaveText('Total errors9');
    await expect(facts.locator('[data-fact="sessions"]')).toContainText('3');
    await expect(facts.locator('[data-fact="new"]')).toContainText('1');
    // A line, where four cards took a third of the screen to say as much.
    expect((await facts.boundingBox())!.height).toBeLessThanOrEqual(40);
    await expect(page.locator('[data-rum-filters] .type-caption')).toHaveCount(0);
  });

  test('a vital that was not measured is shown as not measured, and times in the unit they are read in', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    const point = (pageName: string, lcp: number) => ({
      ts_micros: NOW - 60_000_000,
      session_id: `s-${pageName}`,
      page: pageName,
      application: 'storefront',
      lcp_ms: lcp,
      fid_ms: 96,
      inp_ms: null,
      cls: 0.05,
      ttfb_ms: 400,
    });
    await page.route(/\/api\/v1\/rum\/performance\/vitals(?:\?.*)?$/, (route) =>
      route.fulfill({ json: [point('/checkout', 2_720), point('/products', 1_800)] }),
    );
    await page.goto('/rum/pages');

    const row = page.getByRole('row', { name: /\/checkout/ });
    await expect(row).toContainText('2.7 s');
    // No sample carried an INP: "0 ms" would be the best interaction there could be.
    await expect(row.getByRole('cell').nth(4)).toHaveText('—');

    await page.goto('/rum/performance/overview');
    // Each vital once, as its 75th percentile; the row of averages over them is gone.
    await expect(page.getByText('Largest Contentful Paint')).toHaveCount(0);
    await expect(page.getByText('2.7 s').first()).toBeVisible();
    await expect(page.getByText('2720ms')).toHaveCount(0);
  });

  test('the ranked lists of the overview are rows of two lines, with no air under them', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.route(/\/api\/v1\/rum\/overview\/insights(?:\?.*)?$/, (route) =>
      route.fulfill({
        json: {
          trend: [],
          satisfaction: { good: 1, needsImprovement: 0, poor: 1, total: 2 },
          slowPages: [
            { page: '/campaign/summer', p75: 3_800, grade: 'needs_improvement', errorRate: 0, samples: 2 },
            { page: '/checkout', p75: 3_300, grade: 'needs_improvement', errorRate: 0.25, samples: 6 },
            { page: '/search', p75: 2_200, grade: 'good', errorRate: 0, samples: 4 },
          ],
          frequentErrors: [
            { fingerprint: 'fp-1', message: 'Payment provider timeout', users: 2, version: 'v2.4.0', count: 2 },
          ],
        },
      }),
    );
    await page.goto('/rum/overview');

    const rows = page.locator('[data-rum-ranked-row]');
    await expect(rows).toHaveCount(4);
    // The two lists stand side by side: a row is the same height in both, and no taller than its
    // two lines need. A floor of 62px left a third of every row empty.
    const heights = await rows.evaluateAll((elements) =>
      elements.map((element) => element.getBoundingClientRect().height),
    );
    expect(new Set(heights).size).toBe(1);
    expect(heights[0]).toBeLessThanOrEqual(48);
    // Read across, the first rows of the two lists start on one line.
    const tops = await page
      .locator('[data-rum-ranked-list]')
      .evaluateAll((lists) => lists.map((list) => Math.round(list.getBoundingClientRect().top)));
    expect(tops[0]).toBe(tops[1]);
    // An error rate is said on the row that has one.
    const errorRates = page.locator('[data-rum-page-errors]');
    await expect(errorRates.nth(0)).toHaveText('');
    await expect(errorRates.nth(1)).toHaveText('25% error rate');
    // The first row stands clear of the panel's own heading.
    const gap = await page.locator('[data-rum-ranked-list]').first().evaluate((list) => {
      const head = list.previousElementSibling!.getBoundingClientRect();
      return list.getBoundingClientRect().top - head.bottom;
    });
    expect(gap).toBeGreaterThanOrEqual(6);
  });
});
