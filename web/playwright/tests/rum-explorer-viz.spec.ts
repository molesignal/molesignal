import type { Locator, Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { mockRumData } from '../fixtures/mockRumData';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const DATA = mockRumData(NOW);
const VIEWS = DATA.views;
const SESSIONS = DATA.sessions;

const LCP = 'view.performance.lcp.timestamp';

type Row = { [column: string]: unknown };

/** The rows by the value of a field, a row with none under `null`. */
function countsBy(rows: readonly Row[], field: string): Map<string | null, number> {
  const counts = new Map<string | null, number>();

  for (const row of rows) {
    const value = row[field];
    const key = value === undefined || value === null || value === '' ? null : String(value);

    counts.set(key, (counts.get(key) ?? 0) + 1);
  }

  return counts;
}

const byName = (a: string | null, b: string | null) => (a === b ? 0 : a === null ? 1 : b === null ? -1 : a.localeCompare(b));

/** The groups with the most rows first, as the server ranks them, and the first of them as asked for. */
const ranked = (counts: Map<string | null, number>, limit = 10) =>
  [...counts].sort(([a, x], [b, y]) => y - x || byName(a, b)).slice(0, limit);

/**
 * Opens the page and waits for the time range the page puts in the address, as it does 80 ms after
 * it mounts: a click made before that would be written over by it.
 */
async function open(page: Page, path: string) {
  await page.goto(path);
  await expect(page).toHaveURL(/[?&]time=/);
}

const status = (page: Page) => page.locator('[data-rum-viz-status]');

/** A point of the screen that is inside the shape of a country: its middle is not always, a country is not a rectangle. */
async function pointInside(shape: Locator): Promise<{ x: number; y: number }> {
  return shape.evaluate((element) => {
    const path = element as unknown as SVGGeometryElement;
    const svg = path.ownerSVGElement!;
    const box = path.getBBox();
    const matrix = path.getScreenCTM()!;
    const point = svg.createSVGPoint();

    for (let step = 1; step < 24; step += 1) {
      for (let across = 1; across < 24; across += 1) {
        point.x = box.x + (box.width * across) / 24;
        point.y = box.y + (box.height * step) / 24;

        if (path.isPointInFill(point)) {
          const screen = point.matrixTransform(matrix);
          return { x: screen.x, y: screen.y };
        }
      }
    }

    throw new Error('no point inside the shape');
  });
}

const zoomOf = (page: Page) => page.locator('[data-world-map]').getAttribute('data-map-zoom').then(Number);

test.describe('RUM explorer: the rows drawn as numbers', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('the list is how the rows are drawn until another is chosen, and the address says which', async ({ page }) => {
    await open(page, '/rum/explorer?type=views');

    const rows = page.locator('[data-rum-row]');
    await expect(page.locator('[data-rum-viz-switch]')).toHaveAttribute('data-rum-viz-switch', 'list');
    await expect(rows).toHaveCount(VIEWS.length);
    await expect(page.locator('[data-rum-aggregate]')).toHaveCount(0);

    await page.locator('[data-rum-viz="timeseries"]').click();
    await expect(page).toHaveURL(/[?&]viz=timeseries/);
    await expect(page.locator('[data-rum-aggregate="timeseries"]')).toBeVisible();
    await expect(page.getByTestId('time-series-chart')).toBeVisible();
    await expect(rows).toHaveCount(0);

    await page.locator('[data-rum-viz="list"]').click();
    await expect(page).not.toHaveURL(/[?&]viz=/);
    await expect(rows).toHaveCount(VIEWS.length);
  });

  test('a ranking counts the rows by a field, and a group picked narrows the query to it and lets go again', async ({ page }) => {
    await open(page, '/rum/explorer?type=views&viz=toplist&by=browser.name');

    const expected = ranked(countsBy(VIEWS, 'browser.name'));
    const bars = page.locator('[data-rum-bars="rum-toplist"] [data-rum-bar]');
    await expect(bars).toHaveCount(expected.length);
    await expect(bars.first()).toContainText(String(expected[0]![1]));
    await expect(status(page)).toContainText(`${expected.length} groups`);

    const [name] = expected.find(([value]) => value !== null)!;
    const bar = page.locator('[data-rum-bars="rum-toplist"] [data-rum-bar]', { has: page.getByRole('button', { name: name! }) });

    await bar.getByRole('button', { name: name! }).click();
    await expect.poll(() => new URL(page.url()).searchParams.get('q')).toBe(`browser.name = '${name}'`);
    await expect(bars).toHaveCount(1);
    await expect(bars.first().getByRole('button')).toHaveAttribute('aria-pressed', 'true');

    await bars.first().getByRole('button').click();
    await expect.poll(() => new URL(page.url()).searchParams.has('q')).toBe(false);
    await expect(bars).toHaveCount(expected.length);
  });

  test('a table has a column for each number, ordered by the first of them until a head is picked', async ({ page }) => {
    await open(page, `/rum/explorer?type=views&viz=table&m=count&m=p75:${LCP}&by=view.url_path_group`);

    const groups = countsBy(VIEWS, 'view.url_path_group');
    const table = page.locator('[data-rum-viz-body="table"]');
    const names = () =>
      table.locator('[data-rum-viz-row]').evaluateAll((elements) => elements.map((element) => element.querySelector('[role="cell"]')?.textContent));
    const heads = table.getByRole('columnheader');

    await expect(table.locator('[data-rum-viz-row]')).toHaveCount(Math.min(10, groups.size));
    await expect(heads).toHaveCount(3);
    await expect(heads.nth(1)).toHaveAttribute('aria-sort', 'descending');
    expect(await names()).toEqual(ranked(groups).map(([name]) => name ?? '(none)'));

    // A name is looked for from the first up; the server answers in that order, so the rows are the first of it.
    await table.locator('[data-rum-sort="view.url_path_group"]').click();
    await expect.poll(() => new URL(page.url()).searchParams.get('sort')).toBe('view.url_path_group');
    await expect(heads.first()).toHaveAttribute('aria-sort', 'ascending');
    expect(await names()).toEqual(
      [...groups.keys()]
        .sort(byName)
        .slice(0, 10)
        .map((name) => name ?? '(none)'),
    );

    // Picked again, from the last down.
    await table.locator('[data-rum-sort="view.url_path_group"]').click();
    await expect(heads.first()).toHaveAttribute('aria-sort', 'descending');
    expect(new URL(page.url()).searchParams.get('dir')).toBe('desc');
  });

  test('what is counted is chosen in the row under the query, and the address keeps it', async ({ page }) => {
    await open(page, '/rum/explorer?type=views&viz=toplist&by=view.url_path_group');

    const bars = page.locator('[data-rum-bars="rum-toplist"] [data-rum-bar]');
    await expect(bars.first()).toContainText(/^\S+\s*\d+$/);

    // The number: a function, then the field it takes.
    await page.locator('[data-rum-viz-control="measure"]').click();
    await page.getByRole('button', { name: 'p95', exact: true }).click();
    await page.getByPlaceholder('Search fields').fill('lcp');
    await page.getByRole('option', { name: LCP }).click();
    await expect.poll(() => new URL(page.url()).searchParams.get('m')).toBe(`p95:${LCP}`);
    await expect(bars.first()).toContainText(/\d(\.\d)? (s|ms)/);

    // The group.
    await page.locator('[data-rum-viz-control="group"]').click();
    await page.getByPlaceholder('Search fields').fill('browser.name');
    await page.getByRole('option', { name: 'browser.name' }).click();
    await expect.poll(() => new URL(page.url()).searchParams.get('by')).toBe('browser.name');

    // How many groups.
    await page.locator('[data-rum-viz-control="top"]').click();
    await page.getByRole('option', { name: '5', exact: true }).click();
    await expect.poll(() => new URL(page.url()).searchParams.get('top')).toBe('5');
    await expect(bars).toHaveCount(Math.min(5, countsBy(VIEWS, 'browser.name').size));
  });

  test('a chart over time is lines for a level and stacked bars for a count', async ({ page }) => {
    await open(page, `/rum/explorer?type=views&viz=timeseries&m=p75:${LCP}&by=view.url_path_group`);

    const chart = page.getByTestId('time-series-chart');
    await expect(chart).toHaveAttribute('data-stack-mode', 'none');
    await expect(status(page)).toContainText('1 hour per point');
    await expect(status(page)).toContainText('5 groups');
    await expect(chart.getByRole('listitem')).toHaveCount(Math.min(5, countsBy(VIEWS, 'view.url_path_group').size));

    await page.locator('[data-rum-viz-control="top"]').click();
    await page.getByRole('option', { name: '3', exact: true }).click();
    await expect(chart.getByRole('listitem')).toHaveCount(3);

    await open(page, '/rum/explorer?type=views&viz=timeseries&by=browser.name');
    await expect(page.getByTestId('time-series-chart')).toHaveAttribute('data-stack-mode', 'normal');
  });

  test('a series of the chart can be included in the query, or left out of it, from its pinned tooltip', async ({ page }) => {
    await open(page, '/rum/explorer?type=views&viz=timeseries&by=browser.name');

    const plot = page.getByTestId('time-series-plot');
    await expect(plot).toBeVisible();
    const box = (await plot.boundingBox())!;

    // The bar of the last hour of the range is where the mock views are: the pointer on it shows what is there, and a click pins it.
    const at = { x: box.x + box.width - 40, y: box.y + box.height / 2 };
    const tooltip = page.getByTestId('time-series-tooltip');
    await page.mouse.move(at.x, at.y);
    await expect(tooltip).toBeVisible();
    await page.mouse.click(at.x, at.y);

    const first = (await tooltip.locator('span[title]').first().getAttribute('title'))!;
    await tooltip.getByRole('button', { name: 'Exclude' }).click();
    await expect.poll(() => new URL(page.url()).searchParams.get('q')).toBe(`browser.name != '${first}'`);
  });

  test('a map is drawn by country, whatever else the address asks for', async ({ page }) => {
    await open(page, '/rum/explorer?viz=map&by=browser.name');

    const countries = new Set(SESSIONS.map((row) => row['geo.country_iso_code']).filter(Boolean));
    await expect(page.locator('[data-rum-viz-body="map"] [data-map-country]')).toHaveCount(countries.size);
    await expect(status(page)).toContainText(`${countries.size} countries`);
    await expect(page.locator('[data-rum-viz-control="group-fixed"]')).toContainText('geo.country_iso_code');
    await expect(page.locator('[data-rum-viz-control="group"]')).toHaveCount(0);

    // The whole world is there, country by country: the ones with sessions are tinted, the rest are land.
    expect(await page.locator('[data-world-map] path[data-world-country]').count()).toBeGreaterThan(170);
    await expect(page.locator('[data-map-legend]')).toContainText('Count of sessions');
  });

  test('a country says what it is, and what it came to, when the pointer is on it', async ({ page }) => {
    await open(page, '/rum/explorer?viz=map');

    const china = page.locator('[data-rum-viz-body="map"] path[data-map-country="CN"]');
    const at = await pointInside(china);
    await page.mouse.move(at.x, at.y);

    const tooltip = page.locator('[data-map-tooltip]');
    await expect(tooltip).toContainText('China');
    await expect(tooltip).toContainText(String(SESSIONS.filter((row) => row['geo.country_iso_code'] === 'CN').length));

    // A country with nothing to say is still a country.
    const australia = page.locator('[data-world-map] path[data-world-country="AU"]');
    await expect(australia).not.toHaveAttribute('data-map-country', /.+/);
    const elsewhere = await pointInside(australia);
    await page.mouse.move(elsewhere.x, elsewhere.y);
    await expect(tooltip).toContainText('Australia');
    await expect(tooltip).toContainText('No data');
  });

  test('a map zooms with its buttons, the wheel, a double click and the keys, and moves by dragging', async ({ page }) => {
    await open(page, '/rum/explorer?viz=map');

    const map = page.locator('[data-world-map]');
    const transform = () => page.locator('[data-map-viewport]').getAttribute('transform');
    await expect(map).toHaveAttribute('data-map-zoom', '1.00');
    await expect(page.getByRole('button', { name: 'Zoom out' })).toBeDisabled();
    await expect(page.getByRole('button', { name: 'Show the whole map' })).toBeDisabled();

    await page.getByRole('button', { name: 'Zoom in' }).click();
    await expect(map).toHaveAttribute('data-map-zoom', '1.50');
    await expect(page.getByRole('button', { name: 'Zoom out' })).toBeEnabled();

    // The wheel zooms about the pointer: the country under it is still under it.
    const china = page.locator('[data-rum-viz-body="map"] path[data-map-country="CN"]');
    const at = await pointInside(china);
    await page.mouse.move(at.x, at.y);
    await page.mouse.wheel(0, -400);
    await expect.poll(() => zoomOf(page)).toBeGreaterThan(1.5);
    expect(await page.evaluate(([x, y]) => document.elementFromPoint(x!, y!)?.getAttribute('data-map-country'), [at.x, at.y])).toBe('CN');

    // A drag moves a zoomed map.
    await page.getByRole('button', { name: 'Zoom in' }).click();
    const before = await transform();
    await page.mouse.move(at.x, at.y);
    await page.mouse.down();
    await page.mouse.move(at.x + 70, at.y + 30, { steps: 6 });
    await page.mouse.up();
    expect(await transform()).not.toBe(before);

    // The whole map again.
    await page.getByRole('button', { name: 'Show the whole map' }).click();
    await expect(map).toHaveAttribute('data-map-zoom', '1.00');
    expect(await transform()).toBe('translate(0 0) translate(500 227) scale(1) translate(-500 -227)');

    // A double click zooms in, and the keys zoom and move.
    await page.mouse.dblclick(at.x, at.y);
    await expect(map).toHaveAttribute('data-map-zoom', '1.50');
    await map.focus();
    await page.keyboard.press('+');
    await expect(map).toHaveAttribute('data-map-zoom', '2.25');
    const moved = await transform();
    await page.keyboard.press('ArrowLeft');
    expect(await transform()).not.toBe(moved);
    await page.keyboard.press('-');
    await expect.poll(() => zoomOf(page)).toBeLessThan(2.25);
    await page.keyboard.press('0');
    await expect(map).toHaveAttribute('data-map-zoom', '1.00');
  });

  test('leaving a type leaves the fields it was asked about, and keeps the way of drawing', async ({ page }) => {
    await open(page, `/rum/explorer?type=views&viz=table&m=count&m=p75:${LCP}&by=browser.name&bucket=1h&top=20`);

    await page.getByRole('tab', { name: 'Errors' }).click();
    await expect.poll(() => new URL(page.url()).searchParams.get('type')).toBe('errors');

    const params = new URL(page.url()).searchParams;
    expect(params.get('viz')).toBe('table');
    expect(params.has('m')).toBe(false);
    expect(params.has('by')).toBe(false);
    expect(params.get('top')).toBe('20');
    // The type opens a table of its own: ranked by what its rows are most often told apart by.
    await expect(page.locator('[data-rum-aggregate="table"]')).toContainText('error.type');
  });

  test('says when the numbers could not be counted in time, and does not wait', async ({ page }) => {
    await page.route(/\/api\/v1\/rum\/aggregate/, (route) =>
      route.fulfill({ json: { groupBy: [], measures: [{ name: 'count', fn: 'count', unit: 'count' }], rows: [], unavailable: 'time_budget' } }),
    );
    await open(page, '/rum/explorer?type=views&viz=toplist');

    await expect(page.locator('[data-rum-viz-unavailable="time_budget"]')).toBeVisible();
    await expect(page.locator('[data-rum-viz-unavailable]')).toContainText('Counting took longer than the time budget');
    await expect(status(page)).toHaveText('Not available');
  });
});

test.describe('RUM explorer: the rows drawn as numbers, in Chinese', () => {
  test('is said in Chinese', async ({ page, mockServer }) => {
    await page.addInitScript(() => {
      localStorage.setItem('molesignal-ui-prefs', JSON.stringify({ state: { language: 'zh-cn' }, version: 0 }));
    });
    await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });
    await open(page, `/rum/explorer?type=views&viz=timeseries&m=p75:${LCP}&by=view.url_path_group`);

    await expect(page.locator('[data-rum-results-head]')).toContainText('可视化为');
    await expect(page.locator('[data-rum-viz="toplist"]')).toHaveText('Top 列表');
    await expect(page.locator('[data-rum-aggregate]')).toContainText('时间桶');
    await expect(status(page)).toContainText('每点');
  });
});
