import type { Page } from '@playwright/test';

import type { RumRow } from '../../src/api/rum';
import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM, mockRumData } from '../fixtures/mockRumData';
import { percent } from '../fixtures/rumFigures';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const DATA = mockRumData(NOW);

const WEB = MOCK_RUM.applications.web;
const APP = MOCK_RUM.applications.app;

const frustrated = (application: string) => DATA.actions.filter((row) => row['application.id'] === application && Number(row['action.frustration.count']) > 0);

const WEB_ACTIONS = frustrated(WEB);
const APP_ACTIONS = frustrated(APP);

const words = (row: RumRow): string[] => (row['action.frustration.type'] as string[] | undefined) ?? [];
const showsKind = (prefix: string) => (row: RumRow) => words(row).some((word) => word.startsWith(prefix));
const sessionsOf = (rows: readonly RumRow[]) => new Set(rows.map((row) => row['session.id'])).size;

const text = (row: RumRow, field: string): string | null => (typeof row[field] === 'string' && row[field] !== '' ? (row[field] as string) : null);

/** The groups of frustrated actions by the values of their columns: the most sessions first, ties by the values, a missing value last. */
function groupsOf(rows: readonly RumRow[], fields: readonly string[]) {
  const groups = new Map<string, { values: Array<string | null>; rows: RumRow[] }>();

  for (const row of rows) {
    const values = fields.map((field) => text(row, field));
    const key = JSON.stringify(values);
    const entry = groups.get(key) ?? { values, rows: [] };

    entry.rows.push(row);
    groups.set(key, entry);
  }

  const compare = (a: string | null, b: string | null) => (a === b ? 0 : a === null ? 1 : b === null ? -1 : a.localeCompare(b));

  return [...groups.values()]
    .map((group) => ({ ...group, sessions: sessionsOf(group.rows) }))
    .sort((a, b) => b.sessions - a.sessions || compare(a.values[0] ?? null, b.values[0] ?? null) || compare(a.values[1] ?? null, b.values[1] ?? null));
}

const ELEMENT = ['action.target.name', 'action.target.selector'] as const;

const counted = (n: number, one: string, other: string) => `${n} ${n === 1 ? one : other}`;

/** The words of a count of one sign as the statement writes them. */
const sign = (rows: readonly RumRow[], prefix: string, one: string, other: string) => {
  const of = rows.filter(showsKind(prefix));

  return `${counted(of.length, one, other)} (${counted(sessionsOf(of), 'session', 'sessions')})`;
};

/** A moment of a session as the player's progress reads it: minutes and seconds, each of two digits. */
const mmss = (milliseconds: number) => {
  const seconds = Math.floor(milliseconds / 1_000);

  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`;
};

const rows = (page: Page) => page.locator('[data-rum-frustration-table] [data-rum-row]');

const rowNamed = (page: Page, name: string) => rows(page).filter({ hasText: name });

const count = (row: ReturnType<typeof rowNamed>, kind: string) => row.locator(`[data-rum-count="${kind}"]`);

const panel = (page: Page) => page.locator('[data-rum-panel="frustration"]');

async function open(page: Page, path = '/rum/frustration') {
  await page.goto(path);
  await expect(page).toHaveURL(/[?&]time=/);
}

test.describe('RUM frustration', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('is a tab of its own after the performance tab', async ({ page }) => {
    await open(page);

    const tabs = await page.locator('[data-rum-navigation="surface"] a').allInnerTexts();

    expect(tabs.map((tab) => tab.trim()).slice(0, 9)).toEqual(['Overview', 'Explorer', 'Playlists', 'Errors', 'Performance', 'Frustration', 'Analytics', 'Feature flags', 'Applications']);
    await expect(page.locator('[data-rum-navigation="surface"] [aria-current="page"]').first()).toContainText('Frustration');
  });

  test('says what share of the sessions showed a sign, and how many actions showed each sign in how many sessions', async ({ page }) => {
    await open(page);

    const sessions = DATA.sessions.filter((row) => row['application.id'] === WEB);
    const affected = sessions.filter((row) => Number(row['session.frustration.count']) > 0).length;

    expect(WEB_ACTIONS.length).toBeGreaterThan(5);
    await expect(page.locator('[data-rum-statement]')).toHaveText(
      `${percent(affected / sessions.length)} of the sessions showed signs of frustration: ` +
        `${sign(WEB_ACTIONS, 'rage_', 'rage click', 'rage clicks')}, ${sign(WEB_ACTIONS, 'dead_', 'dead click', 'dead clicks')}, ${sign(WEB_ACTIONS, 'error_', 'error click', 'error clicks')}.`,
    );
    // Each sign is told in its own colour, in the order rage, dead, error.
    await expect(page.locator('[data-rum-statement] [data-rum-kind]')).toHaveText(['rage clicks', 'dead clicks', 'error click']);
  });

  test('lists the elements by the sessions they touched, each with its selector and its count of every sign', async ({ page }) => {
    await open(page);

    const groups = groupsOf(WEB_ACTIONS, ELEMENT);

    expect(groups.length).toBeGreaterThan(4);
    await expect(rows(page)).toHaveCount(groups.length);
    await expect(page.locator('[data-rum-table-count]')).toContainText(`${groups.length} groups`);

    for (const [index, group] of groups.entries()) {
      const cells = rows(page).nth(index).getByRole('cell');
      const dash = (n: number) => (n === 0 ? '—' : String(n));

      await expect(cells.nth(0)).toHaveText(group.values[0] ?? '(no name)');
      await expect(cells.nth(1)).toHaveText(group.values[1] ?? '—');
      await expect(cells.nth(2)).toHaveText(dash(group.rows.filter(showsKind('rage_')).length));
      await expect(cells.nth(3)).toHaveText(dash(group.rows.filter(showsKind('dead_')).length));
      await expect(cells.nth(4)).toHaveText(dash(group.rows.filter(showsKind('error_')).length));
      await expect(cells.nth(5)).toHaveText(String(group.sessions));
    }

    await expect(page.locator('[data-rum-column="sessions"]')).toHaveAttribute('aria-sort', 'descending');
  });

  test('tells an element that has no text by its selector, and says that it has no name', async ({ page }) => {
    await open(page);

    const banner = rows(page).filter({ hasText: 'div.hero-banner' });

    await expect(banner).toHaveCount(1);
    await expect(banner).toHaveAttribute('data-rum-unnamed', '');
    await expect(banner.getByRole('cell').nth(0)).toHaveText('(no name)');
    await expect(count(banner, 'rage')).toHaveText(String(WEB_ACTIONS.filter((row) => text(row, 'action.target.selector') === 'div.hero-banner').length));
    await expect(count(banner, 'dead')).toHaveText('—');
  });

  test('orders by the column clicked, and the other way when it is clicked again', async ({ page }) => {
    await open(page);

    await page.locator('[data-rum-column="rage"] button').click();
    await expect(page).toHaveURL(/[?&]sort=rage/);
    await expect(page.locator('[data-rum-column="rage"]')).toHaveAttribute('aria-sort', 'descending');

    const rage = await rows(page).evaluateAll((elements) => elements.map((element) => Number(element.querySelector('[data-rum-count="rage"]')?.textContent?.replace('—', '0'))));

    expect([...rage].sort((a, b) => b - a)).toEqual(rage);

    await page.locator('[data-rum-column="rage"] button').click();
    await expect(page).toHaveURL(/[?&]dir=asc/);
    await expect(page.locator('[data-rum-column="rage"]')).toHaveAttribute('aria-sort', 'ascending');
  });

  test('narrows the table to one sign, to the words of an element or a page, and the address says which', async ({ page }) => {
    await open(page);

    await page.locator('[data-rum-kind-filter]').click();
    await page.getByRole('option', { name: 'Dead clicks' }).click();
    await expect(page).toHaveURL(/[?&]kind=dead/);

    const dead = WEB_ACTIONS.filter(showsKind('dead_'));

    await expect(rows(page)).toHaveCount(groupsOf(dead, ELEMENT).length);
    // An action that showed two signs counts for both of them.
    const both = rowNamed(page, 'Open order');

    await expect(count(both, 'rage')).toHaveText('1');
    await expect(count(both, 'dead')).toHaveText('1');

    await page.locator('[data-rum-kind-filter]').click();
    await page.getByRole('option', { name: 'All', exact: true }).click();
    await expect(page).not.toHaveURL(/[?&]kind=/);

    await page.getByPlaceholder('Search elements or pages').fill('pay');
    await expect(page).toHaveURL(/[?&]q=pay/);
    await expect(rows(page)).toHaveCount(1);
    await expect(rows(page).first()).toContainText('Pay now');

    await page.getByPlaceholder('Search elements or pages').fill('/cart');
    await expect(rows(page)).toHaveCount(groupsOf(WEB_ACTIONS.filter((row) => text(row, 'view.url_path')?.includes('/cart')), ELEMENT).length);

    await page.getByPlaceholder('Search elements or pages').fill('nothing like this');
    await expect(page.locator('[data-rum-no-match]')).toHaveText('No group matches');
  });

  test('tells the actions apart by a page, a version or a browser', async ({ page }) => {
    await open(page);

    await page.locator('[data-rum-group]').click();
    await page.getByRole('option', { name: 'Page' }).click();
    await expect(page).toHaveURL(/[?&]by=page/);

    const pages = groupsOf(WEB_ACTIONS, ['view.url_path_group']);

    await expect(rows(page)).toHaveCount(pages.length);
    await expect(page.locator('[data-rum-column="sessions"]')).toBeVisible();
    await expect(rows(page).first().getByRole('cell').first()).toHaveText(pages[0]!.values[0] as string);
    await expect(rows(page).first().getByRole('cell').last()).toHaveText(String(pages[0]!.sessions));

    await page.locator('[data-rum-group]').click();
    await page.getByRole('option', { name: 'Version' }).click();
    await expect(rows(page)).toHaveCount(groupsOf(WEB_ACTIONS, ['version']).length);

    await page.locator('[data-rum-group]').click();
    await page.getByRole('option', { name: 'Browser' }).click();
    await expect(page).toHaveURL(/[?&]by=browser/);
    await expect(rows(page)).toHaveCount(groupsOf(WEB_ACTIONS, ['browser.name']).length);
    await expect(page.locator('[data-rum-table-count]')).toBeVisible();
  });

  test('opens what the actions of an element are made of, and the address says which element', async ({ page }) => {
    await open(page);

    const pay = WEB_ACTIONS.filter((row) => text(row, 'action.target.name') === 'Pay now');
    const selector = text(pay[0]!, 'action.target.selector')!;

    await rowNamed(page, 'Pay now').getByRole('button').click();
    await expect(page).toHaveURL(/[?&]open=/);
    await expect(rowNamed(page, 'Pay now')).toHaveClass(/floating-item-selected/);

    await expect(panel(page)).toContainText('Pay now');
    await expect(panel(page)).toContainText(selector);
    await expect(panel(page).locator('[data-rum-panel-sentence]')).toHaveText(`${counted(sessionsOf(pay), 'session', 'sessions')} met signs of frustration on this element. Most happened on /checkout.`);

    // The signs, the version and the pages the sessions were in.
    await expect(panel(page).locator('[data-rum-kind="rage"]')).toContainText(String(pay.filter(showsKind('rage_')).length));
    await expect(panel(page).locator('[data-rum-kind="error"]')).toContainText(String(pay.filter(showsKind('error_')).length));
    await expect(panel(page).locator('[data-rum-kind="dead"]')).toHaveCount(0);
    await expect(panel(page).locator('[data-rum-version]')).toHaveCount(new Set(pay.map((row) => row.version)).size);
    await expect(panel(page).locator('[data-rum-page="/checkout"]')).toContainText('100%');
    await expect(panel(page).locator('[data-rum-panel-section="trend"]')).toBeVisible();

    await panel(page).getByRole('button', { name: 'Close' }).click();
    await expect(page).not.toHaveURL(/[?&]open=/);
    await expect(panel(page)).toHaveCount(0);
  });

  test('lists the newest sessions that met a sign, each with a replay that opens before the sign, and one without a replay as a session', async ({ page }) => {
    await open(page);
    await rowNamed(page, 'Pay now').getByRole('button').click();

    const pay = WEB_ACTIONS.filter((row) => text(row, 'action.target.name') === 'Pay now');
    const bySession = new Map<string, RumRow[]>();

    for (const row of pay) bySession.set(row['session.id'] as string, [...(bySession.get(row['session.id'] as string) ?? []), row]);

    // The newest session first.
    const newest = [...bySession.values()].sort((a, b) => Math.max(...b.map((row) => row.timestamp)) - Math.max(...a.map((row) => row.timestamp)));

    await expect(panel(page).locator('[data-rum-sample]')).toHaveCount(newest.length);
    await expect(panel(page).locator('[data-rum-sample]').first()).toHaveAttribute('data-rum-sample', newest[0]![0]!['session.id'] as string);
    await expect(panel(page).locator('[data-rum-sample]').first()).toContainText(String(newest[0]![0]!['usr.id']));

    // The replay opens five seconds before the first sign of the session, not before the session began.
    const first = newest[0]!.reduce((a, b) => (b.timestamp < a.timestamp ? b : a));
    const began = Number(first['session.start']) * 1_000;
    const opensAt = Math.max(began, first.timestamp - 5_000_000);
    const link = panel(page).locator('[data-rum-sample]').first().getByRole('link');

    await expect(link).toHaveText(`Replay ${Math.floor((opensAt - began) / 60_000_000)}:${String(Math.floor(((opensAt - began) / 1_000_000) % 60)).padStart(2, '0')}`);
    await link.click();
    await expect(page).toHaveURL(new RegExp(`/rum/sessions/view/${newest[0]![0]!['session.id']}\\?.*at=${Math.round(opensAt)}`));

    // The replay waits there until it is played.
    const session = DATA.sessions.find((row) => row['session.id'] === newest[0]![0]!['session.id'])!;

    await expect(page.getByText(new RegExp(`^${mmss((opensAt - began) / 1_000)} / ${mmss(Number(session['session.duration']) / 1_000_000)}$`))).toBeVisible();
    await expect(page.getByRole('button', { name: 'Play', exact: true })).toBeVisible();
  });

  test('opens a session that recorded no replay as a session, with no moment to open at', async ({ page }) => {
    await open(page);
    await rowNamed(page, 'Open order').getByRole('button').click();

    const link = panel(page).locator('[data-rum-sample]').first().getByRole('link');

    await expect(link).toHaveText('Session');
    await expect(link).not.toHaveAttribute('href', /[?&]at=/);
    await expect(panel(page).locator('[data-rum-sample]').first()).toContainText('Rage clicks · Dead clicks');
    await link.click();
    await expect(page).toHaveURL(/\/rum\/sessions\/view\//);
    await expect(page.locator('[data-rum-devtools]')).toBeVisible();
  });

  test('works for an app: its signs are taps, it has no selector, and its grouping is by screen and system', async ({ page }) => {
    await open(page, `/rum/frustration?app=${APP}`);

    await expect(page.locator('[data-rum-statement]')).toContainText(
      `${sign(APP_ACTIONS, 'rage_', 'rage tap', 'rage taps')}, ${sign(APP_ACTIONS, 'error_', 'error tap', 'error taps')}.`,
    );
    await expect(page.locator('[data-rum-statement]')).not.toContainText('dead');
    await expect(page.locator('[data-rum-frustration-table] [role="columnheader"]')).toHaveText([/^Element$/, /^Rage taps$/, /^Error taps$/, /^Sessions/]);
    await expect(rows(page)).toHaveCount(groupsOf(APP_ACTIONS, ['action.target.name']).length);

    await page.locator('[data-rum-group]').click();
    await expect(page.getByRole('option')).toHaveText(['Element', 'Screen', 'Version', 'System']);
    await page.keyboard.press('Escape');

    await page.locator('[data-rum-kind-filter]').click();
    await expect(page.getByRole('option')).toHaveText(['All', 'Rage taps', 'Error taps']);
  });

  test('is the view the address says it is, so that a copy of it opens the same', async ({ page }) => {
    await open(page, `/rum/frustration?by=page&kind=rage&sort=rage&dir=asc&open=${encodeURIComponent('["/"]')}`);

    await expect(page.locator('[data-rum-group]')).toContainText('Page');
    await expect(page.locator('[data-rum-kind-filter]')).toContainText('Rage clicks');
    await expect(page.locator('[data-rum-column="rage"]')).toHaveAttribute('aria-sort', 'ascending');
    await expect(panel(page)).toContainText('/');
    // A page is not told by a page: the panel does not offer the pages it happened on.
    await expect(panel(page).locator('[data-rum-panel-section="pages"]')).toHaveCount(0);
  });

  test('has nothing to say of an environment with no sign of frustration, and offers to widen the range', async ({ page }) => {
    await open(page, '/rum/frustration?env=staging');

    await expect(page.getByText('No signs of frustration in this range')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Expand the time range' })).toBeVisible();
  });

  test('offers another go for the table whose read failed, and keeps the statement', async ({ page }) => {
    await page.route(/\/api\/v1\/rum\/aggregate\?.*groupBy/, (route) => route.fulfill({ status: 500, json: { error: 'internal', message: 'the engine is busy' } }));
    await open(page);

    await expect(page.locator('[data-rum-overview-card="frustration-table"]').getByRole('button', { name: 'Retry' })).toBeVisible();
    await expect(page.locator('[data-rum-statement]')).toBeVisible();
  });
});

test.describe('RUM frustration in Chinese', () => {
  test('says its words in the language of the product', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });
    await open(page);

    await expect(page.locator('[data-rum-navigation="surface"] [aria-current="page"]').first()).toContainText('受挫');
    await expect(page.locator('[data-rum-statement]')).toContainText('的会话出现了受挫信号：狂点');
    await expect(page.locator('[data-rum-column="rage"]')).toContainText('狂点');
    await expect(rows(page).filter({ hasText: 'div.hero-banner' }).getByRole('cell').first()).toHaveText('（无名称）');

    await rowNamed(page, 'Pay now').getByRole('button').click();
    await expect(panel(page).locator('[data-rum-panel-sentence]')).toContainText('个会话在这个元素上遇到过受挫信号');
    await expect(panel(page).locator('[data-rum-sample]').first().getByRole('link')).toContainText('回放');
  });
});
