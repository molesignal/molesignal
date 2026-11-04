import type { Locator, Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM, mockRumData } from '../fixtures/mockRumData';
import { after, axisEnd, duration } from '../fixtures/rumFigures';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const DATA = mockRumData(NOW);

/** The checkout view of the recorded visit that ended in a failed order, in Chrome: every moment and every part reported. */
const CHECKOUT = DATA.views.find((view) => view['session.id'] === MOCK_RUM.session.checkout && view['view.url_path'] === '/checkout')!;
const CHECKOUT_ID = CHECKOUT['view.id'] as string;
const AXIS = axisEnd(CHECKOUT);

const REQUESTS = DATA.resources.filter((row) => row['view.id'] === CHECKOUT_ID).sort((a, b) => a.timestamp - b.timestamp);
const TASKS = DATA.long_tasks.filter((row) => row['view.id'] === CHECKOUT_ID);
const DRAWN = REQUESTS.filter((row) => after(row, CHECKOUT) < AXIS);

/** A view of a browser that reports neither the moments nor the parts of its load. */
const SAFARI = DATA.views.find((view) => view['browser.name'] === 'Safari' && view['view.url_path'] === '/cart')!;
/** A screen of an app: it has no page to load. */
const SCREEN = DATA.views.find((view) => view['application.id'] === MOCK_RUM.applications.app)!;

async function openView(page: Page, view: (typeof DATA.views)[number]) {
  const id = view['view.id'] as string;
  const application = view['application.id'] as string;

  await page.goto(`/rum/explorer?type=views&q=${encodeURIComponent(`application.id = '${application}' AND view.id = '${id}'`)}`);
  await expect(page).toHaveURL(/[?&]time=/);

  // The one row of so narrow a query stands under the pagination bar in a window this short: it is clicked as an event, not by a pointer.
  await page.locator(`[data-rum-row][data-rum-id="${id}"]`).evaluate((element) => (element as HTMLElement).click());
}

const drawer = (page: Page) => page.locator('[data-rum-drawer]');

const rows = (page: Page) => drawer(page).locator('[data-rum-waterfall-row]');

/** The names of the rows in the order they stand. */
const ids = (locator: Locator) => locator.evaluateAll((elements) => elements.map((element) => element.getAttribute('data-rum-waterfall-row')));

test.describe('RUM view drawer: the load waterfall', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('opens a view of a browser on how its page loaded, with its fields in the next section', async ({ page }) => {
    await openView(page, CHECKOUT);

    const sections = drawer(page).locator('[data-rum-drawer-sections]');

    await expect(sections.locator('[data-tab-active="true"]')).toContainText('Load waterfall');
    await expect(sections).toContainText('Fields');
    await expect(drawer(page).locator('[data-rum-waterfall]')).toBeVisible();
    // The drawer is named by the page it is of, and is wide enough for a chart and its panel.
    await expect(drawer(page).getByText('Checkout', { exact: true }).first()).toBeVisible();
    expect((await drawer(page).boundingBox())!.width).toBeGreaterThan(900);

    await sections.getByRole('button', { name: 'Fields' }).click();
    await expect(drawer(page).locator('[data-rum-field-group]').first()).toBeVisible();
    await expect(drawer(page).locator('[data-rum-waterfall]')).toHaveCount(0);

    await sections.getByRole('button', { name: 'Load waterfall' }).click();
    await expect(drawer(page).locator('[data-rum-waterfall]')).toBeVisible();
  });

  test('says how the load went: how long it took, the first byte, the largest paint and the element, what it asked for', async ({ page }) => {
    await openView(page, CHECKOUT);

    const sentence = drawer(page).locator('[data-rum-waterfall-sentence]');
    const loading = (CHECKOUT['view.loading_time'] as number) / 1_000_000;
    const firstByte = (CHECKOUT['view.first_byte'] as number) / 1_000_000;
    const lcp = (CHECKOUT['view.performance.lcp.timestamp'] as number) / 1_000_000;

    await expect(sentence).toContainText(`This view loaded in ${duration(loading)}`);
    await expect(sentence).toContainText(`the first byte came after ${duration(firstByte)}`);
    await expect(sentence).toContainText(`LCP was ${duration(lcp)} (Needs improvement, div.order-summary)`);
    await expect(sentence).toContainText(`asked for ${CHECKOUT['view.resource.count']} resources and ran into ${TASKS.length} long tasks`);
  });

  test('marks the moments the load went through, each at the time it was, on an axis of whole seconds', async ({ page }) => {
    await openView(page, CHECKOUT);

    const marks = drawer(page).locator('[data-rum-mark]');
    const fields: Array<[string, string, string]> = [
      ['first_byte', 'First byte', 'view.first_byte'],
      ['fcp', 'FCP', 'view.performance.fcp.timestamp'],
      ['dcl', 'DCL', 'view.dom_content_loaded'],
      ['lcp', 'LCP', 'view.performance.lcp.timestamp'],
      ['load', 'Load', 'view.load_event'],
    ];

    await expect(marks).toHaveCount(fields.length);

    for (const [mark, name, field] of fields) {
      await expect(drawer(page).locator(`[data-rum-mark="${mark}"]`)).toHaveText(`${name} ${duration((CHECKOUT[field] as number) / 1_000_000)}`);
    }

    // The axis ends after the last moment, in whole steps.
    expect(AXIS).toBe(4_000);
    await expect(drawer(page).getByText('4 s', { exact: true })).toBeVisible();
  });

  test('draws one row for each request that began before the load ended, in the order they began, and says how many began after', async ({ page }) => {
    await openView(page, CHECKOUT);

    expect(DRAWN.length).toBeGreaterThan(0);
    expect(DRAWN.length).toBeLessThan(REQUESTS.length);
    await expect(rows(page)).toHaveCount(DRAWN.length);
    expect(await ids(rows(page))).toEqual(DRAWN.map((row) => row['resource.id']));

    // The request made after the page had loaded is told in a line of its own, not drawn.
    await expect(drawer(page).locator('[data-rum-waterfall-late]')).toHaveText(`${REQUESTS.length - DRAWN.length} more began after the page had loaded`);

    // Each is cut where the first byte came: the wait, and the download after it.
    const first = rows(page).first();
    const bar = first.locator('span[aria-hidden="true"]').first();

    await expect(bar.locator('span')).toHaveCount(2);
    await expect(first).toContainText(String(DRAWN[0]!['resource.type']));
    await expect(first).toContainText(`${Math.round((DRAWN[0]!['resource.duration'] as number) / 1_000_000)} ms`);
  });

  test('shows the long tasks of the main thread on the same axis, and the render-blocking requests say so', async ({ page }) => {
    await openView(page, CHECKOUT);

    const lane = drawer(page).locator('[data-rum-waterfall-lane]');

    await expect(lane).toContainText('Main thread');
    await expect(lane).toContainText(`${TASKS.length} long tasks`);
    await expect(lane.locator('[data-rum-waterfall-task]')).toHaveCount(TASKS.length);

    const blocking = DRAWN.filter((row) => row['resource.render_blocking_status'] === 'blocking');

    expect(blocking.length).toBeGreaterThan(0);

    for (const row of blocking) await expect(drawer(page).locator(`[data-rum-waterfall-row="${row['resource.id']}"]`)).toContainText('blocking');
  });

  test('opens a request at its right: what it is, how it ended, how big, how it was delivered and what it took its time on', async ({ page }) => {
    await openView(page, CHECKOUT);

    const request = DRAWN.find((row) => row['resource.type'] === 'fetch')!;

    await drawer(page).locator(`[data-rum-waterfall-row="${request['resource.id']}"]`).click();

    const detail = drawer(page).locator('[data-rum-waterfall-detail="resource"]');

    await expect(detail).toContainText(String(request['resource.url']));
    await expect(detail).toContainText('200');
    await expect(detail).toContainText('h2');
    await expect(detail.locator('[data-rum-phase]').first()).toBeVisible();
    await expect(detail.locator('[data-rum-phase="first_byte"]')).toBeVisible();
    await expect(drawer(page).locator(`[data-rum-waterfall-row="${request['resource.id']}"]`)).toHaveAttribute('aria-pressed', 'true');

    await detail.getByRole('button', { name: 'Close' }).click();
    await expect(detail).toHaveCount(0);
  });

  test('narrows the requests by kind and by what the reader looks for, and folds all but the first fifteen', async ({ page }) => {
    // A page that asks for forty things, so that there is something to fold.
    await page.route(/\/api\/v1\/rum\/search\?.*type=resources/, (route) => {
      const items = Array.from({ length: 40 }, (_, index) => ({
        timestamp: CHECKOUT.timestamp + (100 + index * 60) * 1_000,
        'resource.id': `synthetic-${String(index).padStart(2, '0')}`,
        'resource.type': index % 5 === 0 ? 'image' : 'js',
        'resource.url': `https://cdn.example.com/chunk-${index}.js`,
        'resource.url_host': 'cdn.example.com',
        'resource.url_path': `/chunk-${index}.js`,
        'resource.status_code': index === 7 ? 500 : 200,
        'resource.duration': (index === 3 ? 900 : 120) * 1_000_000,
        'resource.download.duration': 40 * 1_000_000,
        'resource.render_blocking_status': index === 1 ? 'blocking' : 'non-blocking',
      }));

      return route.fulfill({ json: { items, hasMore: false, nextCursor: null, previousCursor: null } });
    });
    await openView(page, CHECKOUT);

    await expect(rows(page)).toHaveCount(15);
    await expect(drawer(page)).toContainText('25 more resources, most of them done within 200 ms');

    await drawer(page).getByRole('button', { name: 'Show them' }).click();
    await expect(rows(page)).toHaveCount(40);
    await expect(drawer(page).getByRole('button', { name: 'Show fewer' })).toBeVisible();

    await drawer(page).getByRole('button', { name: 'Show fewer' }).click();
    await expect(rows(page)).toHaveCount(15);

    // By kind.
    await drawer(page).locator('[data-rum-waterfall-type]').click();
    await page.getByRole('option', { name: 'image', exact: true }).click();
    await expect(rows(page)).toHaveCount(8);

    // By what the reader looks for: the slow ones, the failed ones, the ones that block the first paint.
    await drawer(page).locator('[data-rum-waterfall-type]').click();
    await page.getByRole('option', { name: 'All' }).click();
    await drawer(page).locator('[data-rum-waterfall-show]').click();
    await page.getByRole('option', { name: 'Slow (500 ms or more)' }).click();
    expect(await ids(rows(page))).toEqual(['synthetic-03']);

    await drawer(page).locator('[data-rum-waterfall-show]').click();
    await page.getByRole('option', { name: 'Failed' }).click();
    expect(await ids(rows(page))).toEqual(['synthetic-07']);

    await drawer(page).locator('[data-rum-waterfall-show]').click();
    await page.getByRole('option', { name: 'Render-blocking' }).click();
    expect(await ids(rows(page))).toEqual(['synthetic-01']);

    await drawer(page).locator('[data-rum-waterfall-type]').click();
    await page.getByRole('option', { name: 'image', exact: true }).click();
    await expect(drawer(page).getByText('No resource matches')).toBeVisible();
  });

  test('says the largest paint without its element when the browser names none, and still draws what the view asked for', async ({ page }) => {
    await openView(page, SAFARI);

    const lcp = (SAFARI['view.performance.lcp.timestamp'] as number) / 1_000_000;

    expect(SAFARI['view.performance.lcp.target_selector']).toBeUndefined();
    await expect(drawer(page).locator('[data-rum-mark]')).toHaveCount(5);
    await expect(drawer(page).locator('[data-rum-waterfall-sentence]')).toContainText(`LCP was ${duration(lcp)} (Good).`);
    await expect(drawer(page).locator('[data-rum-waterfall-sentence] code')).toHaveCount(0);
    await expect(rows(page).first()).toBeVisible();
    // A browser that reports no long tasks has none on its main thread.
    await expect(drawer(page).locator('[data-rum-waterfall-lane]')).toContainText('0 long tasks');
  });

  test('has no waterfall for a screen of an app, which has no page to load', async ({ page }) => {
    await openView(page, SCREEN);

    await expect(drawer(page).locator('[data-rum-field-group]').first()).toBeVisible();
    await expect(drawer(page).locator('[data-rum-drawer-sections]')).toHaveCount(0);
    await expect(drawer(page).locator('[data-rum-waterfall]')).toHaveCount(0);
  });

  test('says why the requests cannot be shown when they could not be read, and offers another go', async ({ page }) => {
    await page.route(/\/api\/v1\/rum\/search\?.*type=resources/, (route) => route.fulfill({ status: 500, json: { error: 'internal', message: 'the engine is busy' } }));
    await openView(page, CHECKOUT);

    await expect(drawer(page).getByText('The requests of this view could not be read.')).toBeVisible();
    await expect(drawer(page).getByRole('button', { name: 'Retry' })).toBeVisible();
    // What the view says of itself does not wait for them.
    await expect(drawer(page).locator('[data-rum-waterfall-sentence]')).toBeVisible();
  });
});

test.describe('RUM view drawer in Chinese', () => {
  test('says its words in the language of the product', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });
    await openView(page, CHECKOUT);

    const sections = drawer(page).locator('[data-rum-drawer-sections]');

    await expect(sections).toContainText('加载瀑布');
    await expect(sections).toContainText('属性');
    await expect(drawer(page).locator('[data-rum-waterfall-sentence]')).toContainText(`这次浏览 ${duration((CHECKOUT['view.loading_time'] as number) / 1_000_000)} 加载完`);
    await expect(drawer(page).locator('[data-rum-waterfall-lane]')).toContainText('主线程');
    await expect(drawer(page).locator('[data-rum-waterfall-late]')).toContainText('个资源在页面加载之后才发起');
  });
});
