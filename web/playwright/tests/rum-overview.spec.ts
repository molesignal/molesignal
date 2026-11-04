import type { Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM, mockRumData } from '../fixtures/mockRumData';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const DATA = mockRumData(NOW);

const WEB = MOCK_RUM.applications.web;
const APP = MOCK_RUM.applications.app;

const of = (rows: typeof DATA.sessions, application: string) => rows.filter((row) => row['application.id'] === application);

/** The 75th percentile by nearest rank, as the mock backend takes it. */
function p75(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);

  return sorted[Math.max(1, Math.ceil(sorted.length * 0.75)) - 1] as number;
}

const share = (fraction: number) => `${Number((fraction * 100).toFixed(2))}%`;

/**
 * Opens the page and waits for the time range the page puts in the address, as it does 80 ms after
 * it mounts: a click made before that would be written over by it.
 */
async function open(page: Page) {
  await page.goto('/rum/overview');
  await expect(page).toHaveURL(/\/rum\/overview\?time=now-24h\.\.now$/);
}

async function useChinese(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem('molesignal-ui-prefs', JSON.stringify({ state: { language: 'zh-cn' }, version: 0 }));
  });
}

test.describe('RUM overview', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('is where the module opens, on the first application by name, and says how its sessions did', async ({ page }) => {
    await page.goto('/rum');
    await expect(page).toHaveURL(/\/rum\/overview\?time=now-24h\.\.now$/);
    await expect(page.locator('[data-rum-navigation="surface"] [aria-current="page"]').first()).toContainText('Overview');
    await expect(page.locator('[data-rum-scope="application"]')).toContainText('Storefront');

    const sessions = of(DATA.sessions, WEB);
    const people = new Set(sessions.map((row) => String(row['usr.id'] ?? row['usr.anonymous_id'])));
    const withErrors = sessions.filter((row) => Number(row['session.error.count']) > 0).length;

    const statement = page.locator('[data-rum-pulse-statement]');
    await expect(statement).toContainText(
      `${sessions.length} sessions from ${people.size} people, and ${share(1 - withErrors / sessions.length)} of the sessions had no errors.`,
    );
  });

  test('grades each web vital on its scale, by the 75th percentile of the views', async ({ page }) => {
    await page.goto('/rum/overview');

    const views = of(DATA.views, WEB);
    const lcp = p75(views.map((row) => Number(row['view.performance.lcp.timestamp']) / 1_000_000));
    const inp = p75(views.map((row) => Number(row['view.performance.inp.duration']) / 1_000_000));
    const cls = p75(views.map((row) => Number(row['view.performance.cls.score'])));

    const grade = (value: number, good: number, poor: number) => (value <= good ? 'good' : value <= poor ? 'needs_improvement' : 'poor');

    await expect(page.locator('[data-rum-vital="lcp"]')).toHaveAttribute('data-rum-grade', grade(lcp, 2_500, 4_000));
    await expect(page.locator('[data-rum-vital="inp"]')).toHaveAttribute('data-rum-grade', grade(inp, 200, 500));
    await expect(page.locator('[data-rum-vital="cls"]')).toHaveAttribute('data-rum-grade', grade(cls, 0.1, 0.25));

    // The mark stands on the scale, and the thresholds are written under it.
    await expect(page.locator('[data-rum-vital="lcp"] [data-rum-vital-marker]')).toBeVisible();
    await expect(page.locator('[data-rum-vital="lcp"]')).toContainText('≤ 2.5 s good');
    await expect(page.locator('[data-rum-vital="inp"]')).toContainText('> 500 ms poor');
  });

  test('lists the most viewed pages, and a row opens the explorer on the views of that page', async ({ page }) => {
    await open(page);

    const counted = new Map<string, number>();
    for (const row of of(DATA.views, WEB)) {
      const group = String(row['view.url_path_group']);
      counted.set(group, (counted.get(group) ?? 0) + 1);
    }
    const ranked = [...counted].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));

    const rows = page.locator('[data-rum-page]');
    await expect(rows).toHaveCount(Math.min(8, ranked.length));
    expect(await rows.evaluateAll((elements) => elements.map((element) => element.getAttribute('data-rum-page')))).toEqual(
      ranked.slice(0, 8).map(([group]) => group),
    );

    // Each page names its views; one that is slower than it should be is told in the colour of its word.
    const first = ranked[0] as [string, number];
    await expect(rows.first()).toContainText(String(first[1]));
    await expect(page.locator('[data-rum-page="/checkout"] [data-rum-grade="needs_improvement"]')).toHaveCount(3);
    await expect(page.locator('[data-rum-page="/"] [data-rum-grade="good"]')).toHaveCount(3);

    await rows.first().getByRole('link').click();
    await expect(page).toHaveURL(/\/rum\/explorer\?type=views&q=/);
    await expect(page.locator('[data-rum-row]')).toHaveCount(first[1]);
  });

  test('shows where the visitors were on a map, and names the countries with the most sessions', async ({ page }) => {
    await page.goto('/rum/overview');

    const sessions = of(DATA.sessions, WEB);
    const countries = new Map<string, number>();
    for (const row of sessions) countries.set(String(row['geo.country_iso_code']), (countries.get(String(row['geo.country_iso_code'])) ?? 0) + 1);

    await expect(page.locator('[data-map-country]')).toHaveCount(countries.size);
    await expect(page.getByRole('group', { name: 'World map of where the sessions came from' })).toBeVisible();
    // The map zooms: its buttons are there, and the countries are tinted by their sessions.
    await expect(page.getByRole('button', { name: 'Zoom in' })).toBeVisible();
    await expect(page.locator('[data-map-legend]')).toContainText('Sessions');

    // The card is in a page that scrolls: the plain wheel is the page's, and says how to zoom the map.
    const map = page.locator('[data-world-map]');
    const box = (await map.boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.wheel(0, -300);
    await expect(page.locator('[data-map-hint]')).toContainText('Hold Ctrl');
    await expect(map).toHaveAttribute('data-map-zoom', '1.00');
    await page.keyboard.down('Control');
    await page.mouse.wheel(0, -300);
    await page.keyboard.up('Control');
    await expect.poll(async () => Number(await map.getAttribute('data-map-zoom'))).toBeGreaterThan(1);

    const top = [...countries].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))[0] as [string, number];
    const named = page.locator('[data-rum-bars="countries"] [data-rum-bar]').first();

    await expect(named).toHaveAttribute('data-rum-bar', top[0]);
    await expect(named).toContainText(String(top[1]));

    // The country is credited to the database it comes from, as on the explorer.
    await expect(page.locator('[data-rum-geo-credit] a')).toHaveAttribute('href', 'https://db-ip.com');
  });

  test('spreads the sessions over browsers, systems and devices, the commonest first', async ({ page }) => {
    await open(page);

    const sessions = of(DATA.sessions, WEB);
    const chrome = sessions.filter((row) => row['browser.name'] === 'Chrome').length;
    const desktop = sessions.filter((row) => row['device.type'] === 'desktop').length;

    const browsers = page.locator('[data-rum-bars="browsers"] [data-rum-bar]');
    await expect(browsers.first()).toHaveAttribute('data-rum-bar', 'Chrome');
    await expect(browsers.first()).toContainText(String(chrome));

    const devices = page.locator('[data-rum-bars="devices"] [data-rum-bar]');
    await expect(devices.first()).toContainText('Desktop');
    await expect(devices.first()).toContainText(String(desktop));

    // A value is a link to the sessions it counts.
    await browsers.first().getByRole('link', { name: 'Chrome' }).click();
    await expect(page).toHaveURL(/\/rum\/explorer\?type=sessions&q=/);
    await expect(page.locator('[data-rum-row]')).toHaveCount(chrome);
  });

  test('names the elements users were frustrated by, with what most of the actions on them were', async ({ page }) => {
    await page.goto('/rum/overview');

    // The elements told by their name and the page they are on: the five with the most frustrated actions, a tie to the name first.
    const groups = new Map<string, { name: string | null; path: string; total: number }>();

    for (const row of of(DATA.actions, WEB).filter((action) => Number(action['action.frustration.count']) > 0)) {
      const name = typeof row['action.target.name'] === 'string' ? row['action.target.name'] : null;
      const path = String(row['view.url_path_group']);
      const entry = groups.get(JSON.stringify([name, path])) ?? { name, path, total: 0 };

      entry.total += 1;
      groups.set(JSON.stringify([name, path]), entry);
    }

    const ranked = [...groups.values()].sort(
      (a, b) => b.total - a.total || (a.name === b.name ? 0 : a.name === null ? 1 : b.name === null ? -1 : a.name.localeCompare(b.name)) || a.path.localeCompare(b.path),
    );
    const rows = page.locator('[data-rum-frustrated]');

    expect(ranked.length).toBeGreaterThan(5);
    await expect(rows).toHaveCount(5);
    await expect(rows.first()).toHaveAttribute('data-rum-frustrated', 'Pay now');
    await expect(rows.first()).toContainText('/checkout');
    await expect(rows.first()).toContainText('Rage clicks');
    await expect(rows.first()).toContainText(String(ranked[0]!.total));
    // An element that has no name is told as one.
    await expect(rows.filter({ hasText: 'An element without a name' })).toHaveCount(ranked.slice(0, 5).filter((entry) => entry.name === null).length);
  });

  test('is of one environment when one is chosen, and the address says which', async ({ page }) => {
    await open(page);

    await page.getByRole('combobox', { name: 'Environment' }).click();
    await expect(page.getByRole('option')).toHaveText(['All environments', 'prod']);
    await page.getByRole('option', { name: 'prod' }).click();

    await expect(page).toHaveURL(/[?&]env=prod/);
    await expect(page.locator('[data-rum-scope="environment"]')).toContainText('prod');
    await expect(page.locator('[data-rum-pulse-statement]')).toContainText(`${of(DATA.sessions, WEB).length} sessions`);

    // An environment nothing ran in has no sessions to say anything about.
    await page.goto('/rum/overview?env=staging');
    await expect(page.getByText('No sessions in this range')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Expand the time range' })).toBeVisible();
    await expect(page.locator('[data-rum-pulse-statement]')).toHaveCount(0);
  });

  test('is of an app without web vitals: it counts crashes, and its screens are told by name', async ({ page }) => {
    await open(page);

    await page.getByRole('combobox', { name: 'Application' }).click();
    await page.getByRole('option', { name: 'Storefront app' }).click();

    await expect(page).toHaveURL(new RegExp(`[?&]app=${APP}`));
    await expect(page.locator('[data-rum-overview]')).toHaveAttribute('data-rum-platform', 'flutter');

    // The app has the one visit; each count agrees with its own number.
    expect(of(DATA.sessions, APP)).toHaveLength(1);
    await expect(page.locator('[data-rum-pulse-statement]')).toContainText(
      '1 session from 1 person; 100% of the sessions and 100% of the people had no crash.',
    );

    // An app sends no web vitals, and no browser to count.
    await expect(page.locator('[data-rum-vital]')).toHaveCount(0);
    await expect(page.locator('[data-rum-overview-card="browsers"]')).toHaveCount(0);
    await expect(page.locator('[data-rum-overview-card="systems"]')).toContainText('Android');
    await expect(page.locator('[data-rum-overview-card="pages"] h2')).toHaveText('Screens');
  });

  test('says how to start before the first application exists', async ({ page }) => {
    await page.route(/\/api\/v1\/rum\/applications(?:\?.*)?$/, (route) => route.fulfill({ json: [] }));
    await page.goto('/rum/overview');

    const firstRun = page.getByTestId('rum-activation');
    await expect(firstRun.getByRole('heading', { level: 2 })).toHaveText('Start collecting real user experience data');
    await expect(page.locator('[data-rum-pulse-statement]')).toHaveCount(0);
  });

  test('offers another go for a card whose read failed, and keeps the others', async ({ page }) => {
    await page.route(/\/api\/v1\/rum\/aggregate\?.*type=actions/, (route) =>
      route.fulfill({ status: 500, json: { error: 'internal', message: 'the engine is busy' } }),
    );
    await page.goto('/rum/overview');

    const card = page.locator('[data-rum-overview-card="frustration"]');
    await expect(card.getByRole('button', { name: 'Retry' })).toBeVisible();
    await expect(page.locator('[data-rum-pulse-statement]')).toBeVisible();
  });

  test('says why a card has no numbers when counting took too long', async ({ page }) => {
    await page.route(/\/api\/v1\/rum\/aggregate\?.*type=actions/, (route) =>
      route.fulfill({
        json: { groupBy: [], measures: [], rows: [], unavailable: 'time_budget' },
      }),
    );
    await page.goto('/rum/overview');

    await expect(page.locator('[data-rum-overview-card="frustration"]')).toContainText(
      'Counting took longer than the time budget',
    );
  });
});

test.describe('RUM overview in Chinese', () => {
  test('says its statement and its words in the language of the product', async ({ page, mockServer }) => {
    await useChinese(page);
    await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });
    await page.goto('/rum/overview');

    const sessions = of(DATA.sessions, WEB).length;

    await expect(page.locator('[data-rum-navigation="surface"] [aria-current="page"]').first()).toContainText('概览');
    await expect(page.locator('[data-rum-pulse-statement]')).toContainText(`${sessions} 次会话`);
    await expect(page.locator('[data-rum-pulse-statement]')).toContainText('的会话没有错误');
    await expect(page.locator('[data-rum-vital="lcp"]')).toContainText('良好');
    await expect(page.locator('[data-rum-overview-card="frustration"]')).toContainText('狂点');
  });
});
