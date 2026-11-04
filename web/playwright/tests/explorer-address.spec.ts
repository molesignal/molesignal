import type { Page, Request } from '@playwright/test';

import { explorerReadBody, isExplorerRead } from '../fixtures/explorerReads';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { expectExplorerResults, serveExplorerResults } from '../fixtures/mockExplorers';

const front = (page: Page) => page.locator('[data-query-tab-panel][data-active="true"]');

/** Opens the editor of the explorer in front, writes a query in it and runs it. */
async function runQuery(page: Page, statement: string) {
  await front(page).getByRole('button', { name: 'Expand query' }).click();
  await front(page).locator('[data-query-editor-state="expanded"] .monaco-editor').first().click();
  await page.keyboard.type(statement, { delay: 4 });
  await front(page).locator('[data-query-toolbar] [data-query-run]').click();
}

const searchParam = (page: Page, name: string) => () => new URL(page.url()).searchParams.get(name);

test.describe('The address of an explorer is the query on screen', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await mountMockRoutes(page, mockServer.port);
    await serveExplorerResults(page);
  });

  test('Logs: a query that runs is written to the address, read with GET, and the address opens it for someone else', async ({
    page,
    browser,
    mockServer,
  }) => {
    const reads: Request[] = [];
    page.on('request', (request) => {
      if (isExplorerRead(request, 'web/logs')) reads.push(request);
    });

    await page.goto('/logs');
    await expectExplorerResults(page, 'logs');
    // The stream the page picked is said before any query is.
    await expect.poll(searchParam(page, 'stream')).not.toBeNull();

    await runQuery(page, "level = 'ERROR'");
    await expect.poll(searchParam(page, 'q')).toBe("level = 'ERROR'");
    expect(reads.length).toBeGreaterThan(1);
    expect(reads.map((request) => request.method())).toEqual(reads.map(() => 'GET'));

    // Copied from the address bar into another browser: the same query, run as it opens.
    const address = page.url();
    const other = await browser.newContext({
      baseURL: 'http://localhost:5173',
      locale: 'en-US',
      timezoneId: 'UTC',
      viewport: { width: 1440, height: 900 },
    });
    const theirs = await other.newPage();
    await mountMockRoutes(theirs, mockServer.port);
    await serveExplorerResults(theirs);
    const asked = theirs.waitForRequest((request) => isExplorerRead(request, 'web/logs'));

    await theirs.goto(address);

    await expectExplorerResults(theirs, 'logs');
    expect(explorerReadBody<{ filters: unknown[] }>(await asked).filters).toEqual([
      expect.objectContaining({ field: 'level', op: '=', value: 'ERROR' }),
    ]);
    await expect(front(theirs).locator('.monaco-editor .view-lines').first()).toHaveText("level = 'ERROR'");
    await other.close();
  });

  test('Traces: the query applied is written to the address and comes back on a reload, the editor as it was left', async ({
    page,
  }) => {
    await page.goto('/traces');
    await expectExplorerResults(page, 'traces');

    await runQuery(page, "service.name = 'checkout'");
    await expect.poll(searchParam(page, 'q')).toBe("service.name = 'checkout'");
    await expect(front(page).locator('[data-query-editor-state="collapsed"]')).toBeVisible();
    // The tab keeps what it was left as a moment after it changes.
    await expect
      .poll(() => page.evaluate(() => localStorage.getItem('molesignal.query-tabs.v1') ?? ''))
      .toContain('"traces.editorFolded":true');

    const listed = page.waitForRequest((request) => /\/api\/v1\/web\/traces\?/.test(request.url()));
    await page.reload();

    expect(decodeURIComponent((await listed).url())).toContain("service.name");
    await expectExplorerResults(page, 'traces');
    // The address carries the query that ran, which is no reason to open the editor on a reload.
    await expect(front(page).locator('[data-query-editor-state="collapsed"]')).toBeVisible();
    await expect(front(page).locator('[data-query-editor-state="collapsed"]')).toContainText("service.name = 'checkout'");
  });

  test('Metrics: the statement of an address runs with GET, its range in the address of the read', async ({ page }) => {
    const ran = page.waitForRequest((request) => isExplorerRead(request, 'query'));

    await page.goto(`/metrics?promql=${encodeURIComponent('sum(rate(http_requests_total[5m]))')}`);

    const request = await ran;
    expect(request.method()).toBe('GET');
    expect(explorerReadBody(request)).toMatchObject({
      language: 'promql',
      statement: 'sum(rate(http_requests_total[5m]))',
      timeRange: { start: expect.any(Number), end: expect.any(Number) },
    });
    await expect.poll(searchParam(page, 'promql')).toBe('sum(rate(http_requests_total[5m]))');
  });
});
