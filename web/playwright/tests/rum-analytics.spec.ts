import type { Page } from '@playwright/test';

import type { RumRow } from '../../src/api/rum';
import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM, mockRumData } from '../fixtures/mockRumData';
import { duration, percent } from '../fixtures/rumFigures';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const DATA = mockRumData(NOW);
const DAY = 86_400_000_000;

const WEB = MOCK_RUM.applications.web;
const GROUP = 'view.url_path_group';

const ofWeb = (rows: readonly RumRow[]) => rows.filter((row) => row['application.id'] === WEB);
const VIEWS = ofWeb(DATA.views);
const ACTIONS = ofWeb(DATA.actions);
const SESSIONS = ofWeb(DATA.sessions);

/** A range that holds every visit of the mock organization, and whole days around them. */
const FROM = '2026-05-22T00:00:00.000Z';
const TO = '2026-05-25T00:00:00.000Z';
const RANGE = `${FROM}..${TO}`;
const TO_MICROS = Date.parse(TO) * 1_000;

// ---------------------------------------------------------------------------------------------
// The answers, worked out again from the rows by the plain statement of each question.

interface Step {
  rows: readonly RumRow[];
  holds: (row: RumRow) => boolean;
}

const viewOf = (group: string): Step => ({ rows: VIEWS, holds: (row) => row[GROUP] === group });
const actionOf = (name: string): Step => ({ rows: ACTIONS, holds: (row) => row['action.target.name'] === name });

const bySession = (rows: readonly RumRow[]) => {
  const sessions = new Map<string, RumRow[]>();

  for (const row of rows) sessions.set(String(row['session.id']), [...(sessions.get(String(row['session.id'])) ?? []), row]);

  return sessions;
};

/** For each session that did the first step: the moments of the steps it got through, one after the other. */
function reached(steps: readonly Step[], windowSecs?: number) {
  const found = steps.map((step) => bySession(step.rows.filter(step.holds)));
  const result: Array<{ first: RumRow; times: number[] }> = [];

  for (const [session, rows] of found[0] as Map<string, RumRow[]>) {
    const first = [...rows].sort((a, b) => a.timestamp - b.timestamp)[0] as RumRow;
    const times = [first.timestamp];

    for (const step of found.slice(1)) {
      const later = (step.get(session) ?? [])
        .map((row) => row.timestamp)
        .filter((at) => at > (times.at(-1) as number) && (windowSecs === undefined || at <= first.timestamp + windowSecs * 1_000_000))
        .sort((a, b) => a - b)[0];

      if (later === undefined) break;

      times.push(later);
    }

    result.push({ first, times });
  }

  return result;
}

const counts = (sessions: ReturnType<typeof reached>, steps: number) => Array.from({ length: steps }, (_, k) => sessions.filter((entry) => entry.times.length > k).length);

const median = (values: number[]) => {
  const sorted = [...values].sort((a, b) => a - b);
  const half = Math.floor(sorted.length / 2);

  return sorted.length % 2 === 1 ? (sorted[half] as number) : ((sorted[half - 1] as number) + (sorted[half] as number)) / 2;
};

/** The pages of each session in the order they were opened, one for a page opened again at once. */
const journeys = () =>
  [...bySession(VIEWS).values()].map((rows) =>
    rows
      .sort((a, b) => a.timestamp - b.timestamp)
      .map((row) => String(row[GROUP]))
      .filter((page, index, all) => index === 0 || all[index - 1] !== page),
  );

/** How many sessions were on each page, `depth` pages from the one the paths start at (or end at). */
function pagesAt(depth: number, anchor: string | null, backward: boolean) {
  const found = new Map<string, number>();

  for (const pages of journeys()) {
    const at = pages.flatMap((page, index) => (anchor === null || page === anchor ? [index] : []));

    if (at.length === 0) continue;

    const origin = backward ? Math.max(...at) : Math.min(...at);
    const page = pages[backward ? origin - depth : origin + depth];

    if (page !== undefined) found.set(page, (found.get(page) ?? 0) + 1);
  }

  return found;
}

// ---------------------------------------------------------------------------------------------

const step = (kind: 'v' | 'a', field: string, value: string) => `${kind}:${field}=${encodeURIComponent(value)}`;
const pageStep = (group: string) => step('v', GROUP, group);
const actionStep = (name: string) => step('a', 'action.target.name', name);

const address = (path: string, params: Record<string, string> = {}) => `${path}?${new URLSearchParams({ time: RANGE, ...params })}`;

const funnelAddress = (steps: string[], more: Record<string, string> = {}) => address('/rum/analytics', { steps: steps.join('|'), ...more });

const funnelStep = (page: Page, index: number) => page.locator(`[data-rum-funnel-step="${index}"]`);

async function expectFunnel(page: Page, steps: readonly Step[], windowSecs?: number) {
  const sessions = reached(steps, windowSecs);
  const total = counts(sessions, steps.length);

  for (const [index, sessionsAtStep] of total.entries()) {
    const row = funnelStep(page, index);

    await expect(row).toHaveAttribute('data-rum-funnel-sessions', String(sessionsAtStep));
    await expect(row).toContainText(total[0] ? percent(sessionsAtStep / total[0]) : '0%');
  }

  return { sessions, total };
}

async function useChinese(page: Page) {
  await page.addInitScript(() => {
    localStorage.setItem('molesignal-ui-prefs', JSON.stringify({ state: { language: 'zh-cn' }, version: 0 }));
  });
}

/** The names of the views of the tab, once they are there. */
async function viewTabs(page: Page) {
  const tabs = page.locator('[data-rum-subnavigation="analytics"] a');

  await expect(tabs).toHaveCount(4);

  return (await tabs.allInnerTexts()).map((tab) => tab.trim());
}

const node = (page: Page, column: number, name: string) => page.locator(`[data-rum-path-node="${column}:${name}"]`);

test.describe('RUM analytics', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('is a tab of its own after frustration, with the funnel, the paths and the retention to choose from', async ({ page }) => {
    await page.goto(address('/rum/analytics'));

    // A tab lays its label out twice, the second copy unseen, and a read made while the page settles can get both: read until it holds.
    await expect
      .poll(async () => (await page.locator('[data-rum-navigation="surface"] a').allInnerTexts()).map((tab) => tab.trim()).slice(0, 9))
      .toEqual(['Overview', 'Explorer', 'Playlists', 'Errors', 'Performance', 'Frustration', 'Analytics', 'Feature flags', 'Applications']);
    await expect(page.locator('[data-rum-navigation="surface"] [aria-current="page"]').first()).toContainText('Analytics');
    await expect.poll(() => viewTabs(page)).toEqual(['Funnel', 'Paths', 'Retention', 'Heatmap']);
    await expect(page.locator('[data-rum-analytics="funnel"]')).toBeVisible();

    await page.locator('[data-rum-subnavigation="analytics"] a', { hasText: 'Paths' }).click();
    await expect(page).toHaveURL(/\/rum\/analytics\/paths\?/);
    await expect(page.locator('[data-rum-navigation="surface"] [aria-current="page"]').first()).toContainText('Analytics');
    await expect(page.locator('[data-rum-analytics="paths"]')).toBeVisible();
  });

  test('opens on the last 7 days when nothing chose a range, as a funnel needs days to have people in it', async ({ page }) => {
    await page.goto('/rum/analytics');

    await expect(page).toHaveURL((url) => new URLSearchParams(url.search).get('time') === 'now-7d..now');
  });

  test('asks for the steps to be named before it counts anything', async ({ page }) => {
    await page.goto(address('/rum/analytics'));

    await expect(page.locator('[data-rum-funnel-step-row]')).toHaveCount(2);
    await expect(page.getByText('Name what each step looks for')).toBeVisible();
    await expect(page.locator('[data-rum-funnel]')).toHaveCount(0);
  });

  test('counts the sessions that did the steps in order, each step by the first row after the one before', async ({ page }) => {
    const steps = [viewOf('/products'), viewOf('/cart'), viewOf('/checkout')];

    await page.goto(funnelAddress(['/products', '/cart', '/checkout'].map(pageStep)));

    const { total } = await expectFunnel(page, steps);

    // Two sessions looked at products; both went to the cart; one of them paid at checkout.
    expect(total).toEqual([2, 2, 1]);
    await expect(funnelStep(page, 0)).toContainText('/products');
    await expect(funnelStep(page, 2)).toContainText('/checkout');
    // The drop-off is what is lost from the step before.
    await expect(funnelStep(page, 2)).toContainText(`−${percent(1 - 1 / 2)}`);
  });

  test('takes an action as a step and says how long sessions took to get to it', async ({ page }) => {
    const steps = [viewOf('/cart'), viewOf('/checkout'), actionOf('Pay now')];

    await page.goto(funnelAddress([pageStep('/cart'), pageStep('/checkout'), actionStep('Pay now')]));

    const { sessions, total } = await expectFunnel(page, steps);

    expect(total[0]).toBeGreaterThan(total[2] as number);
    expect(total[2]).toBeGreaterThan(0);

    const gaps = sessions.filter((entry) => entry.times.length > 1).map((entry) => (entry.times[1] as number) - (entry.times[0] as number));

    // The median of the times between the first step and the second, which the row of the second step says.
    await expect(funnelStep(page, 1)).toContainText(duration(median(gaps) / 1_000));
    await expect(funnelStep(page, 2)).toContainText('Pay now');
  });

  test('counts only the sessions that got through within the time limit', async ({ page }) => {
    const steps = [viewOf('/products'), viewOf('/cart'), viewOf('/checkout')];
    const all = counts(reached(steps), 3);
    const limited = counts(reached(steps, 300), 3);

    // The limit decides something: a session that took longer than five minutes is left out.
    expect(limited).not.toEqual(all);

    await page.goto(funnelAddress(['/products', '/cart', '/checkout'].map(pageStep), { win: '300' }));
    await expectFunnel(page, steps, 300);
    await expect(page.getByRole('combobox', { name: 'Time limit' })).toContainText('Within 5 minutes');
  });

  test('counts the sessions by a field, one funnel for each value of it', async ({ page }) => {
    const steps = [viewOf('/cart'), viewOf('/checkout')];
    const sessions = reached(steps);
    const browsers = [...new Set(sessions.map((entry) => String(entry.first['browser.name'])))];

    await page.goto(funnelAddress(['/cart', '/checkout'].map(pageStep), { by: 'browser.name' }));
    await expect(page.locator('[data-rum-overview-card="funnel-groups"]')).toContainText('Funnel by browser');

    expect(browsers.length).toBeGreaterThan(1);

    for (const browser of browsers) {
      const of = sessions.filter((entry) => entry.first['browser.name'] === browser);
      const row = page.locator(`[data-rum-funnel-group="${browser}"]`);
      const [first, second] = counts(of, 2) as [number, number];

      await expect(row).toContainText(String(first));
      await expect(row).toContainText(`${second}`);
      await expect(row.getByRole('cell').last()).toHaveText(percent(second / first));
    }
  });

  test('builds a funnel from the pages the data has, and keeps it in the address', async ({ page }) => {
    await page.goto(address('/rum/analytics'));

    const choose = async (index: number, value: string) => {
      await page.getByRole('combobox', { name: `Value of step ${index}` }).click();
      await page.getByRole('option', { name: value, exact: true }).click();
    };

    await choose(1, '/products');
    await choose(2, '/cart');

    await expectFunnel(page, [viewOf('/products'), viewOf('/cart')]);
    await expect(page).toHaveURL((url) => new URLSearchParams(url.search).get('steps') === [pageStep('/products'), pageStep('/cart')].join('|'));

    // A third step is an action, chosen by what it is called.
    await page.locator('[data-rum-funnel-add]').click();
    await page.getByRole('combobox', { name: 'What step 3 looks at' }).click();
    await page.getByRole('option', { name: 'Action', exact: true }).click();
    await choose(3, 'Add to cart');

    await expectFunnel(page, [viewOf('/products'), viewOf('/cart'), actionOf('Add to cart')]);
    await expect(page.locator('[data-rum-funnel-step-row]')).toHaveCount(3);

    await page.getByRole('button', { name: 'Remove step 3' }).click();
    await expect(page.locator('[data-rum-funnel-step-row]')).toHaveCount(2);
    await expect(funnelStep(page, 2)).toHaveCount(0);
  });

  test('narrows the values it offers as they are typed, and takes the one that is chosen', async ({ page }) => {
    await page.goto(address('/rum/analytics'));

    await page.getByRole('combobox', { name: 'Value of step 1' }).click();
    await page.getByPlaceholder('Search or type a value').fill('ord');
    // What is typed is offered as it is, and the values it narrows the rest to are offered after it.
    await expect(page.getByRole('option', { name: 'Use "ord"' })).toBeVisible();
    await expect(page.getByRole('option', { name: '/orders/:id', exact: true })).toBeVisible();
    await expect(page.getByRole('option', { name: '/cart', exact: true })).toHaveCount(0);
    await page.getByRole('option', { name: '/orders', exact: true }).click();

    await page.getByRole('combobox', { name: 'Value of step 2' }).click();
    await page.getByRole('option', { name: '/orders/:id', exact: true }).click();

    await expectFunnel(page, [viewOf('/orders'), viewOf('/orders/:id')]);
  });

  test('shows the pages sessions opened first, and the pages they opened after them, as columns', async ({ page }) => {
    await page.goto(address('/rum/analytics/paths'));

    await expect(page.locator('[data-rum-paths-chart]')).toBeVisible();

    for (const depth of [0, 1]) {
      const expected = pagesAt(depth, null, false);

      expect(expected.size).toBeGreaterThan(1);

      for (const [name, sessions] of expected) {
        await expect(node(page, depth, name)).toHaveAttribute('data-rum-path-sessions', String(sessions));
      }
    }

    // Every session starts somewhere: the first column is all of them.
    const firsts = [...pagesAt(0, null, false).values()].reduce((sum, sessions) => sum + sessions, 0);

    expect(firsts).toBe(new Set(VIEWS.map((row) => row['session.id'])).size);
  });

  test('starts the paths from a page when it is clicked, and the address says so', async ({ page }) => {
    await page.goto(address('/rum/analytics/paths'));
    await node(page, 0, '/products').locator('rect').click();

    await expect(page).toHaveURL((url) => new URLSearchParams(url.search).get('pivot') === '/products');
    // `anchor` in the address is the moment the shell pins: a page kept under that name comes back after it is let go of.
    expect(new URL(page.url()).searchParams.has('anchor')).toBe(false);
    await expect(page.getByRole('combobox', { name: 'The page the paths are of' })).toContainText('/products');

    for (const [name, sessions] of pagesAt(0, '/products', false)) {
      await expect(node(page, 0, name)).toHaveAttribute('data-rum-path-sessions', String(sessions));
    }

    for (const [name, sessions] of pagesAt(1, '/products', false)) {
      await expect(node(page, 1, name)).toHaveAttribute('data-rum-path-sessions', String(sessions));
    }

    // The page the paths start from is not offered again as a place to start from.
    await expect(node(page, 0, '/products')).not.toHaveAttribute('role', 'button');

    // Let go of, the paths start from the first page of each session again, and the page is offered again.
    await page.locator('[data-rum-paths-clear]').click();
    await expect(page).toHaveURL((url) => !new URLSearchParams(url.search).has('pivot'));
    await expect(page.locator('[data-rum-paths-clear]')).toHaveCount(0);
    await expect(page.getByRole('combobox', { name: 'The page the paths are of' })).not.toContainText('/products');
    await expect(node(page, 0, '/products')).toHaveAttribute('role', 'button');
  });

  test('follows the sessions back from a page, the page they end at being the last column', async ({ page }) => {
    await page.goto(address('/rum/analytics/paths', { dir: 'before', pivot: '/checkout' }));

    await expect(page.locator('[data-rum-paths-chart]')).toBeVisible();

    // The columns nobody got to are not drawn: the anchor is the last of those that are.
    const reach = [0, 1, 2, 3].filter((depth) => pagesAt(depth, '/checkout', true).size > 0).length;

    expect(reach).toBe(3);

    for (const depth of [0, 1, 2]) {
      for (const [name, sessions] of pagesAt(depth, '/checkout', true)) {
        await expect(node(page, reach - 1 - depth, name)).toHaveAttribute('data-rum-path-sessions', String(sessions));
      }
    }
  });

  test('chooses the page the paths are of from the pages the data has', async ({ page }) => {
    await page.goto(address('/rum/analytics/paths'));

    await page.getByRole('combobox', { name: 'The page the paths are of' }).click();
    await page.getByRole('option', { name: '/cart', exact: true }).click();

    await expect(page).toHaveURL((url) => new URLSearchParams(url.search).get('pivot') === '/cart');

    for (const [name, sessions] of pagesAt(1, '/cart', false)) {
      await expect(node(page, 1, name)).toHaveAttribute('data-rum-path-sessions', String(sessions));
    }
  });

  test('shows who came back: the people first seen on a day, and the share of them seen on each day after', async ({ page }) => {
    await page.goto(address('/rum/analytics/retention'));

    const person = (row: RumRow) => String(row['usr.id'] ?? row['usr.anonymous_id']);
    const first = new Map<string, number>();
    const active = new Map<string, Set<number>>();

    for (const row of SESSIONS) {
      const day = Math.floor(row.timestamp / DAY);

      first.set(person(row), Math.min(first.get(person(row)) ?? day, day));
      active.set(person(row), (active.get(person(row)) ?? new Set()).add(day));
    }

    const cohorts = [...new Set(first.values())].sort((a, b) => a - b);

    expect(cohorts.length).toBe(2);

    for (const cohort of cohorts) {
      const people = [...first].filter(([, day]) => day === cohort).map(([id]) => id);
      const start = cohort * DAY;
      const row = page.locator(`[data-rum-retention-cohort="${start}"]`);

      await expect(row.getByRole('rowheader')).toHaveText(new Date(start / 1_000).toISOString().slice(0, 10));
      await expect(row.getByRole('cell').first()).toHaveText(String(people.length));

      for (const later of [1, 2, 3]) {
        const cell = page.locator(`[data-rum-retention-cell="${start}:${later}"]`);
        const ended = start + (later + 1) * DAY <= TO_MICROS;
        const returned = people.filter((id) => active.get(id)?.has(cohort + later)).length;

        // A day that has not ended by the end of the range cannot say who came back.
        await expect(cell).toHaveText(ended ? percent(returned / people.length) : '—');
      }
    }

    await expect(page.locator('[data-rum-retention-short]')).toHaveCount(0);

    // The one person seen on the first day came back on the second: all of them.
    const firstDay = cohorts[0] as number;

    await expect(page.locator(`[data-rum-retention-cell="${firstDay * DAY}:1"]`)).toHaveText('100%');
    await expect(page.locator('[data-rum-retention-average="1"]')).not.toHaveText('—');
  });

  test('speaks of screens and their names for an app, and has no page group to choose', async ({ page }) => {
    await page.goto(address('/rum/analytics', { app: MOCK_RUM.applications.app }));

    await expect(page.getByRole('combobox', { name: 'What step 1 looks at' })).toContainText('Screen');
    await expect(page.getByRole('combobox', { name: 'Field of step 1' })).toContainText('View name');

    await page.goto(address('/rum/analytics/paths', { app: MOCK_RUM.applications.app }));

    await expect(page.locator('[data-rum-paths-chart]')).toBeVisible();
    await expect(page.locator('[data-rum-paths-by]')).toHaveCount(0);

    // The first screens of the app are the ones its journeys began on.
    const first = new Map<string, number>();

    for (const rows of bySession(DATA.views.filter((row) => row['application.id'] === MOCK_RUM.applications.app)).values()) {
      const name = String([...rows].sort((a, b) => a.timestamp - b.timestamp)[0]?.['view.name']);

      first.set(name, (first.get(name) ?? 0) + 1);
    }

    expect(first.size).toBeGreaterThan(0);

    for (const [name, sessions] of first) {
      await expect(node(page, 0, name)).toHaveAttribute('data-rum-path-sessions', String(sessions));
    }
  });

  test('says when the range is too short for any period to have ended, and offers a longer one', async ({ page }) => {
    await page.goto(address('/rum/analytics/retention', { time: 'now-24h..now' }));

    const hint = page.locator('[data-rum-retention-short]');

    await expect(hint).toBeVisible();
    await expect(page.locator('[data-rum-retention-cohort]')).not.toHaveCount(0);

    await hint.getByRole('button', { name: 'Expand the time range' }).click();
    await expect(page).toHaveURL((url) => new URLSearchParams(url.search).get('time') !== 'now-24h..now');
  });

  test('counts weeks as well as days, the week beginning on Monday', async ({ page }) => {
    await page.goto(address('/rum/analytics/retention', { period: 'week' }));

    const monday = Date.parse('2026-05-18T00:00:00.000Z') * 1_000;
    const people = new Set(SESSIONS.map((row) => String(row['usr.id'] ?? row['usr.anonymous_id'])));
    const row = page.locator(`[data-rum-retention-cohort="${monday}"]`);

    await expect(page.locator('[data-rum-retention-cohort]')).toHaveCount(1);
    await expect(row.getByRole('rowheader')).toHaveText('2026-05-18');
    await expect(row.getByRole('cell').first()).toHaveText(String(people.size));
    // The next week has not ended by the end of the range.
    await expect(page.locator(`[data-rum-retention-cell="${monday}:1"]`)).toHaveText('—');
  });
});

test.describe('RUM analytics in Chinese', () => {
  test('is written in the language of the product for those who read it so', async ({ page, mockServer }) => {
    await useChinese(page);
    await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });
    await page.goto(address('/rum/analytics'));

    await expect(page.locator('[data-rum-navigation="surface"] [aria-current="page"]').first()).toContainText('分析');
    await expect.poll(() => viewTabs(page)).toEqual(['漏斗', '路径', '留存', '热力图']);
    await expect(page.getByText('给每一步指定要找什么')).toBeVisible();

    await page.goto(address('/rum/analytics/retention'));
    await expect(page.locator('[data-rum-analytics="retention"]')).toContainText('首次出现的日期');
  });
});
