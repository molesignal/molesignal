import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.beforeEach(async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
});

test('mobile search and navigation remain keyboard accessible', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/metrics');
  const search = page.getByTestId('command-palette-trigger');
  await expect(search.locator('span')).toBeHidden();
  const searchSize = await search.boundingBox();
  expect(searchSize?.width).toBe(searchSize?.height);
  await search.click();
  await expect(page.getByRole('dialog')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(search).toBeFocused();

  const toggle = page.getByTestId('sidebar-toggle');
  await toggle.click();
  const drawer = page.getByRole('dialog');
  await expect(drawer).toBeVisible();
  await page.keyboard.press('Shift+Tab');
  await expect(drawer).toContainText('Metrics');
  await expect(drawer.locator(':focus')).toHaveCount(1);
  await page.keyboard.press('Escape');
  await expect(drawer).toHaveCount(0);
  await expect(toggle).toBeFocused();
  await toggle.click();
  await drawer.getByRole('link', { name: 'Logs', exact: true }).click();
  await expect(page).toHaveURL(/\/logs/);
  await expect(drawer).toHaveCount(0);
});

test('tablet renders the desktop rail with a short transition', async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 800 });
  await page.goto('/metrics');
  const sidebar = page.getByTestId('primary-sidebar');
  await expect(sidebar).toBeVisible();
  await expect(sidebar).toHaveCSS('transition-duration', '0.15s');
  await expect(page.locator('#main')).toHaveCSS('padding-left', '64px');
});
