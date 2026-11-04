import type { Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM, mockRumData } from '../fixtures/mockRumData';
import { duration, grouped, ms, p75, percent } from '../fixtures/rumFigures';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const DATA = mockRumData(NOW);

const WEB = MOCK_RUM.applications.web;
const TASKS = DATA.long_tasks.filter((row) => row['application.id'] === WEB);
const VIEWS = DATA.views.filter((row) => row['application.id'] === WEB);

const SCRIPT = ['long_task.top_script.source_url', 'long_task.top_script.function'] as const;

const key = (values: ReadonlyArray<string | null>) => JSON.stringify(values);

const rows = (page: Page) => page.locator('[data-rum-long-tasks-table] [data-rum-row]');

const row = (page: Page, values: ReadonlyArray<string | null>) => page.locator(`[data-rum-long-tasks-table] [data-rum-row='${key(values)}']`);

/** A script as the table writes it: its address without the scheme. */
const written = (script: string) => script.replace(/^[a-z][a-z0-9+.-]*:\/\//i, '');

async function open(page: Page, path = '/rum/performance/long-tasks') {
  await page.goto(path);
  await expect(page).toHaveURL(/[?&]time=/);
}

test.describe('RUM performance: long tasks', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('says how many long tasks ran, how many page views met one, and how long they ran and held up the input', async ({ page }) => {
    await open(page);

    const met = VIEWS.filter((view) => typeof view['view.long_task.count'] === 'number' && (view['view.long_task.count'] as number) > 0).length;
    const statement = page.locator('[data-rum-statement]');

    expect(TASKS.length).toBeGreaterThan(0);
    await expect(statement).toContainText(`${TASKS.length} long tasks ran on the main thread`);
    await expect(statement).toContainText(`${percent(met / VIEWS.length)} of the page views met at least one`);
    await expect(statement).toContainText(`ran for ${duration(p75(TASKS.map((task) => ms(task, 'long_task.duration') as number)))}`);
    await expect(statement).toContainText(`held up input for ${duration(p75(TASKS.map((task) => ms(task, 'long_task.blocking_duration') as number)))}`);
  });

  test('lists the scripts the long tasks are put down to, each with its function, the most long tasks first', async ({ page }) => {
    await open(page);

    const groups = grouped(TASKS, SCRIPT);

    await expect(rows(page)).toHaveCount(groups.length);
    expect(await rows(page).evaluateAll((elements) => elements.map((element) => element.getAttribute('data-rum-row')))).toEqual(groups.map((group) => key(group.values)));
    await expect(page.locator('[data-rum-table-count]')).toContainText(`${groups.length} groups`);

    const top = groups[0]!;
    const cells = row(page, top.values).getByRole('cell');
    const durations = top.rows.map((task) => ms(task, 'long_task.duration') as number);

    await expect(cells.nth(0)).toHaveText(written(top.values[0] as string));
    await expect(cells.nth(1)).toHaveText(top.values[1] as string);
    await expect(cells.nth(2)).toHaveText(String(top.rows.length));
    await expect(cells.nth(3)).toHaveText(String(new Set(top.rows.map((task) => task['view.id'])).size));
    await expect(cells.nth(4)).toHaveText(duration(p75(durations)));
  });

  test('tells the long tasks of a browser that names no script as a group of their own, and says what they are', async ({ page }) => {
    await open(page);

    const unnamed = TASKS.filter((task) => task['long_task.top_script.source_url'] === undefined);

    expect(unnamed.length).toBeGreaterThan(0);
    await expect(row(page, [null, null])).toHaveAttribute('data-rum-unattributed', '');
    await expect(row(page, [null, null]).getByRole('cell').nth(0)).toHaveText('Unattributed');
    await expect(row(page, [null, null]).getByRole('cell').nth(2)).toHaveText(String(unnamed.length));

    await row(page, [null, null]).getByRole('button').click();
    await expect(page.locator('[data-rum-panel="long-tasks"]')).toContainText('Safari and Firefox report a long task without the script');
  });

  test('colours the time of a long task: a tenth of a second is worth a word, a fifth of a second is a fault', async ({ page }) => {
    await open(page);

    const heavy = grouped(TASKS, SCRIPT).find((group) => group.rows.length >= 30 && p75(group.rows.map((task) => ms(task, 'long_task.duration') as number)) >= 200)!;

    expect(heavy).toBeDefined();
    await expect(row(page, heavy.values)).not.toHaveAttribute('data-rum-few', '');
    await expect(row(page, heavy.values).getByRole('cell').nth(4)).toHaveClass(/red-ink/);

    // A group with a handful of long tasks is grey, and no ranking puts it first.
    const few = grouped(TASKS, SCRIPT).find((group) => group.rows.length < 30)!;

    await expect(row(page, few.values)).toHaveAttribute('data-rum-few', '');
    await page.locator('[data-rum-column="p75"] button').click();
    await expect(page).toHaveURL(/[?&]sort=p75/);

    const names = await rows(page).evaluateAll((elements) => elements.map((element) => element.hasAttribute('data-rum-few')));

    expect(names.indexOf(true)).toBeGreaterThan(0);
    expect(names.slice(names.indexOf(true)).every(Boolean)).toBe(true);
  });

  test('narrows by words of a script, and tells the long tasks apart by a page, a version or how they were triggered', async ({ page }) => {
    await open(page);

    await page.getByPlaceholder('Search scripts').fill('checkout');
    await expect(page).toHaveURL(/[?&]q=checkout/);

    const checkout = TASKS.filter((task) => String(task['long_task.top_script.source_url'] ?? '').includes('checkout'));

    await expect(rows(page)).toHaveCount(grouped(checkout, SCRIPT).length);

    // Typing is sent on once it has paused, so the words are gone from the address a moment after they are cleared:
    // a grouping chosen in that moment would be written over by it.
    await page.getByPlaceholder('Search scripts').fill('');
    await expect(page).not.toHaveURL(/[?&]q=/);
    await page.locator('[data-rum-group]').click();
    await page.getByRole('option', { name: 'Page' }).click();
    await expect(page).toHaveURL(/[?&]by=page/);

    const pages = grouped(TASKS, ['view.url_path_group']);

    await expect(rows(page)).toHaveCount(pages.length);
    await expect(row(page, pages[0]!.values).getByRole('cell').nth(1)).toHaveText(String(pages[0]!.rows.length));

    await page.locator('[data-rum-group]').click();
    await page.getByRole('option', { name: 'How it was triggered' }).click();
    await expect(page).toHaveURL(/[?&]by=invoker/);
    await expect(row(page, ['event-listener']).getByRole('cell').first()).toHaveText('Event listener');
  });

  test('opens what the long tasks of a script are made of: how they were triggered, where, and the longest of them', async ({ page }) => {
    await open(page);

    const top = grouped(TASKS, SCRIPT)[0]!;

    await row(page, top.values).getByRole('button').click();
    await expect(page).toHaveURL(/[?&]open=/);

    const panel = page.locator('[data-rum-panel="long-tasks"]');

    await expect(panel).toContainText(top.values[1] as string);
    await expect(panel).toContainText(`${top.rows.length} long tasks`);
    await expect(row(page, top.values)).toHaveClass(/floating-item-selected/);

    const invokers = new Set(top.rows.map((task) => task['long_task.top_script.invoker_type']));

    await expect(panel.locator('[data-rum-invoker]')).toHaveCount(invokers.size);
    await expect(panel.locator('[data-rum-invoker]').first()).toContainText('Event listener');
    await expect(panel.locator('[data-rum-page="/checkout"]')).toContainText('100%');
    await expect(panel.locator('[data-rum-panel-sentence]')).toContainText('Most happened on /checkout');

    // The longest three, the longest first.
    const longest = [...top.rows].sort((a, b) => (ms(b, 'long_task.duration') as number) - (ms(a, 'long_task.duration') as number)).slice(0, 3);

    await expect(panel.locator('[data-rum-sample]')).toHaveCount(3);
    await expect(panel.locator('[data-rum-sample]').first()).toContainText(duration(ms(longest[0]!, 'long_task.duration') as number));

    await panel.getByRole('button', { name: 'Close' }).click();
    await expect(page).not.toHaveURL(/[?&]open=/);
    await expect(panel).toHaveCount(0);
  });

  test('opens the load of the view a long task blocked, with that long task chosen on the main thread', async ({ page }) => {
    await open(page);

    const top = grouped(TASKS, SCRIPT)[0]!;

    await row(page, top.values).getByRole('button').click();

    const sample = page.locator('[data-rum-panel="long-tasks"] [data-rum-sample]').first();
    const id = await sample.getAttribute('data-rum-sample');

    await sample.click();
    await expect(page).toHaveURL(new RegExp(`[?&]focus=${id}`));

    const drawer = page.locator('[data-rum-drawer]');

    await expect(drawer.locator('[data-rum-waterfall]')).toBeVisible();
    await expect(drawer.locator('[data-rum-waterfall-lane]')).toBeVisible();
    await expect(drawer.locator(`[data-rum-waterfall-task="${id}"]`)).toHaveAttribute('aria-pressed', 'true');

    const detail = drawer.locator('[data-rum-waterfall-detail="task"]');

    await expect(detail).toContainText(top.values[1] as string);
    await expect(detail).toContainText('Event listener');

    // Another long task on the lane takes the panel.
    const other = drawer.locator('[data-rum-waterfall-task]').nth(1);

    await other.click();
    await expect(other).toHaveAttribute('aria-pressed', 'true');
    await expect(drawer.locator(`[data-rum-waterfall-task="${id}"]`)).toHaveAttribute('aria-pressed', 'false');

    await page.keyboard.press('Escape');
    await expect(page).not.toHaveURL(/[?&]view=/);
    await expect(drawer).toHaveCount(0);
  });

  test('is the view the address says it is, so that a copy of it opens the same', async ({ page }) => {
    await open(page, `/rum/performance/long-tasks?by=page&sort=blocking&open=${encodeURIComponent('["/checkout"]')}`);

    await expect(page.locator('[data-rum-group]')).toContainText('Page');
    await expect(page.locator('[data-rum-column="blocking"]')).toHaveAttribute('aria-sort', 'descending');
    await expect(page.locator('[data-rum-panel="long-tasks"]')).toContainText('/checkout');
    // A page is not told by a script: the panel does not offer the pages a page happened on.
    await expect(page.locator('[data-rum-panel-section="pages"]')).toHaveCount(0);
  });

  test('has nothing to say of an application with no long tasks, and offers to widen the range', async ({ page }) => {
    await open(page, '/rum/performance/long-tasks?env=staging');

    await expect(page.getByText('No long tasks in this range')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Expand the time range' })).toBeVisible();
  });

  test('offers another go for the table whose read failed, and keeps the statement', async ({ page }) => {
    await page.route(/\/api\/v1\/rum\/aggregate\?.*groupBy/, (route) => route.fulfill({ status: 500, json: { error: 'internal', message: 'the engine is busy' } }));
    await open(page);

    await expect(page.locator('[data-rum-overview-card="long-tasks-table"]').getByRole('button', { name: 'Retry' })).toBeVisible();
    await expect(page.locator('[data-rum-statement]')).toBeVisible();
  });
});

test.describe('RUM performance: long tasks in Chinese', () => {
  test('says its words in the language of the product', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });
    await open(page);

    await expect(page.locator('[data-rum-subnavigation="performance"]')).toContainText('长任务');
    await expect(page.locator('[data-rum-statement]')).toContainText(`主线程共运行了 ${TASKS.length} 个长任务`);
    await expect(page.locator('[data-rum-column="blocking"]')).toContainText('p75 阻塞');
    await expect(row(page, [null, null]).getByRole('cell').nth(0)).toHaveText('未归因');

    await rows(page).first().getByRole('button').click();
    await expect(page.locator('[data-rum-panel="long-tasks"]')).toContainText('触发方式');
    await expect(page.locator('[data-rum-panel="long-tasks"]')).toContainText('事件监听');
  });
});
