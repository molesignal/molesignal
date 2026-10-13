import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { expectExplorerResults, serveExplorerResults } from '../fixtures/mockExplorers';

/** What the stream of the fixture opens with: the fields a reader looks for first, five with the time. */
const DEFAULT = ['_timestamp', 'level', 'service.name', 'message', 'trace_id'];

const columns = (page: Page) => page.locator('[data-log-result-columns="logs"] > *');

async function openLogs(page: Page) {
  await page.goto('/logs');
  await expectExplorerResults(page, 'logs');
}

test.describe('The columns a reader arranges for a log stream', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await serveExplorerResults(page);
  });

  test('are kept for the stream on the server, and are there after a reload', async ({ page }) => {
    const kept: { url: string; body: unknown }[] = [];
    page.on('request', (request) => {
      if (request.method() === 'PUT' && request.url().includes('/api/v1/me/ui-preference/')) {
        kept.push({ url: new URL(request.url()).pathname, body: request.postDataJSON() });
      }
    });

    await openLogs(page);
    await expect(columns(page)).toHaveText(DEFAULT);

    // Hide one column, show another, and put the message first.
    await page.getByRole('button', { name: 'Columns' }).click();
    await page.getByRole('menuitemcheckbox', { name: /^Reorder trace_id\./ }).click();
    await page.getByRole('menuitemcheckbox', { name: 'http.status_code', exact: true }).click();
    await page
      .getByRole('menuitemcheckbox', { name: /^Reorder message\./ })
      .dragTo(page.getByRole('menuitemcheckbox', { name: /^Reorder level\./ }), { targetPosition: { x: 24, y: 2 } });
    const arranged = ['_timestamp', 'message', 'level', 'service.name', 'http.status_code'];
    await expect(columns(page)).toHaveText(arranged);

    // Kept under the stream's name, as the fields beside the time, in their order.
    await expect.poll(() => kept.at(-1)).toEqual({
      url: '/api/v1/me/ui-preference/logs.columns/application_logs',
      body: { value: { fields: arranged.slice(1) } },
    });

    await page.keyboard.press('Escape');
    await page.reload();
    await expectExplorerResults(page, 'logs');
    await expect(columns(page)).toHaveText(arranged);
  });

  test('are there in another browser, which has nothing of its own to go by', async ({ page, browser, mockServer }) => {
    await openLogs(page);
    await page.getByRole('button', { name: 'Columns' }).click();
    await page.getByRole('menuitemcheckbox', { name: /^Reorder trace_id\./ }).click();
    await expect(columns(page)).toHaveText(DEFAULT.slice(0, 4));
    const sent = page.waitForResponse(
      (response) => response.request().method() === 'PUT' && response.url().includes('/me/ui-preference/logs.columns/'),
    );
    await sent;

    const elsewhere = await browser.newContext();
    const other = await elsewhere.newPage();
    await mountMockRoutes(other, mockServer.port);
    await serveExplorerResults(other);
    await openLogs(other);
    await expect(columns(other)).toHaveText(DEFAULT.slice(0, 4));
    await elsewhere.close();
  });

  test('go back to what the stream opens with on reset, which is offered only for an arrangement', async ({ page }) => {
    await openLogs(page);
    await page.getByRole('button', { name: 'Columns' }).click();
    const reset = page.locator('[data-log-columns-reset]');
    // The default has nothing to go back to.
    await expect(reset).toHaveCount(0);

    await page.getByRole('menuitemcheckbox', { name: /^Reorder level\./ }).click();
    await expect(columns(page)).toHaveText(['_timestamp', 'service.name', 'message', 'trace_id']);
    await expect(reset).toHaveText('Reset to default');

    const forgotten = page.waitForResponse(
      (response) => response.request().method() === 'DELETE' && response.url().includes('/me/ui-preference/logs.columns/'),
    );
    await reset.click();
    await forgotten;
    await expect(columns(page)).toHaveText(DEFAULT);

    await page.reload();
    await expectExplorerResults(page, 'logs');
    await expect(columns(page)).toHaveText(DEFAULT);
    await page.getByRole('button', { name: 'Columns' }).click();
    await expect(reset).toHaveCount(0);
  });
});
