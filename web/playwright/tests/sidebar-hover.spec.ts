import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('Primary sidebar hover preview', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('starts collapsed, previews on hover, and preserves the toggle state', async ({
    page,
  }) => {
    await page.goto('/agent/chat');

    const sidebar = page.getByTestId('primary-sidebar');
    const main = page.locator('#main');
    const toggle = page.getByTestId('sidebar-toggle');

    await expect(sidebar).toHaveCSS('width', '64px');
    await expect(main).toHaveCSS('padding-left', '64px');

    await page.mouse.move(32, 132);
    await expect(sidebar).toHaveCSS('width', '240px');
    await expect(main).toHaveCSS('padding-left', '64px');

    await page.mouse.move(600, 26);
    await expect(sidebar).toHaveCSS('width', '64px');

    await toggle.click();
    await expect(sidebar).toHaveCSS('width', '240px');
    await expect(main).toHaveCSS('padding-left', '240px');

    await page.mouse.move(600, 26);
    await expect(sidebar).toHaveCSS('width', '240px');

    await toggle.click();
    await expect(sidebar).toHaveCSS('width', '64px');
    await expect(main).toHaveCSS('padding-left', '64px');
  });
});
