import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('NOC display contract', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('forces the wallboard into the dark token scope', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'light' });
    await page.goto('/noc');

    const wallboard = page.locator('[data-mode="noc"]');
    await expect(wallboard).toHaveAttribute('data-theme', 'dark');
    await expect(wallboard).toHaveAttribute('data-palette', 'default');
  });

  test('shows the desktop-width interstitial below 1024px', async ({ page }) => {
    await page.setViewportSize({ width: 768, height: 800 });
    await page.goto('/noc');

    await expect(
      page.getByRole('heading', { name: 'Molesignal is built for wide screens' }),
    ).toBeVisible();
    await expect(page.getByText('Current width 768px · minimum 1024px')).toBeVisible();
  });
});
