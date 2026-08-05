import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('copy icon actions', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('keeps settings copy actions to one visible icon', async ({ page }) => {
    await page.goto('/settings/general');

    const workspaceSection = page
      .locator('[data-settings-section]')
      .filter({ hasText: 'Workspace information' })
      .first();
    const buttons = workspaceSection.getByRole('button', {
      name: 'Copy',
      exact: true,
    });
    await expect(buttons).toHaveCount(2);

    for (let index = 0; index < 2; index += 1) {
      const button = buttons.nth(index);
      await expect(button).toBeVisible();
      await expect(button).toHaveText('');
      await expect(button.locator('svg')).toHaveCount(1);
    }
  });

  test('keeps only the page divider above the billing section', async ({
    page,
  }) => {
    await page.goto('/settings/billing');

    const stripeSection = page
      .getByText('Stripe', { exact: true })
      .locator('xpath=ancestor::section[1]');
    await expect(stripeSection).toBeVisible();
    await expect(stripeSection).toHaveCSS('border-top-width', '0px');
  });
});
