/**
 * 01 — investigation happy path
 *
 * Walks: open palette → search "web" → enter (pushes a service frame) →
 * modified copy-link shortcut to copy the investigation share URL → verify clipboard contents.
 *
 * Backend is fully mocked via mountMockRoutes; clipboard permission is
 * granted in `beforeEach` so the copy shortcut never races against the grant.
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('investigation happy path', () => {
  test.beforeEach(async ({ page, context, mockServer }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    await mountMockRoutes(page, mockServer.port);
  });

  test('palette → service → share shortcut', async ({ page }) => {
    await page.goto('/investigate');
    await page.keyboard.press('Meta+K');
    await expect(page.getByPlaceholder(/search commands/i)).toBeVisible();

    // The mock backend's /web/search returns a "web" service item.
    await page.keyboard.type('web');
    // cmdk's fuzzy filter happily includes static actions that share
    // letters with "web" and leaves auto-selection on the last-rendered
    // item, so click the service row directly rather than press Enter.
    const webService = page.locator('[cmdk-item][data-value="service:web:web"]');
    await webService.click();

    // Service frame mounts. Topology canvas should render the testid nodes.
    await expect(page.getByTestId(/^topology-node-/).first()).toBeVisible({ timeout: 10_000 });

    // Modified shortcut copies the shareable URL.
    await page.keyboard.press('Meta+Alt+Y');
    const copied = await page.evaluate(() => navigator.clipboard.readText());
    expect(copied).toContain('/investigate');
  });
});
