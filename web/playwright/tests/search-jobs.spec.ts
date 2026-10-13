import { readFile } from 'node:fs/promises';

import type { Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import {
  MOCK_SEARCH_JOB,
  setMockSearchJobs,
  setMockSearchJobViewer,
  updateMockSearchJob,
} from '../fixtures/mockSearchJobs';

const NOW_MICROS = Date.parse(FROZEN_NOW_ISO) * 1000;

const row = (page: Page, id: string) =>
  page.locator('tr', { has: page.locator(`[data-search-job="${id}"]`) });
const view = (page: Page, name: string) =>
  page.getByRole('group', { name: 'Job state' }).getByRole('button', { name: new RegExp(`^${name}`) });
const listedIds = (page: Page) =>
  page
    .locator('[data-search-job]')
    .evaluateAll((cells) => cells.map((cell) => (cell as HTMLElement).dataset.searchJob));

/** Every request the page makes for the list of jobs, as it is made. */
function watchListReads(page: Page): { count: () => number } {
  let reads = 0;
  page.on('request', (request) => {
    if (new URL(request.url()).pathname === '/api/v1/query/jobs') reads += 1;
  });
  return { count: () => reads };
}

/** Makes the page what it is in a tab nobody is looking at, or brings it back. */
async function setPageHidden(page: Page, hidden: boolean) {
  await page.evaluate((state) => {
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => state });
    Object.defineProperty(document, 'hidden', { configurable: true, get: () => state === 'hidden' });
    document.dispatchEvent(new Event('visibilitychange'));
    window.dispatchEvent(new Event('visibilitychange'));
  }, hidden ? 'hidden' : 'visible');
}

test.describe('search jobs', () => {
  test('the stream list leads to the jobs, each said in a row', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);

    await page.goto('/streams');
    await page.getByRole('button', { name: 'Search jobs' }).click();

    await expect(page).toHaveURL(/\/streams\/jobs$/);
    await expect(page.getByRole('heading', { level: 1, name: 'Search jobs' })).toBeVisible();
    // The page belongs to Streams: its entry stays lit and leads back.
    await expect(
      page.getByRole('navigation').getByRole('link', { name: 'Streams', exact: true }).first(),
    ).toHaveAttribute('aria-current', 'page');

    // Most recently submitted first.
    await expect(row(page, MOCK_SEARCH_JOB.running)).toBeVisible();
    expect(await listedIds(page)).toEqual([
      MOCK_SEARCH_JOB.pendingBackfill,
      MOCK_SEARCH_JOB.running,
      MOCK_SEARCH_JOB.done,
      MOCK_SEARCH_JOB.failed,
      MOCK_SEARCH_JOB.cancelled,
      MOCK_SEARCH_JOB.doneEmpty,
    ]);

    const running = row(page, MOCK_SEARCH_JOB.running);
    await expect(running.getByText('Running', { exact: true })).toBeVisible();
    await expect(running.getByText('Query', { exact: true })).toBeVisible();
    await expect(running.getByText('access_logs', { exact: true })).toBeVisible();
    await expect(
      running.getByText(
        'SELECT status, count(*) AS requests FROM "access_logs" WHERE status >= 500 GROUP BY status',
      ),
    ).toBeVisible();
    await expect(running.getByText('2026-05-22 10:00')).toBeVisible();
    await expect(running.getByText('2026-05-23 10:00')).toBeVisible();
    await expect(running.getByText('Dana Reyes')).toBeVisible();
    await expect(running.getByText('1 min 50 s')).toBeVisible();
    await expect(running.getByText('Not finished')).toBeVisible();

    const done = row(page, MOCK_SEARCH_JOB.done);
    await expect(done.getByText('Done', { exact: true })).toBeVisible();
    await expect(done.getByText('42 s')).toBeVisible();
    await expect(done.getByText('12,480')).toBeVisible();

    const failed = row(page, MOCK_SEARCH_JOB.failed);
    await expect(failed.getByText('Failed', { exact: true })).toBeVisible();
    await expect(failed.getByText(/^Failed: resource exhausted: the query needs 2\.4 GiB/)).toBeVisible();
    await expect(failed.getByTitle('Attempt 2')).toHaveText('2');

    const backfill = row(page, MOCK_SEARCH_JOB.pendingBackfill);
    await expect(backfill.getByText('Queued', { exact: true })).toBeVisible();
    await expect(backfill.getByText('Backfill', { exact: true })).toBeVisible();
    await expect(backfill.getByRole('link', { name: 'Open pipeline' })).toHaveAttribute(
      'href',
      '/pipelines/checkout-logs',
    );

    // Whoever submitted the cancelled job has left; the id is what remains.
    await expect(row(page, MOCK_SEARCH_JOB.cancelled).getByText('u-former-7c41')).toBeVisible();
    // The mock user owns the organization: the list is everybody's and does not say otherwise.
    await expect(page.locator('[data-search-jobs-scope]')).toHaveCount(0);
  });

  test('a member who is not an administrator is shown the jobs they submitted, and told so', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, {
      token: 'custom-role-analyst',
      role: 'Viewer',
      scope: 'organization',
      capabilityPermissions: ['streams.read', 'streams.query'],
    });
    // Kenji Watanabe submitted the backfill and the query that found nothing.
    await setMockSearchJobViewer(mockServer.port, 'u2');

    await page.goto('/streams/jobs');

    await expect(row(page, MOCK_SEARCH_JOB.pendingBackfill)).toBeVisible();
    expect(await listedIds(page)).toEqual([
      MOCK_SEARCH_JOB.pendingBackfill,
      MOCK_SEARCH_JOB.doneEmpty,
    ]);
    await expect(page.locator('[data-search-jobs-scope="own"]')).toHaveText(
      'Only the jobs you submitted',
    );
    // The counts are of the member's own jobs too, and no row repeats the member's name.
    await expect(view(page, 'All')).toHaveText(/2$/);
    await expect(view(page, 'Running')).toHaveText(/0$/);
    await expect(page.getByText('Kenji Watanabe')).toHaveCount(0);
    await expect(row(page, MOCK_SEARCH_JOB.pendingBackfill).getByText('1 minute ago')).toBeVisible();

    // A member manages what is theirs.
    await row(page, MOCK_SEARCH_JOB.pendingBackfill)
      .getByRole('button', { name: /^Cancel job/ })
      .click();
    await page.getByRole('dialog').getByRole('button', { name: 'Cancel job' }).click();

    await expect(
      row(page, MOCK_SEARCH_JOB.pendingBackfill).getByText('Cancelled', { exact: true }),
    ).toBeVisible();
  });

  test('the rows fit the page, with what can be done to a job in reach', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);

    await page.goto('/streams/jobs');
    await expect(row(page, MOCK_SEARCH_JOB.running)).toBeVisible();

    // In the suite's 1280px window nothing of the table is scrolled out of its card. The table's
    // own wrapper is what scrolls when the columns do not fit.
    const overflow = await page.locator('[data-list-surface] table').evaluate((table) => {
      const scroller = table.parentElement!;
      return scroller.scrollWidth - scroller.clientWidth;
    });
    expect(overflow).toBeLessThanOrEqual(0);
    await expect(
      row(page, MOCK_SEARCH_JOB.running).getByRole('button', { name: /^Delete job/ }),
    ).toBeInViewport({ ratio: 1 });

    // No column wraps its name to a second line.
    const headHeights = await page
      .locator('[data-list-surface] thead th')
      .evaluateAll((heads) => heads.map((head) => head.getBoundingClientRect().height));
    expect(new Set(headHeights).size).toBe(1);
  });

  test('a state view lists the jobs of that state, and the address keeps it', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);

    await page.goto('/streams/jobs');
    await expect(view(page, 'All')).toHaveText(/6$/);
    await expect(view(page, 'Queued')).toHaveText(/1$/);
    await expect(view(page, 'Running')).toHaveText(/1$/);
    await expect(view(page, 'Done')).toHaveText(/2$/);
    await expect(view(page, 'Failed')).toHaveText(/1$/);
    await expect(view(page, 'Cancelled')).toHaveText(/1$/);

    await view(page, 'Done').click();

    await expect(page).toHaveURL(/\/streams\/jobs\?state=done$/);
    await expect(row(page, MOCK_SEARCH_JOB.running)).toHaveCount(0);
    expect(await listedIds(page)).toEqual([MOCK_SEARCH_JOB.done, MOCK_SEARCH_JOB.doneEmpty]);

    await page.reload();

    await expect(view(page, 'Done')).toHaveAttribute('aria-pressed', 'true');
    await expect(row(page, MOCK_SEARCH_JOB.done)).toBeVisible();
    expect(await listedIds(page)).toEqual([MOCK_SEARCH_JOB.done, MOCK_SEARCH_JOB.doneEmpty]);

    await view(page, 'All').click();

    await expect(page).toHaveURL(/\/streams\/jobs$/);
    await expect(row(page, MOCK_SEARCH_JOB.running)).toBeVisible();
  });

  test('a running job is cancelled only after the reader confirms, and then says so', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);

    await page.goto('/streams/jobs');
    const running = row(page, MOCK_SEARCH_JOB.running);
    await expect(running.getByText('Running', { exact: true })).toBeVisible();

    await running.getByRole('button', { name: /^Cancel job/ }).click();
    const dialog = page.getByRole('dialog');
    await expect(dialog.getByText('Cancel this job?')).toBeVisible();
    await dialog.getByRole('button', { name: 'Keep running' }).click();

    await expect(dialog).toHaveCount(0);
    await expect(running.getByText('Running', { exact: true })).toBeVisible();

    await running.getByRole('button', { name: /^Cancel job/ }).click();
    await dialog.getByRole('button', { name: 'Cancel job' }).click();

    await expect(dialog).toHaveCount(0);
    await expect(running.getByText('Cancelled', { exact: true })).toBeVisible();
    await expect(running.getByText('Running', { exact: true })).toHaveCount(0);
    // The counts follow, and what the row offers now is another run.
    await expect(view(page, 'Running')).toHaveText(/0$/);
    await expect(view(page, 'Cancelled')).toHaveText(/2$/);
    await expect(running.getByRole('button', { name: /^Retry job/ })).toBeVisible();
    await expect(running.getByRole('button', { name: /^Cancel job/ })).toHaveCount(0);
  });

  test('a failed job is queued again at once, and a deleted one is gone', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);

    await page.goto('/streams/jobs');
    const failed = row(page, MOCK_SEARCH_JOB.failed);
    await failed.getByRole('button', { name: /^Retry job/ }).click();

    await expect(failed.getByText('Queued', { exact: true })).toBeVisible();
    await expect(failed.getByText(/resource exhausted/)).toHaveCount(0);
    await expect(failed.getByTitle('Attempt 3')).toHaveText('3');
    await expect(page.getByRole('dialog')).toHaveCount(0);

    const done = row(page, MOCK_SEARCH_JOB.done);
    await done.getByRole('button', { name: /^Delete job/ }).click();
    const dialog = page.getByRole('dialog');
    await expect(
      dialog.getByText('The job and its stored results are deleted and cannot be restored.'),
    ).toBeVisible();
    await dialog.getByRole('button', { name: 'Delete', exact: true }).click();

    await expect(done).toHaveCount(0);
    await expect(view(page, 'All')).toHaveText(/5$/);
    await expect(view(page, 'Done')).toHaveText(/1$/);
  });

  test('the results of a finished job download as one NDJSON file', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);

    await page.goto('/streams/jobs');
    const done = row(page, MOCK_SEARCH_JOB.done);
    const downloading = page.waitForEvent('download');
    await done.getByRole('button', { name: /^Download the results/ }).click();
    const download = await downloading;

    expect(download.suggestedFilename()).toBe(`search-job-${MOCK_SEARCH_JOB.done}.ndjson`);
    const lines = (await readFile(await download.path(), 'utf8')).trimEnd().split('\n');
    // 12 480 rows are more than one request for results may carry: both pages are in the file.
    expect(lines).toHaveLength(12_480);
    expect(JSON.parse(lines[0]!)).toMatchObject({ row: 1 });
    expect(JSON.parse(lines.at(-1)!)).toMatchObject({ row: 12_480 });

    // A job that found nothing has nothing to download.
    await expect(
      row(page, MOCK_SEARCH_JOB.doneEmpty).getByRole('button', { name: /^Download the results/ }),
    ).toBeDisabled();
  });

  test('a job the worker finishes turns to done without a reload, and a hidden page is not read', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    const reads = watchListReads(page);

    await page.goto('/streams/jobs');
    const running = row(page, MOCK_SEARCH_JOB.running);
    await expect(running.getByText('Running', { exact: true })).toBeVisible();
    await expect(page.locator('[data-search-jobs-live]')).toHaveText(
      'Jobs are under way, refreshing every 3 s',
    );

    await updateMockSearchJob(mockServer.port, MOCK_SEARCH_JOB.running, {
      state: 'done',
      result_rows: 5,
      finished_at_micros: NOW_MICROS,
    });

    // Nothing is clicked: the page reads the list again by itself.
    await expect(running.getByText('Done', { exact: true })).toBeVisible({ timeout: 10_000 });
    await expect(running.getByRole('button', { name: /^Download the results/ })).toBeEnabled();
    await expect(view(page, 'Running')).toHaveText(/0$/);
    // The backfill still waits, so the page keeps reading.
    await expect(page.locator('[data-search-jobs-live]')).toBeVisible();

    await setPageHidden(page, true);
    // A read that was already on its way may still land.
    await page.waitForTimeout(500);
    const whileHidden = reads.count();
    await page.waitForTimeout(7_000);
    expect(reads.count()).toBe(whileHidden);

    await setPageHidden(page, false);
    await expect.poll(() => reads.count(), { timeout: 2_000 }).toBeGreaterThan(whileHidden);
  });

  test('with no job the page says what a job is and where one comes from', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    await setMockSearchJobs(mockServer.port, []);

    await page.goto('/streams/jobs');

    const state = page.locator('[data-product-state]');
    await expect(state.getByText('No search jobs yet')).toBeVisible();
    await expect(state.getByRole('listitem')).toHaveText([
      'A query sent with Prefer: respond-async, or one whose result is expected to be too large',
      'The backfill of a data pipeline',
      'A query submitted by Mole Agent',
    ]);
    await expect(page.getByRole('group', { name: 'Job state' })).toHaveCount(0);
    await expect(page.locator('[data-search-jobs-live]')).toHaveCount(0);
  });

  test('a reader who may not run a query is shown no way in, and is turned away at the door', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, {
      token: 'custom-role-stream-reader',
      role: 'Viewer',
      scope: 'organization',
      capabilityPermissions: ['streams.read'],
    });
    const reads = watchListReads(page);

    await page.goto('/streams');
    await expect(page.getByRole('heading', { level: 1, name: 'Streams' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Refresh' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Search jobs' })).toHaveCount(0);

    await page.goto('/streams/jobs');

    await expect(page.getByText('You cannot view search jobs')).toBeVisible();
    await expect(page.locator('[data-search-job]')).toHaveCount(0);
    expect(reads.count()).toBe(0);
  });

  test('the page reads in Chinese as it does in English', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });

    await page.goto('/streams/jobs');

    await expect(page.getByRole('heading', { level: 1, name: '搜索任务' })).toBeVisible();
    const running = row(page, MOCK_SEARCH_JOB.running);
    await expect(running.getByText('运行中', { exact: true })).toBeVisible();
    await expect(running.getByText('查询', { exact: true })).toBeVisible();
    await expect(running.getByText('尚未结束')).toBeVisible();
    await expect(
      row(page, MOCK_SEARCH_JOB.pendingBackfill).getByText('回填', { exact: true }),
    ).toBeVisible();
    await expect(page.locator('[data-search-jobs-live]')).toHaveText('有任务在执行，每 3 秒刷新');
  });
});
