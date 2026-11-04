import type { Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM, mockRumData } from '../fixtures/mockRumData';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const DATA = mockRumData(NOW);

const WEB = MOCK_RUM.applications.web;
const APP = MOCK_RUM.applications.app;

const VIEWS = DATA.views.filter((row) => row['application.id'] === WEB);

type Level = 'lcp' | 'inp' | 'cls' | 'fcp' | 'ttfb';

const FIELD: Record<Level, string> = {
  lcp: 'view.performance.lcp.timestamp',
  inp: 'view.performance.inp.duration',
  cls: 'view.performance.cls.score',
  fcp: 'view.performance.fcp.timestamp',
  ttfb: 'view.first_byte',
};

/** Where each level turns from good to needs improvement and on to poor, in the unit the page reads it in. */
const LINES: Record<Level, [good: number, poor: number]> = {
  lcp: [2_500, 4_000],
  inp: [200, 500],
  cls: [0.1, 0.25],
  fcp: [1_800, 3_000],
  ttfb: [800, 1_800],
};

const GRADE_WORDS = { good: 'Good', needs_improvement: 'Needs improvement', poor: 'Poor' } as const;

const gradeOf = (level: Level, value: number): keyof typeof GRADE_WORDS => {
  const [good, poor] = LINES[level];

  return value <= good ? 'good' : value <= poor ? 'needs_improvement' : 'poor';
};

/** What a view carries of a level, in the unit the page reads it in: milliseconds, or a score. */
const levelOf = (row: (typeof VIEWS)[number], level: Level): number | null => {
  const raw = row[FIELD[level]];

  return raw === undefined || raw === null ? null : level === 'cls' ? Number(raw) : Number(raw) / 1_000_000;
};

/** The 75th percentile by nearest rank, as the mock backend takes it. */
function p75(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);

  return sorted[Math.max(1, Math.ceil(sorted.length * 0.75)) - 1] as number;
}

const valuesOf = (rows: readonly (typeof VIEWS)[number][], level: Level) =>
  rows.flatMap((row) => {
    const value = levelOf(row, level);

    return value === null ? [] : [value];
  });

/** A time in milliseconds as the page writes it: in the unit it reads best in. */
const duration = (ms: number): string => (ms < 1_000 ? `${Math.round(ms)} ms` : `${(ms / 1_000).toFixed(1)} s`);

/** A level as the page writes it: a time in its best unit, a score with up to two decimals. */
const written = (level: Level, value: number): string => (level === 'cls' ? String(Number(value.toFixed(2))) : duration(value));

const percent = (part: number, whole: number) => `${Math.round((part / whole) * 100)}%`;

/** The views of each value of a field, the most viewed first, ties by the value. */
function groupsOf(field: string): Array<{ value: string; rows: typeof VIEWS }> {
  const groups = new Map<string, typeof VIEWS>();

  for (const row of VIEWS) {
    const value = String(row[field]);

    groups.set(value, [...(groups.get(value) ?? []), row]);
  }

  return [...groups.entries()]
    .map(([value, rows]) => ({ value, rows }))
    .sort((a, b) => b.rows.length - a.rows.length || a.value.localeCompare(b.value));
}

const PAGE = 'view.url_path_group';

const row = (page: Page, value: string) => page.locator(`[data-rum-row="${value}"]`);

const order = (page: Page) => page.locator('[data-rum-performance-table] [data-rum-row]');

/**
 * Opens the page and waits for the time range the page puts in the address, as it does 80 ms after
 * it mounts: a click made before that would be written over by it.
 */
async function open(page: Page, path = '/rum/performance') {
  await page.goto(path);
  await expect(page).toHaveURL(/[?&]time=/);
}

async function useChinese(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem('molesignal-ui-prefs', JSON.stringify({ state: { language: 'zh-cn' }, version: 0 }));
  });
}

test.describe('RUM performance', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('says where the vitals stand at the 75th percentile, each in the colour of its grade', async ({ page }) => {
    await open(page);
    await expect(page.locator('[data-rum-navigation="surface"] [aria-current="page"]').first()).toContainText('Performance');
    await expect(page.locator('[data-rum-scope="application"]')).toContainText('Storefront');

    const reading = (level: Level) => {
      const value = p75(valuesOf(VIEWS, level));

      return { text: written(level, value), grade: gradeOf(level, value) };
    };

    const statement = page.locator('[data-rum-statement]');
    const { text: lcp, grade: lcpGrade } = reading('lcp');
    const { text: inp, grade: inpGrade } = reading('inp');
    const { text: cls, grade: clsGrade } = reading('cls');

    await expect(statement).toContainText(`${VIEWS.length} page views`);
    await expect(statement).toContainText(`LCP is ${lcp} (${GRADE_WORDS[lcpGrade]})`);
    await expect(statement).toContainText(`INP is ${inp} (${GRADE_WORDS[inpGrade]})`);
    await expect(statement).toContainText(`CLS is ${cls} (${GRADE_WORDS[clsGrade]})`);

    // The five levels, to pick the one the page is of: the vitals, and the two that come before them.
    for (const level of ['lcp', 'inp', 'cls', 'fcp', 'ttfb'] as const) {
      const { text, grade } = reading(level);

      await expect(page.locator(`[data-rum-metric="${level}"]`)).toContainText(text);
      await expect(page.locator(`[data-rum-metric="${level}"]`)).toHaveAttribute('data-rum-grade', grade);
    }

    await expect(page.locator('[data-rum-metric="lcp"]')).toHaveAttribute('aria-pressed', 'true');
    await expect(page.locator('[data-rum-overview-card="performance-trend"]')).toContainText('LCP over time');
  });

  test('picks the level the chart, the grades and the table are of, and the address says which', async ({ page }) => {
    await open(page);

    await page.locator('[data-rum-metric="inp"]').click();
    await expect(page).toHaveURL(/[?&]vital=inp/);
    await expect(page.locator('[data-rum-metric="inp"]')).toHaveAttribute('aria-pressed', 'true');
    await expect(page.locator('[data-rum-metric="lcp"]')).toHaveAttribute('aria-pressed', 'false');
    await expect(page.locator('[data-rum-overview-card="performance-trend"]')).toContainText('INP over time');
    await expect(page.locator('[data-rum-column="good"]')).toContainText('INP good');

    // The first level is the one the page opens on: picking it again takes the word out of the address.
    await page.locator('[data-rum-metric="lcp"]').click();
    await expect(page).not.toHaveURL(/vital=/);
    await expect(page.locator('[data-rum-column="good"]')).toContainText('LCP good');
  });

  test('is opened from a vital on the overview, on that vital, of the same application', async ({ page }) => {
    await open(page, '/rum/overview');
    await page.locator('[data-rum-vital="inp"]').getByRole('link').click();

    await expect(page).toHaveURL(new RegExp(`/rum/performance\\?.*app=${WEB}`));
    await expect(page).toHaveURL(/[?&]vital=inp/);
    await expect(page.locator('[data-rum-metric="inp"]')).toHaveAttribute('aria-pressed', 'true');
    await expect(page.locator('[data-rum-navigation="surface"] [aria-current="page"]').first()).toContainText('Performance');
  });

  test('divides the views among the grades, for every level, and says it in words for the one in front', async ({ page }) => {
    await open(page);

    for (const level of ['lcp', 'inp', 'cls'] as const) {
      const values = valuesOf(VIEWS, level);
      const [good, poor] = LINES[level];
      const goodViews = values.filter((value) => value <= good).length;
      const poorViews = values.filter((value) => value > poor).length;
      const middle = values.length - goodViews - poorViews;

      await page.locator(`[data-rum-grades-row="${level}"]`).click();

      const detail = page.locator(`[data-rum-grades-detail="${level}"]`);
      await expect(detail).toContainText(`Good${percent(goodViews, values.length)}`);
      await expect(detail).toContainText(`Needs improvement${percent(middle, values.length)}`);
      await expect(detail).toContainText(`Poor${percent(poorViews, values.length)}`);
      await expect(page.locator(`[data-rum-grades-row="${level}"]`)).toHaveAttribute('aria-pressed', 'true');
    }
  });

  test('tells the pages apart, the most viewed first, with the level of each in the colour of its grade', async ({ page }) => {
    await open(page);

    const pages = groupsOf(PAGE);

    await expect(order(page)).toHaveCount(pages.length);
    expect(await order(page).evaluateAll((rows) => rows.map((element) => element.getAttribute('data-rum-row')))).toEqual(
      pages.map((entry) => entry.value),
    );

    // A page with a handful of views is grey: a few views say nothing about how fast a page is.
    const checkout = pages.find((entry) => entry.value === '/checkout')!;
    const lcp = p75(valuesOf(checkout.rows, 'lcp'));

    expect(checkout.rows.length).toBeLessThan(30);
    await expect(row(page, '/checkout')).toHaveAttribute('data-rum-few', '');
    await expect(row(page, '/checkout').getByRole('cell').nth(2)).toHaveText(written('lcp', lcp));
    await expect(row(page, '/checkout').getByRole('cell').nth(1)).toHaveText(String(checkout.rows.length));
    await expect(page.locator('[data-rum-table-count]')).toContainText(`${pages.length} values`);
  });

  test('orders by a level when its heading is clicked, and the other way when it is clicked again', async ({ page }) => {
    await open(page);

    const pages = groupsOf(PAGE).map((entry) => ({ ...entry, lcp: p75(valuesOf(entry.rows, 'lcp')) }));
    const slowest = [...pages].sort((a, b) => b.lcp - a.lcp || b.rows.length - a.rows.length).map((entry) => entry.value);
    const names = async () => order(page).evaluateAll((rows) => rows.map((element) => element.getAttribute('data-rum-row')));

    await page.locator('[data-rum-column="lcp"] button').click();
    await expect(page).toHaveURL(/[?&]sort=lcp/);
    await expect(page.locator('[data-rum-column="lcp"]')).toHaveAttribute('aria-sort', 'descending');
    expect(await names()).toEqual(slowest);

    await page.locator('[data-rum-column="lcp"] button').click();
    await expect(page).toHaveURL(/[?&]dir=asc/);
    await expect(page.locator('[data-rum-column="lcp"]')).toHaveAttribute('aria-sort', 'ascending');
    expect(await names()).toEqual(
      [...pages].sort((a, b) => a.lcp - b.lcp || b.rows.length - a.rows.length).map((entry) => entry.value),
    );

    // Another column starts from the way it reads best: the most views, the slowest, the fewest good ones.
    await page.locator('[data-rum-column="inp"] button').click();
    await expect(page).toHaveURL(/[?&]sort=inp/);
    await expect(page).toHaveURL(/[?&]vital=inp/);
    await expect(page).not.toHaveURL(/dir=/);
    await expect(page.locator('[data-rum-column="inp"]')).toHaveAttribute('aria-sort', 'descending');
  });

  test('narrows the table by words, and tells the views apart by another dimension', async ({ page }) => {
    await open(page);

    await page.getByPlaceholder('Search values').fill('check');
    await expect(page).toHaveURL(/[?&]q=check/);
    await expect(order(page)).toHaveCount(1);
    await expect(row(page, '/checkout')).toBeVisible();

    await page.getByPlaceholder('Search values').fill('zzz');
    await expect(page.locator('[data-rum-no-match]')).toHaveText('No value matches');

    await page.getByPlaceholder('Search values').fill('');
    await expect(page).not.toHaveURL(/[?&]q=/);

    await page.locator('[data-rum-dimension]').click();
    await page.getByRole('option', { name: 'Browser' }).click();
    await expect(page).toHaveURL(/[?&]by=browser/);

    const browsers = groupsOf('browser.name');

    await expect(order(page)).toHaveCount(browsers.length);

    for (const entry of browsers) {
      await expect(row(page, entry.value).getByRole('cell').nth(1)).toHaveText(String(entry.rows.length));
    }

    // A country is told by its name in the language of the page, not by its code.
    await page.locator('[data-rum-dimension]').click();
    await page.getByRole('option', { name: 'Country or region' }).click();
    await expect(page).toHaveURL(/[?&]by=country/);
    await expect(row(page, 'DE')).toContainText('Germany');
  });

  test('opens what the views of a page are made of: how far a level is, its parts, the elements, the worst views', async ({ page }) => {
    await open(page);
    await page.locator('[data-rum-row="/checkout"] [data-rum-open]').click();

    await expect(page).toHaveURL(/[?&]open=%2Fcheckout/);

    const drawer = page.locator('[data-rum-attribution-drawer]');
    const checkout = VIEWS.filter((view) => view[PAGE] === '/checkout');
    const lcp = p75(valuesOf(checkout, 'lcp'));

    await expect(drawer.locator('[data-rum-attribution-title]')).toHaveText('/checkout');
    await expect(drawer.locator('[data-rum-attribution-statement]')).toContainText(
      `LCP is ${written('lcp', lcp)} (${GRADE_WORDS[gradeOf('lcp', lcp)]}) at the 75th percentile, over ${checkout.length} views.`,
    );
    await expect(drawer.locator('[data-rum-drawer-tabs] [data-tab-active="true"]')).toContainText('LCP');

    // What the level is made of: the first byte and the three parts of the largest paint, which add up to it.
    const parts = ['first_byte', 'load_delay', 'load_time', 'render_delay'];
    const mean = (field: string) => checkout.reduce((sum, view) => sum + Number(view[field]), 0) / checkout.length / 1_000_000;
    const ms = [
      mean('view.first_byte'),
      mean('view.performance.lcp.sub_parts.load_delay'),
      mean('view.performance.lcp.sub_parts.load_time'),
      mean('view.performance.lcp.sub_parts.render_delay'),
    ];
    const total = ms.reduce((sum, value) => sum + value, 0);
    const longest = ms.indexOf(Math.max(...ms));

    await expect(drawer.locator('[data-rum-part]')).toHaveCount(parts.length);
    await expect(drawer.locator('[data-rum-part-line]')).toHaveCount(parts.length);
    await expect(drawer.locator('[data-rum-attribution-longest]')).toContainText(
      `The longest part is ${['Time to first byte', 'Load delay', 'Load time', 'Render delay'][longest]}: ${duration(ms[longest]!)}, ${percent(ms[longest]!, total)} of the whole.`,
    );

    // The element that was painted last: a block of text, so it has no file.
    await expect(drawer.locator('[data-rum-element="div.order-summary"]')).toContainText('text');
    await expect(drawer.locator('[data-rum-element="div.order-summary"]')).toContainText(String(checkout.length));

    // The views that were worst, each with where it opens.
    await expect(drawer.locator('[data-rum-worst-view]')).toHaveCount(checkout.length);
    await expect(drawer.locator('[data-rum-worst-view]').first().getByRole('link', { name: 'Session' })).toHaveAttribute('href', /\/rum\/sessions\/view\//);
    await expect(drawer.getByRole('link', { name: 'View in explorer' })).toHaveAttribute('href', /\/rum\/explorer\?/);

    // The vital the drawer is of is the tab, and it is not in the address while it is the one in front.
    await drawer.locator('[data-rum-drawer-tabs]').getByRole('button', { name: 'INP' }).click();
    await expect(page).toHaveURL(/[?&]tab=inp/);

    const inp = p75(valuesOf(checkout, 'inp'));

    await expect(drawer.locator('[data-rum-attribution-statement]')).toContainText(`INP is ${written('inp', inp)}`);
    await expect(drawer.locator('[data-rum-attribution-longest]')).toContainText('The longest part is Processing');
    await expect(drawer.locator('[data-rum-element="button.pay-now"]')).toBeVisible();

    // A shift is a score, not a time: it has no parts, only the elements that moved.
    await drawer.locator('[data-rum-drawer-tabs]').getByRole('button', { name: 'CLS' }).click();
    await expect(page).toHaveURL(/[?&]tab=cls/);
    await expect(drawer.locator('[data-rum-overview-card="performance-makeup"]')).toHaveCount(0);
    await expect(drawer.locator('[data-rum-element="div.promo-banner"]')).toBeVisible();

    await page.keyboard.press('Escape');
    await expect(page).not.toHaveURL(/[?&]open=/);
    await expect(page).not.toHaveURL(/[?&]tab=/);
    await expect(drawer).toHaveCount(0);
  });

  test('opens the load of a worst view over the drawer, and Escape closes it before the drawer', async ({ page }) => {
    await open(page);
    await page.locator('[data-rum-row="/checkout"] [data-rum-open]').click();

    const attribution = page.locator('[data-rum-attribution-drawer]');
    const worst = attribution.locator('[data-rum-worst-view]').first();
    const id = await worst.getAttribute('data-rum-worst-view');

    await worst.locator('[data-rum-worst-load]').click();
    await expect(page).toHaveURL(new RegExp(`[?&]view=${id}`));
    await expect(page).toHaveURL(/[?&]open=%2Fcheckout/);

    const load = page.locator('[data-rum-drawer]');

    await expect(load.locator('[data-rum-waterfall]')).toBeVisible();
    await expect(load.locator('[data-rum-waterfall-sentence]')).toContainText('This view loaded in');

    // The drawer of the load is on top: Escape closes it, and the drawer of the value is as it was.
    await page.keyboard.press('Escape');
    await expect(page).not.toHaveURL(/[?&]view=/);
    await expect(page).toHaveURL(/[?&]open=%2Fcheckout/);
    await expect(load).toHaveCount(0);
    await expect(attribution).toBeVisible();
  });

  test('says so when the browsers of the views report no parts, and carries on with what they do report', async ({ page }) => {
    // Safari reports neither the element nor the parts of a level; only Chromium does.
    const safari = VIEWS.filter((view) => view['browser.name'] === 'Safari');

    expect(safari.length).toBeGreaterThan(0);
    expect(safari.every((view) => view['view.performance.lcp.target_selector'] === undefined)).toBe(true);

    await open(page, '/rum/performance?by=browser');
    await row(page, 'Safari').locator('[data-rum-open]').click();

    const drawer = page.locator('[data-rum-attribution-drawer]');

    await expect(drawer.locator('[data-rum-attribution-title]')).toHaveText('Safari');
    await expect(drawer.locator('[data-rum-attribution-statement]')).toContainText('at the 75th percentile');
    await expect(drawer.locator('[data-rum-overview-card="performance-makeup"]')).toContainText('No view in this range reported the parts of LCP.');
    await expect(drawer.locator('[data-rum-overview-card="performance-elements"]')).toContainText('No view in this range named an element');
  });

  test('is the page the address says it is, so that a copy of it opens the same', async ({ page }) => {
    await open(page, '/rum/performance?vital=fcp&by=browser&sort=fcp&open=Chrome&tab=inp');

    await expect(page.locator('[data-rum-metric="fcp"]')).toHaveAttribute('aria-pressed', 'true');
    await expect(page.locator('[data-rum-column="fcp"]')).toHaveAttribute('aria-sort', 'descending');
    await expect(page.locator('[data-rum-attribution-drawer] [data-rum-attribution-title]')).toHaveText('Chrome');
    await expect(page.locator('[data-rum-attribution-drawer] [data-rum-drawer-tabs] [data-tab-active="true"]')).toContainText('INP');
    await expect(row(page, 'Chrome')).toHaveClass(/floating-item-selected/);
  });

  test('has no page loads to grade for an application that does not send from a browser', async ({ page }) => {
    await open(page);

    await page.getByRole('combobox', { name: 'Application' }).click();
    await page.getByRole('option', { name: 'Storefront app' }).click();

    await expect(page).toHaveURL(new RegExp(`[?&]app=${APP}`));
    await expect(page.locator('[data-rum-performance]')).toHaveAttribute('data-rum-platform', 'flutter');
    await expect(page.getByText('Web Vitals are measured in browsers')).toBeVisible();
    await expect(page.locator('[data-rum-statement]')).toHaveCount(0);
    await expect(page.locator('[data-rum-attribution-drawer]')).toHaveCount(0);
  });

  test('is of one environment when one is chosen, and has nothing to say of one nothing ran in', async ({ page }) => {
    await open(page);

    await page.getByRole('combobox', { name: 'Environment' }).click();
    await page.getByRole('option', { name: 'prod' }).click();
    await expect(page).toHaveURL(/[?&]env=prod/);
    await expect(page.locator('[data-rum-statement]')).toContainText(`${VIEWS.length} page views`);

    await page.goto('/rum/performance?env=staging');
    await expect(page.getByText('No page views in this range')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Expand the time range' })).toBeVisible();
    await expect(page.locator('[data-rum-statement]')).toHaveCount(0);
  });

  test('says how to start before the first application exists', async ({ page }) => {
    await page.route(/\/api\/v1\/rum\/applications(?:\?.*)?$/, (route) => route.fulfill({ json: [] }));
    await page.goto('/rum/performance');

    await expect(page.getByTestId('rum-activation').getByRole('heading', { level: 2 })).toHaveText('Start collecting real user experience data');
    await expect(page.locator('[data-rum-statement]')).toHaveCount(0);
  });

  test('offers another go for the table whose read failed, and keeps the rest', async ({ page }) => {
    await page.route(/\/api\/v1\/rum\/aggregate\?.*groupBy/, (route) =>
      route.fulfill({ status: 500, json: { error: 'internal', message: 'the engine is busy' } }),
    );
    await open(page);

    await expect(page.locator('[data-rum-overview-card="performance-table"]').getByRole('button', { name: 'Retry' })).toBeVisible();
    await expect(page.locator('[data-rum-statement]')).toBeVisible();
    await expect(page.locator('[data-rum-overview-card="performance-grades"]')).toBeVisible();
  });

  test('says why the chart has no line when counting took too long', async ({ page }) => {
    await page.route(/\/api\/v1\/rum\/aggregate\?.*bucketTime/, (route) =>
      route.fulfill({ json: { groupBy: [], measures: [], rows: [], unavailable: 'time_budget' } }),
    );
    await open(page);

    await expect(page.locator('[data-rum-overview-card="performance-trend"]')).toContainText('Counting took longer than the time budget');
    await expect(page.locator('[data-rum-statement]')).toBeVisible();
  });
});

test.describe('RUM performance in Chinese', () => {
  test('says its statement and its words in the language of the product', async ({ page, mockServer }) => {
    await useChinese(page);
    await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });
    await open(page);

    const lcp = p75(valuesOf(VIEWS, 'lcp'));

    await expect(page.locator('[data-rum-navigation="surface"] [aria-current="page"]').first()).toContainText('性能');
    await expect(page.locator('[data-rum-statement]')).toContainText(`${VIEWS.length} 次页面浏览`);
    await expect(page.locator('[data-rum-statement]')).toContainText(`LCP 的 p75 是 ${written('lcp', lcp)}（${{ good: '良好', needs_improvement: '待改进', poor: '差' }[gradeOf('lcp', lcp)]}）`);
    await expect(page.locator('[data-rum-overview-card="performance-trend"]')).toContainText('LCP 走势');
    await expect(page.locator('[data-rum-column="good"]')).toContainText('LCP 良好');

    await page.locator('[data-rum-row="/checkout"] [data-rum-open]').click();
    await expect(page.locator('[data-rum-attribution-longest]')).toContainText('最长的一段是 渲染延迟');
    await expect(page.locator('[data-rum-attribution-drawer] [data-rum-overview-card="performance-makeup"]')).toContainText('LCP 由什么构成');
  });
});
