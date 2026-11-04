import type { Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM, mockRumData } from '../fixtures/mockRumData';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const DATA = mockRumData(NOW);

const CHECKOUT = MOCK_RUM.session.checkout;
const FRUSTRATED = MOCK_RUM.session.frustrated;
const ANONYMOUS = MOCK_RUM.session.anonymous;
/** A session the seeded playlist holds that the data no longer has. */
const GONE = 'ffffffff-0000-4000-8000-000000000000';

const startOf = (id: string) => DATA.sessions.find((row) => row['session.id'] === id)!.timestamp;

const sessionUrl = (id: string) => new RegExp(`/rum/sessions/view/${id}\\?`);

async function useChinese(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem('molesignal-ui-prefs', JSON.stringify({ state: { language: 'zh-cn' }, version: 0 }));
  });
}

const playlistOption = (page: Page, id: string) => page.locator(`[data-rum-playlist-option="${id}"]`);

test.describe('RUM playlists', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('is a tab of its own after the explorer, and lists the playlists with their application and the sessions they hold', async ({ page }) => {
    await page.goto('/rum/playlists');

    const tabs = await page.locator('[data-rum-navigation="surface"] a').allInnerTexts();

    expect(tabs.map((tab) => tab.trim()).slice(0, 4)).toEqual(['Overview', 'Explorer', 'Playlists', 'Errors']);
    await expect(page.locator('[data-rum-navigation="surface"] [aria-current="page"]').first()).toContainText('Playlists');

    // The one changed last first.
    await expect(page.locator('[data-rum-playlist-link]')).toHaveText(['Checkout failures', 'Slow product pages']);

    const first = page.getByRole('row', { name: /Checkout failures/ });

    await expect(first).toContainText('Sessions that ended at the checkout with an error.');
    await expect(first).toContainText('Storefront');
    await expect(first.locator('[data-rum-playlist-sessions]')).toHaveText('3');
    await expect(page.getByRole('row', { name: /Slow product pages/ }).locator('[data-rum-playlist-sessions]')).toHaveText('0');
    await expect(page.getByText('2 playlists')).toBeVisible();
  });

  test('narrows to the playlists of one application', async ({ page }) => {
    await page.goto('/rum/playlists');
    await page.getByRole('combobox', { name: 'Application' }).click();
    await page.getByRole('option', { name: 'Storefront app' }).click();

    await expect(page).toHaveURL(/[?&]app=storefront-app/);
    await expect(page.locator('[data-rum-playlist-link]')).toHaveCount(0);
    await expect(page.getByText('No playlists yet')).toBeVisible();
  });

  test('opens a playlist with its sessions in the order they were put in, and says which one is gone from the data', async ({ page }) => {
    await page.goto('/rum/playlists/pl-1');

    await expect(page.getByRole('heading', { name: 'Checkout failures' }).first()).toBeVisible();

    const checkout = DATA.sessions.find((row) => row['session.id'] === CHECKOUT)!;
    const rows = page.locator('[data-rum-playlist] tbody tr');

    await expect(rows).toHaveCount(3);
    await expect(rows.nth(0)).toContainText(CHECKOUT);
    await expect(rows.nth(0)).toContainText(String(checkout['session.exit_view.url_path'] ?? checkout['session.landing_view.url_path']));
    // The moment it opens at, from the beginning of the session.
    await expect(rows.nth(0)).toContainText('00:05');
    await expect(rows.nth(1)).toContainText(FRUSTRATED);
    await expect(rows.nth(2)).toContainText(GONE);
    await expect(rows.nth(2)).toContainText('Not found in the data');
  });

  test('plays a playlist one session after the other, and says where each stands', async ({ page }) => {
    await page.goto('/rum/playlists/pl-1');
    await page.locator('[data-rum-playlist-play]').click();

    await expect(page).toHaveURL(sessionUrl(CHECKOUT));
    // The moment it was put in at, and the playlist, are in the address.
    expect(new URL(page.url()).searchParams.get('at')).toBe(String(startOf(CHECKOUT) + 5_000_000));
    expect(new URL(page.url()).searchParams.get('playlist')).toBe('pl-1');

    const bar = page.locator('[data-rum-playlist-bar="pl-1"]');

    await expect(bar).toContainText('Checkout failures');
    await expect(bar.locator('[data-rum-playlist-place]')).toHaveText('1 of 3');
    await expect(bar.locator('[data-rum-playlist-previous]')).toHaveCount(0);

    // The page writes its time range into the address a moment after it opens: a click made before
    // that would be written over by it.
    await expect(page).toHaveURL(/[?&]time=/);
    await bar.locator('[data-rum-playlist-next]').click();
    await expect(page).toHaveURL(sessionUrl(FRUSTRATED));
    await expect(page.locator('[data-rum-playlist-place]')).toHaveText('2 of 3');

    // A session the data no longer has does not end the walk: the bar stays and leads back.
    await expect(page).toHaveURL(/[?&]time=/);
    await page.locator('[data-rum-playlist-next]').click();
    await expect(page).toHaveURL(sessionUrl(GONE));
    await expect(page.locator('[data-rum-playlist-place]')).toHaveText('3 of 3');
    await expect(page.locator('[data-rum-playlist-next]')).toHaveCount(0);

    await expect(page).toHaveURL(/[?&]time=/);
    await page.locator('[data-rum-playlist-previous]').click();
    await expect(page).toHaveURL(sessionUrl(FRUSTRATED));
  });

  test('puts a session in a playlist from its page, and takes it out again', async ({ page }) => {
    await page.goto(`/rum/sessions/view/${ANONYMOUS}`);
    await page.locator('[data-rum-playlist-menu]').click();

    // The playlists of the application of the session, with how many sessions each holds.
    await expect(playlistOption(page, 'pl-1')).toContainText('Checkout failures');
    await expect(playlistOption(page, 'pl-1')).toContainText('3');
    await expect(playlistOption(page, 'pl-2')).toHaveAttribute('aria-checked', 'false');

    const added = page.waitForRequest((request) => request.method() === 'POST' && request.url().endsWith('/rum/playlists/pl-2/sessions'));

    await playlistOption(page, 'pl-2').click();

    const body = (await added).postDataJSON() as { sessions: Array<{ sessionId: string; atTime?: number }> };

    expect(body.sessions).toEqual([{ sessionId: ANONYMOUS }]);
    await expect(playlistOption(page, 'pl-2')).toHaveAttribute('aria-checked', 'true');
    await expect(playlistOption(page, 'pl-2')).toContainText('1');

    const removed = page.waitForRequest((request) => request.method() === 'DELETE' && request.url().endsWith(`/rum/playlists/pl-2/sessions/${ANONYMOUS}`));

    await playlistOption(page, 'pl-2').click();
    await removed;
    await expect(playlistOption(page, 'pl-2')).toHaveAttribute('aria-checked', 'false');
    await expect(playlistOption(page, 'pl-2')).toContainText('0');
  });

  test('puts a session in at the moment its replay is at, when asked', async ({ page }) => {
    await page.goto(`/rum/sessions/view/${CHECKOUT}`);
    await page.locator('[data-rum-playlist-menu]').click();

    const moment = page.locator('[data-rum-playlist-moment]');

    // The replay has to have been read for there to be a moment.
    await expect(moment).toBeVisible();
    await moment.check();

    const added = page.waitForRequest((request) => request.method() === 'POST' && request.url().endsWith('/rum/playlists/pl-2/sessions'));

    await playlistOption(page, 'pl-2').click();

    const body = (await added).postDataJSON() as { sessions: Array<{ sessionId: string; atTime?: number }> };

    expect(body.sessions[0]?.sessionId).toBe(CHECKOUT);
    expect(body.sessions[0]?.atTime).toBeGreaterThanOrEqual(startOf(CHECKOUT) - 60_000_000);
    expect(body.sessions[0]?.atTime).toBeLessThanOrEqual(startOf(CHECKOUT) + 12 * 60_000_000);
  });

  test('makes a playlist from the page of a session, with the session in it', async ({ page }) => {
    await page.goto(`/rum/sessions/view/${ANONYMOUS}`);
    await page.locator('[data-rum-playlist-menu]').click();

    const created = page.waitForRequest((request) => request.method() === 'POST' && request.url().endsWith('/rum/playlists'));

    await page.getByRole('textbox', { name: 'Name of the new playlist' }).fill('Blog readers');
    await page.locator('[data-rum-playlist-new]').click();

    const body = (await created).postDataJSON() as { application: string; name: string; sessions: Array<{ sessionId: string }> };

    expect(body.application).toBe(MOCK_RUM.applications.web);
    expect(body.name).toBe('Blog readers');
    expect(body.sessions).toEqual([{ sessionId: ANONYMOUS }]);

    await expect(playlistOption(page, 'pl-3')).toContainText('Blog readers');
    await expect(playlistOption(page, 'pl-3')).toHaveAttribute('aria-checked', 'true');
  });

  test('copies the address of a session at the moment the replay is at', async ({ page }) => {
    await page.context().grantPermissions(['clipboard-read', 'clipboard-write']);
    await page.goto(`/rum/sessions/view/${CHECKOUT}`);

    const button = page.locator('[data-rum-copy-link]');

    // Once the replay has been read the button says it copies a moment.
    await expect(button).toContainText('Copy link to this moment');
    await button.click();
    await expect(button).toContainText('Link copied');

    const copied = new URL(await page.evaluate(() => navigator.clipboard.readText()));

    expect(copied.pathname).toBe(`/rum/sessions/view/${CHECKOUT}`);
    expect(Number(copied.searchParams.get('at'))).toBeGreaterThan(startOf(CHECKOUT) - 60_000_000);
    expect(Number(copied.searchParams.get('at'))).toBeLessThan(startOf(CHECKOUT) + 12 * 60_000_000);
    // The stretch the session ran in, so that an old session is found by the link.
    expect(Date.parse(copied.searchParams.get('from') ?? '')).toBeLessThanOrEqual(startOf(CHECKOUT) / 1_000);
    expect(Date.parse(copied.searchParams.get('to') ?? '')).toBeGreaterThan(startOf(CHECKOUT) / 1_000);
    expect(copied.searchParams.has('playlist')).toBe(false);
  });

  test('makes, renames and deletes a playlist', async ({ page }) => {
    await page.goto('/rum/playlists');
    await page.locator('[data-rum-playlist-create]').first().click();

    const drawer = page.getByRole('dialog');

    await drawer.getByRole('textbox', { name: 'Name' }).fill('Mobile crashes');
    await drawer.getByRole('textbox', { name: 'Description' }).fill('Sessions of the app that ended in a crash.');
    await drawer.getByRole('button', { name: 'Create', exact: true }).click();

    await expect(page).toHaveURL(/\/rum\/playlists\/pl-3$/);
    await expect(page.getByRole('heading', { name: 'Mobile crashes' }).first()).toBeVisible();
    await expect(page.getByText('No session in this playlist.')).toBeVisible();

    await page.getByRole('button', { name: 'Rename' }).click();

    const rename = page.getByRole('dialog');

    await expect(rename.getByRole('combobox', { name: 'Application' })).toBeDisabled();
    await rename.getByRole('textbox', { name: 'Name' }).fill('App crashes');

    const patched = page.waitForRequest((request) => request.method() === 'PATCH' && request.url().endsWith('/rum/playlists/pl-3'));

    await rename.getByRole('button', { name: 'Save', exact: true }).click();
    expect(((await patched).postDataJSON() as { name: string }).name).toBe('App crashes');
    await expect(page.getByRole('heading', { name: 'App crashes' }).first()).toBeVisible();

    await page.getByRole('button', { name: 'Delete playlist' }).click();
    await page.getByRole('dialog', { name: 'Delete App crashes?' }).getByRole('button', { name: 'Delete playlist' }).click();

    await expect(page).toHaveURL(/\/rum\/playlists$/);
    await expect(page.locator('[data-rum-playlist-link]')).toHaveText(['Checkout failures', 'Slow product pages']);
  });

  test('refuses a name the application already has, and says so', async ({ page }) => {
    await page.goto('/rum/playlists');
    await page.locator('[data-rum-playlist-create]').first().click();

    const drawer = page.getByRole('dialog');

    await drawer.getByRole('textbox', { name: 'Name' }).fill('checkout failures');
    await drawer.getByRole('button', { name: 'Create', exact: true }).click();

    await expect(drawer.getByRole('alert')).toContainText('already has this name');
    await expect(page).toHaveURL(/\/rum\/playlists$/);
  });

  test('takes a session out of a playlist from the playlist', async ({ page }) => {
    await page.goto('/rum/playlists/pl-1');

    const removed = page.waitForRequest((request) => request.method() === 'DELETE' && request.url().endsWith(`/rum/playlists/pl-1/sessions/${FRUSTRATED}`));

    await page.locator(`[data-rum-playlist-remove="${FRUSTRATED}"]`).click();
    await removed;

    await expect(page.locator('[data-rum-playlist] tbody tr')).toHaveCount(2);
    await expect(page.locator(`[data-rum-playlist-session="${FRUSTRATED}"]`)).toHaveCount(0);
  });

  test('says that a playlist was not found', async ({ page }) => {
    await page.goto('/rum/playlists/pl-404');

    await expect(page.getByText('This playlist was not found')).toBeVisible();
  });
});

test.describe('RUM playlists in Chinese', () => {
  test('is written in the language of the product', async ({ page, mockServer }) => {
    await useChinese(page);
    await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });
    await page.goto('/rum/playlists');

    await expect(page.locator('[data-rum-navigation="surface"] [aria-current="page"]').first()).toContainText('播放列表');
    await expect(page.getByText('2 个播放列表')).toBeVisible();

    await page.goto('/rum/playlists/pl-1');
    await expect(page.locator('[data-rum-playlist] tbody tr').nth(2)).toContainText('数据里找不到');
    await page.locator('[data-rum-playlist-play]').click();
    await expect(page.locator('[data-rum-playlist-place]')).toHaveText('第 1 个，共 3 个');
  });
});
