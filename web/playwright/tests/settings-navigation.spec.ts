import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('settings navigation', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.addInitScript(() => {
      if (window.sessionStorage.getItem('settings_navigation_test_ready')) {
        return;
      }
      window.localStorage.removeItem('settings_sidebar_collapsed');
      window.sessionStorage.setItem('settings_navigation_test_ready', 'true');
    });
  });

  test('fully hides the desktop navigation and persists the choice', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/settings/general');

    await page
      .getByRole('button', { name: 'Collapse settings navigation' })
      .click();

    const content = page.locator('[data-management-content]');
    await expect(content).toHaveAttribute('data-sections-collapsed', 'true');
    await expect(
      page.getByRole('button', { name: 'Expand settings navigation' }),
    ).toBeVisible();
    await expect(page.locator('nav[aria-label="Settings"]')).toBeHidden();
    expect(
      await content.evaluate((element) =>
        getComputedStyle(element.parentElement!).gridTemplateColumns
          .trim()
          .split(/\s+/),
      ),
    ).toHaveLength(1);
    expect(
      await page.evaluate(() =>
        window.localStorage.getItem('settings_sidebar_collapsed'),
      ),
    ).toBe('true');

    await page.reload();
    await expect(
      page.getByRole('button', { name: 'Expand settings navigation' }),
    ).toBeVisible();

    await page
      .getByRole('button', { name: 'Expand settings navigation' })
      .click();
    await expect(page.locator('nav[aria-label="Settings"]')).toBeVisible();
    expect(
      await page.evaluate(() =>
        window.localStorage.getItem('settings_sidebar_collapsed'),
      ),
    ).toBe('false');
  });

  test('opens the settings navigation as a drawer below desktop width', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 900, height: 900 });
    await page.goto('/settings/general');

    const openButton = page.getByRole('button', {
      name: 'Open settings navigation',
    });
    await expect(openButton).toBeVisible();
    await expect(page.locator('nav[aria-label="Settings"]')).toBeHidden();

    await openButton.click();

    const drawer = page.getByRole('dialog');
    await expect(drawer).toBeVisible();
    await expect(
      drawer.getByRole('navigation', { name: 'Settings' }),
    ).toBeVisible();
    await drawer.getByRole('link', { name: 'General' }).click();
    await expect(drawer).toBeHidden();
  });

  test('keeps the narrow-screen exception scoped to management routes', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 900, height: 900 });
    await page.goto('/logs');

    await expect(
      page.getByText('Molesignal is built for wide screens'),
    ).toBeVisible();
  });
});
