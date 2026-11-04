import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM } from '../fixtures/mockRumData';

const WEB = MOCK_RUM.applications.web;

const panel = (page: Page) => page.getByRole('complementary', { name: 'Mole Agent' });

/** What the person finds written in the input of the panel, never sent. */
const draft = (page: Page) => panel(page).locator('.ProseMirror');

/** The panel is opened on a new chat with the application attached, and the question is in the input. */
async function expectDrafted(page: Page, ...words: string[]) {
  await expect(panel(page)).toHaveAttribute('aria-hidden', 'false');

  for (const word of words) await expect(draft(page)).toContainText(word);

  await expect(panel(page).locator(`[data-agent-reference="rum_application:${WEB}"]`)).toBeVisible();
}

/** Opens a page and waits for the time range it puts in the address, as it does a moment after it mounts. */
async function open(page: Page, path: string) {
  await page.goto(path);
  await expect(page).toHaveURL(/[?&]time=/);
}

test.describe('Mole Agent from RUM', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('is offered in the head of the overview beside the dashboard, and drafts a question about the application', async ({ page }) => {
    await open(page, `/rum/overview?app=${WEB}&env=prod`);

    const head = page.locator('[data-testid="page-header"]');

    await expect(head.locator('[data-rum-dashboard-link]')).toHaveCount(1);
    await head.locator('[data-rum-investigate="overview"]').click();

    await expectDrafted(page, 'Storefront', 'prod', 'overview');
  });

  test('is in the head of the errors, the performance and the frustration tabs', async ({ page }) => {
    await open(page, `/rum/errors?app=${WEB}`);
    await page.locator('[data-testid="page-header"] [data-rum-investigate="errors"]').click();
    await expectDrafted(page, 'errors of Storefront');

    await open(page, `/rum/performance?app=${WEB}`);
    await expect(page.locator('[data-testid="page-header"] [data-rum-investigate="vital"]')).toHaveCount(1);

    await open(page, `/rum/frustration?app=${WEB}`);
    await page.locator('[data-testid="page-header"] [data-rum-investigate="frustration"]').click();
    await expectDrafted(page, 'frustrates the users of Storefront');
  });

  test('carries the vital in front on the performance tab', async ({ page }) => {
    await open(page, `/rum/performance?app=${WEB}&vital=inp`);
    await page.locator('[data-testid="page-header"] [data-rum-investigate="vital"]').click();

    await expectDrafted(page, 'INP of Storefront');
  });

  test('asks about the other tabs of performance as the performance of the pages', async ({ page }) => {
    await open(page, `/rum/performance/resources?app=${WEB}`);
    await page.locator('[data-testid="page-header"] [data-rum-investigate="performance"]').click();

    await expectDrafted(page, 'how fast the pages of Storefront');
  });

  test('is on a page of the performance table, drafting a question about the views of that page', async ({ page }) => {
    await open(page, `/rum/performance?app=${WEB}`);

    const row = page.locator('[data-rum-performance-table] [data-rum-row]').first();

    await row.hover();
    await row.locator('[data-rum-investigate="slice"]').click();

    await expectDrafted(page, 'whose Page is');
    // The row is not opened by it.
    await expect(page).not.toHaveURL(/[?&]open=/);
  });

  test('is on an issue, drafting a question about it', async ({ page }) => {
    await open(page, `/rum/errors?app=${WEB}`);
    await page.locator('[data-rum-issue]').first().click();

    const drawer = page.locator('[data-rum-issue-drawer]');

    await drawer.locator('[data-rum-investigate="issue"]').click();

    await expectDrafted(page, 'Investigate the issue', 'Storefront');
  });

  test('is on a group of the frustration table, drafting a question about the element', async ({ page }) => {
    await open(page, `/rum/frustration?app=${WEB}`);
    await page.locator('[data-rum-frustration-table] [data-rum-row]').first().click();

    await page.locator('[data-rum-panel="frustration"] [data-rum-investigate="element"]').click();

    await expectDrafted(page, 'Look into the element');
  });

  test('is in the head of a session, drafting a question about it', async ({ page }) => {
    await page.goto(`/rum/sessions/view/${MOCK_RUM.session.checkout}`);
    await page.locator('[data-testid="page-header"] [data-rum-investigate="session"]').click();

    await expectDrafted(page, `session ${MOCK_RUM.session.checkout}`);
  });
});
