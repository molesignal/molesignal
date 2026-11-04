import type { Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM, mockRumData } from '../fixtures/mockRumData';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const DATA = mockRumData(NOW);
const SESSIONS = DATA.sessions;

const idOf = (row: { [column: string]: unknown } | undefined) => String(row?.['session.id']);

async function useChinese(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem('molesignal-ui-prefs', JSON.stringify({ state: { language: 'zh-cn' }, version: 0 }));
  });
}

test.describe('RUM explorer', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('opens on the sessions of the day, newest first, counted in full by its histogram', async ({ page }) => {
    await page.goto('/rum/explorer');

    const rows = page.locator('[data-rum-row]');
    await expect(rows).toHaveCount(SESSIONS.length);
    expect(await rows.evaluateAll((elements) => elements.map((element) => element.getAttribute('data-rum-id')))).toEqual(
      SESSIONS.map(idOf),
    );

    // The bars count every row the query matches, not the page that was loaded.
    await expect(page.locator('[data-histogram-counting="all"]')).toBeVisible();
    await expect(page.getByRole('tab', { name: /^Sessions/ })).toContainText(String(SESSIONS.length));
    // A session that ran into an error is marked down its left edge, and only that one.
    const withErrors = SESSIONS.filter((row) => Number(row['session.error.count']) > 0).length;
    await expect(rows.filter({ has: page.locator('span.bg-red') })).toHaveCount(withErrors);
  });

  test('a type is a tab: the address says which, and the rows are of that type', async ({ page }) => {
    await page.goto('/rum/explorer');

    const rows = page.locator('[data-rum-row]');
    await expect(rows).toHaveCount(SESSIONS.length);

    await page.getByRole('tab', { name: 'Errors' }).click();
    await expect(page).toHaveURL(/[?&]type=errors/);
    await expect(rows).toHaveCount(DATA.errors.length);
    await expect(rows.first()).toContainText('Cannot read properties');

    // Back to the first type: its address says nothing of a type.
    await page.getByRole('tab', { name: /^Sessions/ }).click();
    await expect(rows).toHaveCount(SESSIONS.length);
    await expect(page).not.toHaveURL(/[?&]type=/);
  });

  test('pages through more rows than one page holds, and back', async ({ page }) => {
    await page.goto('/rum/explorer?type=resources');

    const rows = page.locator('[data-rum-row]');
    await expect(rows).toHaveCount(100);
    const first = await rows.first().getAttribute('data-rum-id');

    await page.getByRole('button', { name: 'Next' }).click();
    await expect(rows).toHaveCount(DATA.resources.length - 100);
    await expect(page.getByRole('button', { name: 'Next' })).toBeDisabled();

    await page.getByRole('button', { name: 'Previous' }).click();
    await expect(rows).toHaveCount(100);
    await expect(rows.first()).toHaveAttribute('data-rum-id', first!);
  });

  test('the address is the query: it can be sent to someone and opens on the same rows', async ({ page }) => {
    const safari = SESSIONS.filter((row) => row['browser.name'] === 'Safari');
    await page.goto(`/rum/explorer?q=${encodeURIComponent("browser.name = 'Safari'")}`);

    const rows = page.locator('[data-rum-row]');
    await expect(rows).toHaveCount(safari.length);
    expect(await rows.evaluateAll((elements) => elements.map((element) => element.getAttribute('data-rum-id')))).toEqual(
      safari.map(idOf),
    );

    // On another type the same address holds, with the words of that type.
    const typeErrors = DATA.errors.filter((row) => row['error.type'] === 'TypeError').length;
    await page.goto(`/rum/explorer?type=errors&q=${encodeURIComponent("error.type = 'TypeError'")}`);
    await expect(rows).toHaveCount(typeErrors);
  });

  test('a value in the field list narrows the query, and the same value takes it off again', async ({ page }) => {
    const safari = SESSIONS.filter((row) => row['browser.name'] === 'Safari');
    await page.goto('/rum/explorer');

    const rows = page.locator('[data-rum-row]');
    await expect(rows).toHaveCount(SESSIONS.length);

    await page.locator('[data-rum-field="browser.name"]').first().getByRole('button', { expanded: false }).first().click();
    const values = page.locator('[data-rum-field-values="browser.name"]');
    // The values are counted over the rows the query matches.
    await expect(values).toContainText(new RegExp(`Safari\\s*${safari.length}`));

    const only = values.getByRole('button', { name: 'Show only rows with browser.name Safari' });
    await only.click();
    await expect(rows).toHaveCount(safari.length);
    await expect(page).toHaveURL(/[?&]q=browser\.name/);
    await expect(only).toHaveAttribute('aria-pressed', 'true');

    await only.click();
    await expect(rows).toHaveCount(SESSIONS.length);
    await expect(page).not.toHaveURL(/[?&]q=/);
  });

  test('a row opens in a drawer; J and K walk the rows, and closing it gives the focus back', async ({ page }) => {
    await page.goto('/rum/explorer');

    const rows = page.locator('[data-rum-row]');
    await rows.first().click();

    const drawer = page.locator('[data-rum-drawer]');
    await expect(drawer).toBeVisible();
    await expect(drawer).toContainText(idOf(SESSIONS[0]));

    await page.keyboard.press('j');
    await expect(drawer).toContainText(idOf(SESSIONS[1]));
    await page.keyboard.press('k');
    await expect(drawer).toContainText(idOf(SESSIONS[0]));

    await page.keyboard.press('Escape');
    await expect(drawer).toBeHidden();
    await expect(rows.first()).toBeFocused();

    // The row of a session opens its page. The explorer writes its range into the address a moment
    // after it opens: a click that leaves before that would be written over by it.
    await page.locator(`[data-rum-id="${MOCK_RUM.session.checkout}"]`).click();
    await expect(page).toHaveURL(/[?&]time=/);
    await page.getByTestId('rum-drawer-session').click();
    await expect(page).toHaveURL(new RegExp(`/rum/sessions/view/${MOCK_RUM.session.checkout}`));
  });

  test('a query that matches nothing offers the way out, and a read that fails offers another go', async ({ page }) => {
    await page.goto(`/rum/explorer?q=${encodeURIComponent("usr.id = 'nobody'")}`);

    const none = page.getByTestId('rum-table-no-data');
    await expect(none).toContainText('No rows in this range');
    await none.getByRole('button', { name: 'Clear the query' }).click();
    await expect(page.locator('[data-rum-row]')).toHaveCount(SESSIONS.length);
    await expect(page).not.toHaveURL(/[?&]q=/);

    await page.route(/\/api\/v1\/rum\/search(?:\?.*)?$/, (route) =>
      route.fulfill({ status: 500, json: { error: 'internal', message: 'the engine is busy' } }),
    );
    await page.getByRole('tab', { name: 'Errors' }).click();
    await expect(page.getByTestId('rum-table-no-data')).toContainText('The query failed');
  });
});

test.describe('RUM in Chinese', () => {
  test('every page stands under the name of the module, with the time range in the language of the product', async ({
    page,
    mockServer,
  }) => {
    await useChinese(page);
    await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });

    // The tabs say which page is open; the title is the module's wherever the page has a title row.
    for (const path of ['/rum/applications', '/rum/settings/sdk']) {
      await page.goto(path);
      await expect(page.getByRole('heading', { level: 1 })).toHaveText('用户体验');
    }

    // The explorer gives its title row to the data: its name stands in the bar.
    await page.goto('/rum/explorer');
    await expect(page.getByRole('heading', { level: 1 })).toHaveText('探索');
    await expect(page.locator('[data-rum-row]')).toHaveCount(SESSIONS.length);
    // "Last 24 hours" stood here, in English on a page written in Chinese.
    await expect(page.locator('[data-time-range-trigger="true"]')).toHaveText('近 24 小时');
    await expect(page.getByRole('tab', { name: /^会话\s*10/ })).toBeVisible();
  });
});

test.describe('RUM session', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('says how it went, what it did in the order it happened, and which backend traces it made', async ({ page }) => {
    await page.goto(`/rum/sessions/view/${MOCK_RUM.session.checkout}`);

    await expect(page.getByRole('heading', { level: 1 })).toHaveText(/Session with errors · \/checkout/);
    await expect(page.getByText('Recorded replay').first()).toBeVisible();

    // Who it was of, where from.
    await expect(page.getByText('Mei Lin').first()).toBeVisible();
    await expect(page.getByText('China · Shanghai')).toBeVisible();

    // Everything it did: one row for each view, action, request, error, long task and log it left.
    const mine = (type: 'views' | 'actions' | 'errors' | 'resources' | 'long_tasks' | 'logs') =>
      DATA[type].filter((row) => row['session.id'] === MOCK_RUM.session.checkout).length;
    const timeline = page.locator('[data-rum-timeline-event]');
    await expect(timeline).toHaveCount(
      mine('views') + mine('actions') + mine('errors') + mine('resources') + mine('long_tasks') + mine('logs'),
    );
    await expect(page.locator('[data-rum-timeline-event="error"]')).toHaveCount(1);

    // The request that failed comes before the error it raised.
    const labels = await timeline.locator('span.truncate.font-strong').allTextContents();
    const failed = labels.findIndex((label) => label.includes('POST · https://api.example.com/api/orders'));
    const kinds = await timeline.evaluateAll((rows) => rows.map((row) => row.getAttribute('data-rum-timeline-event')));
    const raised = kinds.indexOf('error');
    expect(failed).toBeGreaterThan(-1);
    expect(raised).toBeGreaterThan(failed);

    // The trace its failed request carried.
    await expect(page.getByText('Direct', { exact: true })).toBeVisible();
    await expect(page.getByText('Time-correlated', { exact: true })).toHaveCount(0);
    await expect(page.getByText('checkout-api').first()).toBeVisible();
  });

  test('a session whose requests carried no trace lists the traces that ran while it did', async ({ page }) => {
    await page.goto(`/rum/sessions/view/${MOCK_RUM.session.frustrated}`);

    await expect(page.getByText('Time-correlated', { exact: true })).toHaveCount(2);
    await expect(page.getByText('Direct', { exact: true })).toHaveCount(0);
  });

  test('a recorded visit has a player, and one nobody recorded has its actions in its place', async ({ page }) => {
    await page.goto(`/rum/sessions/view/${MOCK_RUM.session.checkout}`);
    await expect(page.getByRole('button', { name: 'Play', exact: true })).toBeVisible();
    // The recording is as long as the visit: twelve minutes.
    await expect(page.getByText('00:00 / 12:00')).toBeVisible();

    await page.goto(`/rum/sessions/view/${MOCK_RUM.session.anonymous}`);
    await expect(page.getByText('No DOM recording is available for this session')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Play', exact: true })).toHaveCount(0);
  });

  test('a session that was not found says so', async ({ page }) => {
    await page.goto('/rum/sessions/view/not-a-session');

    await expect(page.getByText('session not found')).toBeVisible();
    await expect(page.locator('[data-rum-timeline-event]')).toHaveCount(0);
  });
});

test.describe('RUM applications', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  /** The row of the application that shows this identifier. */
  const rowOf = (page: Page, id: string) =>
    page.getByRole('row').filter({ has: page.getByText(id, { exact: true }) });

  test('lists what sends events and what each one keeps; its token stays hidden until asked for', async ({ page }) => {
    await page.goto('/rum/applications');

    await expect(rowOf(page, 'storefront-app')).toContainText('Flutter');
    await expect(page.getByText('2 applications')).toBeVisible();

    await rowOf(page, 'storefront-app').click();
    const drawer = page.getByRole('dialog');
    await expect(drawer).toContainText('storefront-app');

    // Write-only and bound to the application: it is read from the server, and shown masked first.
    await expect(drawer.getByText(/^mspat-8f•+/)).toBeVisible();
    await drawer.getByRole('button', { name: 'Show the token' }).click();
    await expect(drawer.getByText(/^mspat-8f3a2c9d41e6b075a9d2c4e8f1b30a76/)).toBeVisible();
    await expect(drawer.getByRole('link', { name: 'Open the setup guide' })).toHaveAttribute(
      'href',
      '/datasource/recommended/rum-flutter?app=storefront-app',
    );
  });

  test('makes an application from a name, and opens it for its token', async ({ page }) => {
    await page.goto('/rum/applications');
    await page.getByRole('button', { name: 'Create application' }).first().click();

    const name = page.getByPlaceholder('Checkout web');
    // The identifier is made from the name unless one is typed.
    await name.fill('Checkout web');
    await expect(page.getByText('Leave blank to use checkout-web.')).toBeVisible();

    const created = page.waitForRequest((request) => request.method() === 'POST' && request.url().endsWith('/rum/applications'));
    await page.getByRole('button', { name: 'Create', exact: true }).click();
    expect(((await created).postDataJSON() as { name: string }).name).toBe('Checkout web');

    // The new application is open, and listed.
    const drawer = page.getByRole('dialog');
    await expect(drawer).toContainText('checkout-web');
    await expect(drawer.getByRole('button', { name: 'Show the token' })).toBeVisible();
    // The drawer has its cross in the head and a button in the foot: either closes it.
    await drawer.getByRole('button', { name: 'Close', exact: true }).last().click();
    await expect(page.getByText('3 applications')).toBeVisible();
  });

  test('refuses an identifier an SDK could not send, before asking the server', async ({ page }) => {
    await page.goto('/rum/applications');
    await page.getByRole('button', { name: 'Create application' }).first().click();

    await page.getByPlaceholder('Checkout web').fill('Checkout web');
    await page.getByPlaceholder('checkout-web').fill('not an id!');

    await expect(page.getByText(/^Use letters, digits and/)).toBeVisible();
    await expect(page.getByRole('button', { name: 'Create', exact: true })).toBeDisabled();
  });

  test('says what is taken out of the text of an event, all of it until it is turned off, and what share of sessions is kept', async ({ page }) => {
    await page.goto('/rum/applications');
    await rowOf(page, 'storefront').click();
    const drawer = page.getByRole('dialog');

    const kinds = ['Tokens, keys, and passwords', 'Email addresses', 'Phone numbers', 'Payment card numbers', 'Identity numbers', 'Sensitive URL parameters'];

    for (const kind of kinds) await expect(drawer.getByRole('switch', { name: kind })).toBeChecked();

    const share = drawer.getByRole('spinbutton', { name: 'Share of sessions kept (%)' });

    await expect(share).toHaveValue('100');

    const saved = page.waitForRequest((request) => request.method() === 'PATCH' && request.url().endsWith('/rum/applications/storefront'));

    await drawer.getByRole('switch', { name: 'Phone numbers' }).click();
    await drawer.getByRole('switch', { name: 'Email addresses' }).click();
    await share.fill('25');
    await drawer.getByRole('button', { name: 'Save', exact: true }).click();

    const body = (await saved).postDataJSON() as { settings: { scrub: string[]; sessionSampleRate: number } };

    // The kinds that stay on are sent in the order they are shown.
    expect(body.settings.scrub).toEqual(['secrets', 'cards', 'national_ids', 'url_params']);
    expect(body.settings.sessionSampleRate).toBe(25);

    // What was saved is what the drawer shows when it is opened again.
    await drawer.getByRole('button', { name: 'Close', exact: true }).last().click();
    await rowOf(page, 'storefront').click();
    await expect(page.getByRole('dialog').getByRole('switch', { name: 'Phone numbers' })).not.toBeChecked();
    await expect(page.getByRole('dialog').getByRole('switch', { name: 'Tokens, keys, and passwords' })).toBeChecked();
    await expect(page.getByRole('dialog').getByRole('spinbutton', { name: 'Share of sessions kept (%)' })).toHaveValue('25');
  });

  test('does not save a share of sessions that is more than all of them', async ({ page }) => {
    await page.goto('/rum/applications');
    await rowOf(page, 'storefront').click();
    const drawer = page.getByRole('dialog');

    await drawer.getByRole('spinbutton', { name: 'Share of sessions kept (%)' }).fill('150');

    await expect(drawer.getByRole('button', { name: 'Save', exact: true })).toBeDisabled();

    await drawer.getByRole('spinbutton', { name: 'Share of sessions kept (%)' }).fill('0');

    await expect(drawer.getByRole('button', { name: 'Save', exact: true })).toBeEnabled();
  });

  test('saves what is kept with each event, and asks before it deletes', async ({ page }) => {
    await page.goto('/rum/applications');
    await rowOf(page, 'storefront').click();
    const drawer = page.getByRole('dialog');

    const saved = page.waitForRequest((request) => request.method() === 'PATCH' && request.url().endsWith('/rum/applications/storefront'));
    await drawer.getByRole('switch', { name: 'Keep the IP address' }).click();
    await drawer.getByRole('button', { name: 'Save', exact: true }).click();
    expect(((await saved).postDataJSON() as { settings: { collectIp: boolean } }).settings.collectIp).toBe(false);

    await drawer.getByRole('button', { name: 'Delete application' }).click();
    const confirm = page.getByRole('dialog', { name: 'Delete Storefront?' });
    await expect(confirm).toBeVisible();
    await confirm.getByRole('button', { name: 'Delete application' }).click();

    await expect(page.getByText('1 application', { exact: true })).toBeVisible();
    await expect(rowOf(page, 'storefront')).toHaveCount(0);
  });
});

test.describe('RUM setup guide', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('makes the reader choose the application, then shows its token and snippet and checks what arrived', async ({
    page,
  }) => {
    await page.goto('/datasource/recommended/rum');

    // Two applications: none is taken for the reader, and no token is shown before one is chosen.
    const connection = page.locator('[data-guide-connection]');
    const choice = connection.getByRole('combobox', { name: 'RUM application' });
    await expect(choice).toBeEnabled();
    await expect(connection.getByText('Choose a RUM application first: its client token is shown once it is chosen.')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Verify connection' })).toBeDisabled();

    await choice.click();
    await page.getByRole('option', { name: 'Storefront · storefront', exact: true }).click();

    // Its token, which may only write RUM data, the one entry events go to, and the snippet that
    // carries the application.
    await expect(connection.getByText('Write RUM data', { exact: true })).toBeVisible();
    await expect(connection).toContainText('/api/v1/rum/events');
    await expect(page.locator('[data-datasource-guide]')).toContainText("applicationId: 'storefront'");

    // Nothing has arrived yet: the check says so, and offers a test visit instead.
    const search = /\/api\/v1\/rum\/search(?:\?.*)?$/;
    const rows = (items: unknown[]) => ({ items, hasMore: false, nextCursor: null, previousCursor: null });
    await page.route(search, (route) => route.fulfill({ json: rows([]) }));
    await page.getByRole('button', { name: 'Verify connection' }).click();
    await expect(page.locator('[data-verification="warning"]')).toBeVisible();

    // The test visit is accepted: its view and its error.
    await page.getByRole('button', { name: 'Send test event' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Sent' })).toContainText('2 accepted · 0 rejected');

    // Then a row is there, and the check finds it.
    await page.route(search, (route) => route.fulfill({ json: rows([{ 'view.id': 'v', timestamp: NOW - 5_000_000 }]) }));
    await page.getByRole('button', { name: 'Check again' }).click();
    await expect(page.locator('[data-verification="success"]')).toBeVisible();
  });

  test('opens on the application whose page sent the reader here', async ({ page }) => {
    await page.goto('/datasource/recommended/rum-flutter?app=storefront-app');

    await expect(page.getByRole('combobox', { name: 'RUM application' })).toContainText('Storefront app · storefront-app');
    await expect(page.locator('[data-datasource-guide]')).toContainText("applicationId: 'storefront-app'");
  });

  test('sends a reader without an application to make one', async ({ page }) => {
    await page.route(/\/api\/v1\/rum\/applications(?:\?.*)?$/, (route) => route.fulfill({ json: [] }));
    await page.goto('/datasource/recommended/rum');

    await page.getByRole('link', { name: 'Create an application' }).click();
    await expect(page).toHaveURL(/\/rum\/applications$/);
    await expect(page.getByPlaceholder('Checkout web')).toBeVisible();
  });
});
