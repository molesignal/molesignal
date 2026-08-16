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

  test('navigates on the first click while the rail expands on hover', async ({
    page,
  }) => {
    await page.goto('/agent/chat');
    await expect(page.getByTestId('primary-sidebar')).toHaveCSS('width', '64px');

    await page.getByRole('link', { name: 'Metrics', exact: true }).click();

    await expect(page).toHaveURL(/\/metrics(?:\?|$)/);
  });

  test('clears rail tooltips when the pointer leaves the sidebar', async ({
    page,
  }) => {
    await page.goto('/agent/chat');
    const sidebar = page.getByTestId('primary-sidebar');

    await expect(sidebar).toHaveCSS('width', '64px');
    for (const name of ['Logs', 'Traces', 'APM']) {
      const box = await page
        .getByRole('link', { name, exact: true })
        .boundingBox();
      expect(box).not.toBeNull();
      await page.mouse.move(
        box!.x + Math.min(24, box!.width / 2),
        box!.y + box!.height / 2,
      );
      await page.waitForTimeout(250);
    }
    await page.mouse.move(600, 400);

    await expect(sidebar).toHaveCSS('width', '64px');
    await page.waitForTimeout(300);
    await expect(page.getByRole('tooltip')).toHaveCount(0);
  });
});
