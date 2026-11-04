import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM, mockRumData } from '../fixtures/mockRumData';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
const DATA = mockRumData(NOW);
const ADDRESS = `/rum/feature-flags?app=${MOCK_RUM.applications.web}&time=now-7d..now`;

test.describe('RUM feature flags', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('is a tab of the module, listing the flags sessions evaluated', async ({ page }) => {
    await page.goto(ADDRESS);

    await expect(page.locator('[data-rum-navigation="surface"] [aria-current="page"]').first()).toContainText('Feature flags');
    await expect(page.locator('[data-rum-flag="checkout_v2"]')).toBeVisible();
    await expect(page.locator('[data-rum-flag="new_nav"]')).toBeVisible();
  });

  test('compares how the sessions of each value did', async ({ page }) => {
    await page.goto(`${ADDRESS}&flag=checkout_v2`);

    const treated = DATA.views.filter(
      (row) =>
        row['application.id'] === MOCK_RUM.applications.web &&
        (row.feature_flags as Record<string, unknown> | undefined)?.checkout_v2 === 'treatment',
    );

    const row = page.locator('[data-rum-flag-value="treatment"]');

    await expect(row).toBeVisible();
    await expect(page.locator('[data-rum-flag-value="control"]')).toBeVisible();
    await expect(row.getByRole('cell').nth(1)).toHaveText(String(new Set(treated.map((view) => view['session.id'])).size));
    await expect(row.locator('[data-rum-flag-errors]')).toHaveClass(/red-ink/);
  });

  test('a value leads to its views in the explorer', async ({ page }) => {
    await page.goto(`${ADDRESS}&flag=checkout_v2`);
    await page.locator('[data-rum-flag-value="control"]').locator('[data-rum-flag-explore]').click();

    await expect(page).toHaveURL(/\/rum\/explorer/);
    await expect(page).toHaveURL(/feature_flags\.checkout_v2/);
    await expect(page.locator('[data-rum-query-rejected]')).toHaveCount(0);
  });

  test('an application that evaluated none says how to start', async ({ page }) => {
    await page.goto(`/rum/feature-flags?app=${MOCK_RUM.applications.app}&time=now-7d..now`);

    await expect(page.getByText('No feature flags in this range')).toBeVisible();
  });
});
