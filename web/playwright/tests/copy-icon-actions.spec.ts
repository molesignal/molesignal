import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('copy icon actions', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('keeps settings copy actions to one visible icon', async ({ page }) => {
    await page.goto('/settings/general');

    const identitySection = page
      .locator('[data-settings-section]')
      .filter({ hasText: 'System identity' })
      .first();
    const buttons = identitySection.getByRole('button', {
      name: 'Copy',
      exact: true,
    });
    await expect(identitySection).toContainText('Basic information');
    await expect(buttons).toHaveCount(2);

    for (let index = 0; index < 2; index += 1) {
      const button = buttons.nth(index);
      await expect(button).toBeVisible();
      await expect(button).toHaveText('');
      await expect(button.locator('svg')).toHaveCount(1);
    }

    const accessSection = page
      .locator('[data-settings-section]')
      .filter({ hasText: 'Access control' });
    await expect(accessSection).toContainText('Registration and access');
    await expect(accessSection).toContainText('Sharing and public access');
    await expect(page.locator('[data-settings-section]')).toHaveCount(4);
  });


});
