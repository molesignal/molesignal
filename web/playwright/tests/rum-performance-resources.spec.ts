import type { Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM, mockRumData } from '../fixtures/mockRumData';
import { after, axisEnd, duration, failed, grouped, ms, p75, percent, rate, size } from '../fixtures/rumFigures';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const DATA = mockRumData(NOW);

const WEB = MOCK_RUM.applications.web;
const REQUESTS = DATA.resources.filter((row) => row['application.id'] === WEB);

const HOST_TYPE = ['resource.url_host', 'resource.type'] as const;

/** What the table says of a group of requests: how many, their time, how they ended, their size and how many came from the cache. */
function figures(rows: typeof REQUESTS) {
  const answered = rows.filter((row) => typeof row['resource.status_code'] === 'number');
  const delivered = rows.filter((row) => row['resource.delivery_type'] !== undefined);
  const sizes = rows.map((row) => row['resource.size']).filter((value): value is number => typeof value === 'number');

  return {
    requests: rows.length,
    time: duration(p75(rows.map((row) => ms(row, 'resource.duration') as number)) as number),
    failed: answered.length === 0 ? '—' : rate(rows.filter(failed).length / answered.length),
    size: sizes.length === 0 ? '—' : size(p75(sizes)),
    cache: delivered.length === 0 ? '—' : percent(delivered.filter((row) => row['resource.delivery_type'] === 'cache').length / delivered.length),
  };
}

const key = (values: ReadonlyArray<string | null>) => JSON.stringify(values);

const rows = (page: Page) => page.locator('[data-rum-resources-table] [data-rum-row]');

const row = (page: Page, values: ReadonlyArray<string | null>) => page.locator(`[data-rum-resources-table] [data-rum-row='${key(values)}']`);

async function open(page: Page, path = '/rum/performance/resources') {
  await page.goto(path);
  await expect(page).toHaveURL(/[?&]time=/);
}

test.describe('RUM performance: resources', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('is a view of the performance tab, told by its own address, and carries the application from view to view', async ({ page }) => {
    await open(page, `/rum/performance?app=${WEB}`);

    const strip = page.locator('[data-rum-subnavigation="performance"]');

    await expect(strip.getByRole('link', { name: 'Web Vitals' })).toHaveAttribute('aria-current', 'page');
    await expect(page.locator('[data-rum-navigation="surface"] [aria-current="page"]').first()).toContainText('Performance');

    await strip.getByRole('link', { name: 'Resources' }).click();
    await expect(page).toHaveURL(new RegExp(`/rum/performance/resources\\?.*app=${WEB}`));
    // The range the page puts in its address a moment after it mounts would write over a click made before it.
    await expect(page).toHaveURL(/[?&]time=/);
    await expect(strip.getByRole('link', { name: 'Resources' })).toHaveAttribute('aria-current', 'page');
    // The tab of the page stays lit on the view under it.
    await expect(page.locator('[data-rum-navigation="surface"] [aria-current="page"]').first()).toContainText('Performance');

    await strip.getByRole('link', { name: 'Long tasks' }).click();
    await expect(page).toHaveURL(/\/rum\/performance\/long-tasks/);
    await expect(strip.getByRole('link', { name: 'Long tasks' })).toHaveAttribute('aria-current', 'page');
  });

  test('says what the pages load: how many requests, how long they took, how many failed, and what a page view asked for', async ({ page }) => {
    await open(page);

    const answered = REQUESTS.filter((row) => typeof row['resource.status_code'] === 'number');
    const views = new Set(REQUESTS.map((row) => row['view.id'])).size;
    const bytes = REQUESTS.reduce((sum, row) => sum + (typeof row['resource.size'] === 'number' ? (row['resource.size'] as number) : 0), 0);
    const statement = page.locator('[data-rum-statement]');

    await expect(statement).toContainText(`The pages made ${REQUESTS.length} requests`);
    await expect(statement).toContainText(`they took ${duration(p75(REQUESTS.map((row) => ms(row, 'resource.duration') as number)))}`);
    await expect(page.locator('[data-rum-failure-rate]')).toHaveText(rate(REQUESTS.filter(failed).length / answered.length));
    await expect(statement).toContainText(`A page view asked for ${Math.round(REQUESTS.length / views)} resources, ${size(bytes / views)} in all, on average.`);
  });

  test('lists the groups of requests by host and kind, the most requested first, each with how it went', async ({ page }) => {
    await open(page);

    const groups = grouped(REQUESTS, HOST_TYPE);

    await expect(rows(page)).toHaveCount(groups.length);
    expect(await rows(page).evaluateAll((elements) => elements.map((element) => element.getAttribute('data-rum-row')))).toEqual(groups.map((group) => key(group.values)));
    await expect(page.locator('[data-rum-table-count]')).toContainText(`${groups.length} groups`);

    for (const group of groups.slice(0, 4)) {
      const found = figures(group.rows);
      const cells = row(page, group.values).getByRole('cell');

      await expect(cells.nth(0)).toHaveText(group.values[0] as string);
      await expect(cells.nth(1)).toHaveText(group.values[1] as string);
      await expect(cells.nth(2)).toHaveText(String(found.requests));
      await expect(cells.nth(3)).toHaveText(found.time);
      await expect(cells.nth(4)).toHaveText(found.failed);
      await expect(cells.nth(5)).toHaveText(found.size);
      await expect(cells.nth(6)).toHaveText(found.cache);
    }
  });

  test('greys a group with a handful of requests, which no ranking puts first', async ({ page }) => {
    await open(page);

    const few = grouped(REQUESTS, HOST_TYPE).filter((group) => group.rows.length < 30);

    expect(few.length).toBeGreaterThan(0);

    for (const group of few) await expect(row(page, group.values)).toHaveAttribute('data-rum-few', '');

    // The slowest group has too few requests to be the slowest: the ranking leaves it at the end.
    await page.locator('[data-rum-column="p75"] button').click();
    await expect(page).toHaveURL(/[?&]sort=p75/);
    await expect(page.locator('[data-rum-column="p75"]')).toHaveAttribute('aria-sort', 'descending');

    const names = await rows(page).evaluateAll((elements) => elements.map((element) => ({ key: element.getAttribute('data-rum-row'), few: element.hasAttribute('data-rum-few') })));
    const firstFew = names.findIndex((entry) => entry.few);

    expect(firstFew).toBeGreaterThan(0);
    expect(names.slice(firstFew).every((entry) => entry.few)).toBe(true);

    await page.locator('[data-rum-column="p75"] button').click();
    await expect(page).toHaveURL(/[?&]dir=asc/);
  });

  test('narrows the requests by kind, by how they were answered and by words of their address', async ({ page }) => {
    await open(page);

    await page.locator('[data-rum-type]').click();
    await page.getByRole('option', { name: 'fetch', exact: true }).click();
    await expect(page).toHaveURL(/[?&]type=fetch/);

    const fetches = REQUESTS.filter((row) => row['resource.type'] === 'fetch');
    const groups = grouped(fetches, HOST_TYPE);

    await expect(rows(page)).toHaveCount(groups.length);
    expect(await rows(page).evaluateAll((elements) => elements.every((element) => (element.getAttribute('data-rum-row') ?? '').endsWith('"fetch"]')))).toBe(true);

    // The ones that answered with a server error.
    await page.locator('[data-rum-status]').click();
    await page.getByRole('option', { name: 'Server errors (5xx)' }).click();
    await expect(page).toHaveURL(/[?&]status=5xx/);

    const broken = grouped(
      fetches.filter((row) => typeof row['resource.status_code'] === 'number' && (row['resource.status_code'] as number) >= 500),
      HOST_TYPE,
    );

    await expect(rows(page)).toHaveCount(broken.length);
    await expect(row(page, broken[0]!.values).getByRole('cell').nth(4)).toHaveText('100%');

    // Words of the address find a host or a path, whatever the groups are.
    await page.locator('[data-rum-status]').click();
    await page.getByRole('option', { name: 'All' }).click();
    await expect(page).not.toHaveURL(/[?&]status=/);
    await page.getByPlaceholder('Search host or path').fill('recommend');
    await expect(page).toHaveURL(/[?&]q=recommend/);
    await expect(rows(page)).toHaveCount(1);
    await expect(row(page, ['api.example.com', 'fetch']).getByRole('cell').nth(2)).toHaveText(String(fetches.filter((entry) => String(entry['resource.url']).includes('recommend')).length));

    await page.getByPlaceholder('Search host or path').fill('zzz');
    await expect(page.locator('[data-rum-no-match], [data-product-state]').first()).toBeVisible();
  });

  test('tells the requests apart by another grouping: a path under its host, a protocol, how they were delivered', async ({ page }) => {
    await open(page);

    await page.locator('[data-rum-group]').click();
    await page.getByRole('option', { name: 'Host and path' }).click();
    await expect(page).toHaveURL(/[?&]by=path/);

    const paths = grouped(REQUESTS, ['resource.url_host', 'resource.url_path_group']);

    await expect(rows(page)).toHaveCount(paths.length);
    await expect(page.locator('[data-rum-column="requests"]')).toBeVisible();
    await expect(row(page, paths[0]!.values).getByRole('cell').nth(1)).toHaveText(paths[0]!.values[1] as string);

    await page.locator('[data-rum-group]').click();
    await page.getByRole('option', { name: 'Protocol' }).click();
    await expect(page).toHaveURL(/[?&]by=protocol/);
    await expect(rows(page)).toHaveCount(grouped(REQUESTS, ['resource.protocol']).length);

    // A request that says nothing of how it was delivered is a group of its own, said as none.
    await page.locator('[data-rum-group]').click();
    await page.getByRole('option', { name: 'Delivery' }).click();
    await expect(row(page, [null])).toContainText('(none)');
  });

  test('opens what the requests of a group are made of: how they went, their parts, how they were answered, the slowest', async ({ page }) => {
    await open(page);

    const group = grouped(REQUESTS, HOST_TYPE).find((entry) => entry.values[0] === 'api.example.com' && entry.values[1] === 'fetch')!;
    const found = figures(group.rows);

    await row(page, group.values).getByRole('button').click();
    await expect(page).toHaveURL(/[?&]open=/);

    const panel = page.locator('[data-rum-panel="resources"]');

    await expect(panel).toContainText('api.example.com');
    await expect(panel).toContainText(`${group.rows.length} requests`);
    await expect(panel.locator('[data-rum-panel-sentence]')).toContainText(`they took ${found.time}`);
    await expect(row(page, group.values)).toHaveClass(/floating-item-selected/);

    // The parts of the requests around the 75th percentile, in the order they happen.
    await expect(panel.locator('[data-rum-panel-section="parts"] [data-rum-phase]').first()).toBeVisible();
    await expect(panel.locator('[data-rum-panel-section="parts"]')).toContainText('First byte');
    await expect(panel.locator('[data-rum-panel-sentence]')).toContainText('The longest part is');

    // How they were answered: the commonest answer first, with its share of the requests that said.
    const answered = group.rows.filter((entry) => typeof entry['resource.status_code'] === 'number');
    const ok = answered.filter((entry) => entry['resource.status_code'] === 200).length;

    await expect(panel.locator('[data-rum-status-code]').first()).toContainText('200');
    await expect(panel.locator('[data-rum-status-code]').first()).toContainText(rate(ok / answered.length));
    await expect(panel.locator('[data-rum-status-code="500"]')).toBeVisible();

    // The slowest three, the slowest first.
    const slowest = [...group.rows].sort((a, b) => (ms(b, 'resource.duration') as number) - (ms(a, 'resource.duration') as number)).slice(0, 3);

    await expect(panel.locator('[data-rum-slow-request]')).toHaveCount(3);
    await expect(panel.locator('[data-rum-slow-request]').first()).toContainText(duration(ms(slowest[0]!, 'resource.duration') as number));

    // The trace id of the backend, in how many of the requests; and the way to see them in the explorer.
    await expect(panel).toContainText('of the requests carry a trace id');
    await expect(panel.getByRole('link', { name: 'See them in the explorer' })).toHaveAttribute('href', /\/rum\/explorer\?type=resources&q=.*resource\.url_host/);

    await panel.getByRole('button', { name: 'Close' }).click();
    await expect(page).not.toHaveURL(/[?&]open=/);
    await expect(panel).toHaveCount(0);
  });

  test('opens the load of the view that made a request, on that request, and lets go of it on Escape', async ({ page }) => {
    await open(page);

    const group = grouped(REQUESTS, HOST_TYPE).find((entry) => entry.values[0] === 'api.example.com' && entry.values[1] === 'fetch')!;
    const slowest = [...group.rows].sort((a, b) => (ms(b, 'resource.duration') as number) - (ms(a, 'resource.duration') as number))[0]!;
    const id = slowest['resource.id'] as string;
    const view = DATA.views.find((entry) => entry['view.id'] === slowest['view.id'])!;
    // The slowest request of an API is made by what the visitor did, a while after the page had loaded.
    const late = after(slowest, view) >= axisEnd(view);

    await row(page, group.values).getByRole('button').click();
    await page.locator(`[data-rum-slow-request="${id}"]`).click();
    await expect(page).toHaveURL(/[?&]view=/);
    await expect(page).toHaveURL(new RegExp(`[?&]focus=${id}`));

    const drawer = page.locator('[data-rum-drawer]');

    await expect(drawer.locator('[data-rum-waterfall]')).toBeVisible();
    await expect(drawer.locator('[data-rum-drawer-sections] [data-tab-active="true"]')).toContainText('Load waterfall');
    // The drawer is named by the page the view is of, not by its id.
    await expect(drawer.getByText(String(view['view.name']), { exact: true }).first()).toBeVisible();

    // The request is the one that was chosen: its panel says what it is, and where it was not drawn it says why.
    const detail = drawer.locator('[data-rum-waterfall-detail="resource"]');

    await expect(detail).toContainText(String(slowest['resource.url']));
    await expect(drawer.locator(`[data-rum-waterfall-row="${id}"]`)).toHaveCount(late ? 0 : 1);
    await expect(drawer.locator('[data-rum-waterfall-after-load]')).toHaveCount(late ? 1 : 0);

    await page.keyboard.press('Escape');
    await expect(page).not.toHaveURL(/[?&]view=/);
    await expect(page).not.toHaveURL(/[?&]focus=/);
    await expect(page).toHaveURL(/[?&]open=/);
    await expect(drawer).toHaveCount(0);
  });

  test('is the view the address says it is, so that a copy of it opens the same', async ({ page }) => {
    await open(page, `/rum/performance/resources?by=type&type=js&sort=p75&open=${encodeURIComponent('["js"]')}`);

    await expect(page.locator('[data-rum-type]')).toContainText('js');
    await expect(page.locator('[data-rum-column="p75"]')).toHaveAttribute('aria-sort', 'descending');
    await expect(page.locator('[data-rum-panel="resources"]')).toBeVisible();
    await expect(row(page, ['js'])).toHaveClass(/floating-item-selected/);
  });

  test('has nothing to say of an application that does not send from a browser, and of one nothing ran in', async ({ page }) => {
    await open(page, `/rum/performance/resources?app=${MOCK_RUM.applications.app}`);
    await expect(page.getByText('Web Vitals are measured in browsers')).toBeVisible();
    await expect(page.locator('[data-rum-statement]')).toHaveCount(0);
    await expect(page.locator('[data-rum-subnavigation="performance"]')).toHaveCount(0);

    await page.goto('/rum/performance/resources?env=staging');
    await expect(page.getByText('No requests in this range')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Expand the time range' })).toBeVisible();
  });

  test('offers another go for the table whose read failed, and keeps the statement', async ({ page }) => {
    await page.route(/\/api\/v1\/rum\/aggregate\?.*groupBy/, (route) => route.fulfill({ status: 500, json: { error: 'internal', message: 'the engine is busy' } }));
    await open(page);

    await expect(page.locator('[data-rum-overview-card="resources-table"]').getByRole('button', { name: 'Retry' })).toBeVisible();
    await expect(page.locator('[data-rum-statement]')).toBeVisible();
  });

  test('says why a panel has no numbers when counting took too long', async ({ page }) => {
    await open(page);

    const group = grouped(REQUESTS, HOST_TYPE)[0]!;

    await page.route(/\/api\/v1\/rum\/aggregate\?.*bucketTime/, (route) =>
      route.fulfill({ json: { groupBy: [], measures: [], rows: [], unavailable: 'time_budget' } }),
    );
    await row(page, group.values).getByRole('button').click();

    await expect(page.locator('[data-rum-panel-section="trend"]')).toContainText('Counting took longer than the time budget');
    await expect(page.locator('[data-rum-panel-section="statuses"]')).toBeVisible();
  });
});

test.describe('RUM performance: resources in Chinese', () => {
  test('says its words in the language of the product', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });
    await open(page);

    await expect(page.locator('[data-rum-subnavigation="performance"]')).toContainText('资源');
    await expect(page.locator('[data-rum-statement]')).toContainText(`页面共发出 ${REQUESTS.length} 次请求`);
    await expect(page.locator('[data-rum-column="requests"]')).toContainText('请求');
    await expect(page.locator('[data-rum-column="failed"]')).toContainText('失败率');
    await expect(page.locator('[data-rum-column="cache"]')).toContainText('缓存命中');

    await rows(page).first().getByRole('button').click();
    await expect(page.locator('[data-rum-panel="resources"]')).toContainText('最慢的请求');
  });
});
