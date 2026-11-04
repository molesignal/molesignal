import type { Page } from '@playwright/test';

import type { RumRow, RumType } from '../../src/api/rum';
import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM, mockRumData } from '../fixtures/mockRumData';
import { duration, failed, ms } from '../fixtures/rumFigures';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const DATA = mockRumData(NOW);

const CHECKOUT = MOCK_RUM.session.checkout;
const ANONYMOUS = MOCK_RUM.session.anonymous;

const of = (type: RumType, session: string): RumRow[] => DATA[type].filter((row) => row['session.id'] === session).sort((a, b) => a.timestamp - b.timestamp);

const sessionRow = (session: string) => DATA.sessions.find((row) => row['session.id'] === session)!;
const startOf = (session: string) => Number(sessionRow(session)['session.start']) * 1_000;

const viewsOf = (session: string) => of('views', session);
const viewAt = (session: string, path: string) => viewsOf(session).find((row) => row['view.url_path'] === path)!;
const requestsOf = (view: RumRow) => DATA.resources.filter((row) => row['view.id'] === view['view.id']).sort((a, b) => a.timestamp - b.timestamp);

/** A moment of the recording as the progress of the player reads it: minutes and seconds, each of two digits. */
const mmss = (micros: number) => {
  const seconds = Math.floor(micros / 1_000_000);

  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`;
};

/** A moment of the session as a list reads it: minutes, seconds and a tenth. */
const clock = (micros: number) => {
  const tenths = Math.floor(micros / 100_000);

  return `${Math.floor(tenths / 600)}:${String(Math.floor(tenths / 10) % 60).padStart(2, '0')}.${tenths % 10}`;
};

const tools = (page: Page) => page.locator('[data-rum-devtools]');
const tab = (page: Page, name: string) => page.locator('[data-rum-tool-tabs]').getByRole('button', { name: new RegExp(`^${name}`) });
const position = (page: Page) => page.getByText(/^\d\d:\d\d \/ \d\d:\d\d$/);

async function open(page: Page, path: string) {
  await page.goto(path);
  await expect(tools(page)).toBeVisible();
}

test.describe('RUM session: developer tools', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('has three tabs with their counts, and opens on the errors where the session ran into one', async ({ page }) => {
    await open(page, `/rum/sessions/view/${CHECKOUT}`);

    const first = viewsOf(CHECKOUT)[0]!;

    await expect(tools(page)).toHaveAttribute('data-rum-tool-tab', 'errors');
    await expect(tab(page, 'Errors')).toContainText(String(of('errors', CHECKOUT).length));
    // The requests are those of one view: the one the replay is in, which is the first at the start.
    await expect(tab(page, 'Network')).toContainText(String(requestsOf(first).length));
    await expect(tab(page, 'Performance')).toContainText(String(viewsOf(CHECKOUT).length));
    await expect(page.locator('[data-rum-tool-tabs] [data-tab]')).toHaveCount(5);

    const error = of('errors', CHECKOUT)[0]!;
    const row = page.locator('[data-rum-error]').first();

    await expect(row).toContainText(String(error['error.message']));
    await expect(row).toContainText(String(error['error.type']));
    await expect(row.getByRole('link')).toContainText(`issue ${String(error['error.issue_id']).slice(0, 8)}`);
  });

  test('opens on the requests where the session ran into no error', async ({ page }) => {
    await open(page, `/rum/sessions/view/${ANONYMOUS}`);

    await expect(tools(page)).toHaveAttribute('data-rum-tool-tab', 'network');
    await expect(tab(page, 'Errors')).toContainText('0');
    await tab(page, 'Errors').click();
    await expect(page.locator('[data-rum-tool-empty]')).toHaveText('This session ran into no errors');
  });

  test('ticks the ruler of the replay where it went wrong, where a sign of frustration was shown, and where the page changed', async ({ page }) => {
    await open(page, `/rum/sessions/view/${CHECKOUT}`);

    const signs = of('actions', CHECKOUT).filter((row) => Number(row['action.frustration.count']) > 0).length;

    await expect(page.locator('[data-replay-tick="error"]')).toHaveCount(of('errors', CHECKOUT).length);
    await expect(page.locator('[data-replay-tick="frustration"]')).toHaveCount(signs);
    await expect(page.locator('[data-replay-tick="view"]')).toHaveCount(viewsOf(CHECKOUT).length - 1);
  });

  test('moves the replay to the moment of a row, pauses it there, and lights the row', async ({ page }) => {
    await open(page, `/rum/sessions/view/${CHECKOUT}`);

    const error = of('errors', CHECKOUT)[0]!;
    const at = error.timestamp - startOf(CHECKOUT);

    await page.locator('[data-rum-error] [data-rum-seek]').first().click();

    await expect(position(page)).toHaveText(new RegExp(`^${mmss(at)} / `));
    await expect(page.getByRole('button', { name: 'Play', exact: true })).toBeVisible();
    await expect(page.locator('[data-rum-error]').first()).toHaveAttribute('data-current', 'true');
    await expect(page.locator('[data-rum-error] [data-rum-seek]').first()).toHaveText(clock(at));
  });

  test('lists the requests of the view the replay is in, lights the one it is at, and changes view as the replay does', async ({ page }) => {
    await open(page, `/rum/sessions/view/${CHECKOUT}`);
    await tab(page, 'Network').click();
    await expect(page).toHaveURL(/[?&]tab=network/);

    const products = viewAt(CHECKOUT, '/products');
    const checkout = viewAt(CHECKOUT, '/checkout');

    await expect(page.locator('[data-rum-tool-view]')).toContainText('Products');
    await expect(page.locator('[data-rum-request]')).toHaveCount(requestsOf(products).length);

    // The replay goes to the moment of the error, which was on the checkout page.
    await tab(page, 'Errors').click();
    await page.locator('[data-rum-error] [data-rum-seek]').first().click();
    await tab(page, 'Network').click();

    const error = of('errors', CHECKOUT)[0]!;
    const requests = requestsOf(checkout);
    const lit = requests.filter((row) => row.timestamp <= error.timestamp).at(-1)!;

    await expect(page.locator('[data-rum-tool-view]')).toContainText('Checkout');
    await expect(page.locator('[data-rum-request]')).toHaveCount(requests.length);
    await expect(page.locator('[data-rum-request][data-current="true"]')).toHaveCount(1);
    await expect(page.locator('[data-rum-request][data-current="true"]')).toHaveAttribute('data-rum-request', String(lit['resource.id']));
    // The request that failed is told by its status.
    await expect(page.locator('[data-rum-request][data-failed]')).toHaveCount(requests.filter((row) => Number(row['resource.status_code']) >= 400).length);
    await expect(tab(page, 'Network')).toContainText(String(requests.length));
  });

  test('says of a request when it began, how it was made, what it was, how it ended, how long it took and how big it was', async ({ page }) => {
    await open(page, `/rum/sessions/view/${CHECKOUT}?tab=network`);

    const first = requestsOf(viewAt(CHECKOUT, '/products'))[0]!;
    const row = page.locator('[data-rum-request]').first();
    const cells = row.getByRole('cell');

    await expect(cells.nth(1)).toHaveText(String(first['resource.method']));
    await expect(cells.nth(2)).toContainText(String(first['resource.url_host']));
    await expect(cells.nth(2)).toContainText(String(first['resource.url_path']));
    await expect(cells.nth(3)).toHaveText(String(first['resource.status_code']));
    await expect(cells.nth(4)).toHaveText(String(first['resource.type']));
    await expect(cells.nth(5)).toHaveText(duration(ms(first, 'resource.duration') as number));
  });

  test('is of the view that was chosen until the replay is given back, and of every view when that is chosen', async ({ page }) => {
    await open(page, `/rum/sessions/view/${CHECKOUT}?tab=network`);

    const cart = viewAt(CHECKOUT, '/cart');

    await page.locator('[data-rum-tool-view]').click();
    await expect(page.getByRole('option')).toHaveText([/^All views$/, /^Products · /, /^Cart · /, /^Checkout · /]);
    await page.getByRole('option', { name: /^Cart · / }).click();
    await expect(page).toHaveURL(new RegExp(`[?&]view=${cart['view.id']}`));
    await expect(page.locator('[data-rum-request]')).toHaveCount(requestsOf(cart).length);

    // Another view is not followed from the replay; the replay can be followed again.
    await page.locator('[data-rum-tool-view]').click();
    await expect(page.getByRole('option', { name: 'Follow the replay' })).toBeVisible();
    await page.getByRole('option', { name: 'All views' }).click();
    await expect(page).toHaveURL(/[?&]view=all/);
    await expect(page.locator('[data-rum-request]')).toHaveCount(of('resources', CHECKOUT).length);
    await expect(tab(page, 'Network')).toContainText(String(of('resources', CHECKOUT).length));
    // The bars are on the time of one view: not where every view is listed.
    await expect(page.locator('[data-rum-tool-display]')).toHaveCount(0);

    await page.locator('[data-rum-tool-view]').click();
    await page.getByRole('option', { name: 'Follow the replay' }).click();
    await expect(page).not.toHaveURL(/[?&]view=/);
    await expect(page.locator('[data-rum-tool-view]')).toContainText('Products');
  });

  test('narrows the requests of a view to the slow ones or the ones that failed, and the address says which', async ({ page }) => {
    const checkout = viewAt(CHECKOUT, '/checkout');
    const requests = requestsOf(checkout);

    await open(page, `/rum/sessions/view/${CHECKOUT}?tab=network&view=${checkout['view.id']}`);
    await expect(page.locator('[data-rum-request]')).toHaveCount(requests.length);

    await page.locator('[data-rum-tool-show]').click();
    await expect(page.getByRole('option')).toHaveText(['All', 'Slow (500 ms or more)', 'Failed']);
    await page.getByRole('option', { name: 'Failed' }).click();
    await expect(page).toHaveURL(/[?&]show=failed/);
    await expect(page.locator('[data-rum-request]')).toHaveCount(requests.filter(failed).length);
    await expect(page.locator('[data-rum-request][data-failed]')).toHaveCount(requests.filter(failed).length);
    // The tab still counts the requests of the view, and the bars keep the ruler of all of them.
    await expect(tab(page, 'Network')).toContainText(String(requests.length));

    await page.locator('[data-rum-tool-show]').click();
    await page.getByRole('option', { name: /^Slow/ }).click();
    await expect(page).toHaveURL(/[?&]show=slow/);
    await expect(page.locator('[data-rum-request]')).toHaveCount(requests.filter((row) => (ms(row, 'resource.duration') as number) >= 500).length);

    await page.locator('[data-rum-tool-show]').click();
    await page.getByRole('option', { name: 'All', exact: true }).click();
    await expect(page).not.toHaveURL(/[?&]show=/);
    await expect(page.locator('[data-rum-request]')).toHaveCount(requests.length);

    // A view whose requests all went well has none that failed, and says so.
    const products = viewAt(CHECKOUT, '/products');

    await open(page, `/rum/sessions/view/${CHECKOUT}?tab=network&view=${products['view.id']}&show=failed`);
    await expect(page.locator('[data-rum-tool-empty]')).toHaveText('No request matches');
    // The waterfall has filters of its own: the list's is not offered over it.
    await page.locator('[data-rum-tool-display]').getByRole('button', { name: 'Waterfall' }).click();
    await expect(page.locator('[data-rum-tool-show]')).toHaveCount(0);
  });

  test('shows the load of a view as the waterfall of its requests, and back as a list', async ({ page }) => {
    await open(page, `/rum/sessions/view/${CHECKOUT}?tab=network`);

    await page.locator('[data-rum-tool-display]').getByRole('button', { name: 'Waterfall' }).click();
    await expect(page).toHaveURL(/[?&]display=waterfall/);

    const waterfall = page.locator('[data-rum-tool-waterfall] [data-rum-waterfall]');

    await expect(waterfall).toBeVisible();
    await expect(page.locator('[data-rum-tool-waterfall] [data-rum-waterfall-sentence]')).toContainText('This view loaded');
    await expect(page.locator('[data-rum-request]')).toHaveCount(0);

    await page.locator('[data-rum-tool-display]').getByRole('button', { name: 'List' }).click();
    await expect(page).not.toHaveURL(/[?&]display=/);
    await expect(page.locator('[data-rum-request]').first()).toBeVisible();
    await expect(waterfall).toHaveCount(0);
  });

  test('lists how each view of the session did, grades the levels, and opens the load of a view', async ({ page }) => {
    await open(page, `/rum/sessions/view/${CHECKOUT}`);
    await tab(page, 'Performance').click();
    await expect(page).toHaveURL(/[?&]tab=performance/);

    const views = viewsOf(CHECKOUT);
    const checkout = viewAt(CHECKOUT, '/checkout');
    const rows = page.locator('[data-rum-view]');

    await expect(rows).toHaveCount(views.length);
    await expect(rows.nth(0)).toHaveAttribute('data-current', 'true');

    // A level is told in the ink of its grade: the checkout page needs improvement, the first page is good.
    const row = page.locator(`[data-rum-view="${checkout['view.id']}"]`);

    await expect(row.getByRole('cell').nth(1)).toHaveText(duration(ms(checkout, 'view.performance.lcp.timestamp') as number));
    await expect(row.getByRole('cell').nth(1).locator('span')).toHaveClass(/yellow-ink/);
    await expect(row.getByRole('cell').nth(2).locator('span')).toHaveClass(/yellow-ink/);
    await expect(rows.nth(0).getByRole('cell').nth(1).locator('span')).toHaveClass(/green-ink/);
    await expect(row.getByRole('cell').nth(4)).toHaveText(String(requestsOf(checkout).length));

    await row.getByRole('button').click();
    await expect(page).toHaveURL(new RegExp(`[?&]tab=network.*view=${checkout['view.id']}.*display=waterfall`));
    await expect(page.locator('[data-rum-tool-waterfall] [data-rum-waterfall]')).toBeVisible();
    await expect(page.locator('[data-rum-tool-view]')).toContainText('Checkout');
  });

  test('is what the address says it is, so that a copy of it opens the same', async ({ page }) => {
    const cart = viewAt(CHECKOUT, '/cart');

    await open(page, `/rum/sessions/view/${CHECKOUT}?tab=network&view=${cart['view.id']}&display=waterfall`);

    await expect(tools(page)).toHaveAttribute('data-rum-tool-tab', 'network');
    await expect(page.locator('[data-rum-tool-view]')).toContainText('Cart');
    await expect(page.locator('[data-rum-tool-waterfall] [data-rum-waterfall]')).toBeVisible();
    await expect(page.locator('[data-rum-tool-display] [aria-pressed="true"]')).toContainText('Waterfall');
  });

  test('opens the replay at the moment the address names, and waits there', async ({ page }) => {
    const at = startOf(CHECKOUT) + 3 * 60 * 1_000_000 + 7_000_000;

    await open(page, `/rum/sessions/view/${CHECKOUT}?at=${at}`);

    await expect(position(page)).toHaveText(new RegExp(`^03:07 / `));
    await expect(page.getByRole('button', { name: 'Play', exact: true })).toBeVisible();
  });

  test('opens the issue an error was grouped into on the errors page', async ({ page }) => {
    await open(page, `/rum/sessions/view/${CHECKOUT}`);

    const issue = String(of('errors', CHECKOUT)[0]!['error.issue_id']);

    await page.locator('[data-rum-error]').first().getByRole('link').click();
    await expect(page).toHaveURL(new RegExp(`/rum/errors\\?.*issue=${issue}`));
  });

  test('has nothing to follow or move where the session recorded no replay: its rows only tell what happened', async ({ page }) => {
    await open(page, `/rum/sessions/view/${ANONYMOUS}`);

    await expect(page.locator('[data-rum-seek]')).toHaveCount(0);
    await expect(page.locator('[data-current="true"]')).toHaveCount(0);
    await expect(page.locator('[data-replay-tick]')).toHaveCount(0);
    // Every request of the session is listed, since there is no replay to say which view is in sight.
    await expect(page.locator('[data-rum-tool-view]')).toContainText('All views');
    await expect(page.locator('[data-rum-request]')).toHaveCount(of('resources', ANONYMOUS).length);
    // A time is from the start of the session.
    await expect(page.locator('[data-rum-request]').first().getByRole('cell').first()).toHaveText(
      clock(of('resources', ANONYMOUS)[0]!.timestamp - Number(sessionRow(ANONYMOUS).timestamp)),
    );
  });

  test('says that a list holds only the first rows when the timeline was cut short', async ({ page, mockServer }) => {
    await page.route(new RegExp(`/api/v1/rum/sessions/${CHECKOUT}/timeline`), async (route) => {
      const url = new URL(route.request().url());
      const response = await route.fetch({ url: `http://127.0.0.1:${mockServer.port}${url.pathname}${url.search}` });
      const body = (await response.json()) as { truncated: string[] };

      await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ ...body, truncated: ['resources'] }) });
    });
    await open(page, `/rum/sessions/view/${CHECKOUT}?tab=network`);

    await expect(page.locator('[data-rum-tool-notice]')).toContainText('Only the first rows of Resources are listed.');
  });
});

test.describe('RUM session: developer tools in Chinese', () => {
  test('says its words in the language of the product', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });
    await open(page, `/rum/sessions/view/${CHECKOUT}`);

    await expect(tab(page, '错误')).toBeVisible();
    await expect(tab(page, '网络')).toBeVisible();
    await expect(tab(page, '性能')).toBeVisible();
    await expect(page.locator('[data-rum-tool-tabs]')).not.toContainText('控制台');

    await tab(page, '网络').click();
    await expect(page.locator('[data-rum-tool-view]')).toContainText('视图');
    await expect(page.locator('[data-rum-tool-list] [role="columnheader"]').first()).toHaveText('时间');
    await expect(page.locator('[data-rum-tool-display]')).toContainText('瀑布');
  });
});
