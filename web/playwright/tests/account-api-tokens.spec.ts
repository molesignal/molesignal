import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('personal API tokens', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/account/settings/api-tokens');
  });

  test('issues a token that follows a policy, shows its secret once, then lists it', async ({
    page,
  }) => {
    await expect(page.getByRole('heading', { name: 'API tokens', level: 2 })).toBeVisible();
    await expect(page.getByText('No API tokens yet')).toBeVisible();

    await page.getByRole('button', { name: 'New token' }).click();
    const drawer = page.getByRole('dialog', { name: 'New personal token' });
    await drawer.getByRole('textbox', { name: 'Name' }).fill('collector-production');

    // Same as me is where a token starts; a policy narrows it.
    await expect(drawer.getByRole('radio', { name: /^Same as me/ })).toBeChecked();
    await drawer.getByRole('radio', { name: /^Permission policy/ }).click();
    await drawer.getByRole('combobox').filter({ hasText: 'Choose a policy' }).click();
    await page.getByRole('option', { name: 'Data intake', exact: true }).click();
    // What the policy allows is read before the token is issued.
    await expect(drawer.locator('[data-permission-list] li')).toHaveCount(5);
    await drawer.getByRole('button', { name: 'Create token' }).click();

    await expect(drawer.getByText('Token created')).toBeVisible();
    await expect(drawer.getByText('mspat-0123456789abcdefghjkmnpqrstvwxyz0123')).toBeVisible();
    await drawer.getByRole('button', { name: 'Done' }).click();

    const row = page.getByRole('row').filter({ hasText: 'collector-production' });
    await expect(row).toContainText('mspat-0123456789…');
    await expect(row).toContainText('Data intake');
    await expect(row).toContainText('Personal API token');
  });

  test('finds a permission by search and issues a token with just that one', async ({ page }) => {
    await page.getByRole('button', { name: 'New token' }).click();
    const drawer = page.getByRole('dialog', { name: 'New personal token' });
    await drawer.getByRole('textbox', { name: 'Name' }).fill('dashboard-reader');
    await drawer.getByRole('radio', { name: /^Custom/ }).click();

    // One line per permission, and the search keeps only what matches.
    const picker = drawer.locator('[data-permission-picker]');
    const rows = picker.getByRole('checkbox');
    expect(await rows.count()).toBeGreaterThan(5);
    await picker.getByRole('searchbox').fill('dashboards.read');
    await expect(rows).toHaveCount(1);
    const row = picker.locator('label:has(input[type="checkbox"])');
    expect((await row.boundingBox())?.height).toBeLessThanOrEqual(36);

    await rows.first().check();
    await picker.getByRole('searchbox').fill('no permission is called this');
    await expect(picker.getByText('No permission matches')).toBeVisible();
    await drawer.getByRole('button', { name: 'Create token' }).click();
    await drawer.getByRole('button', { name: 'Done' }).click();

    await expect(page.getByRole('row').filter({ hasText: 'dashboard-reader' })).toBeVisible();
  });

  test('is reached from the account menu of settings', async ({ page }) => {
    await page.getByRole('link', { name: 'API tokens', exact: true }).click();
    await expect(page).toHaveURL(/\/account\/settings\/api-tokens$/);
  });
});
