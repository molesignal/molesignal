import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { setMockIntakeUsage } from '../fixtures/mockIntakeUsage';

const row = (page: Page, name: string) =>
  page.locator('tr', { has: page.getByText(name, { exact: true }) });
const view = (page: Page, name: string) =>
  page.getByRole('group', { name: 'Divide by' }).getByRole('button', { name });

test.describe('Intake usage', () => {
  test.beforeEach(async ({ mockServer }) => {
    await setMockIntakeUsage(mockServer.port, { seed_budgets: true });
  });

  test('is reached from the stream list, not from a first-level entry', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/streams');

    await page.getByRole('button', { name: 'Intake usage' }).click();

    await expect(page).toHaveURL(/\/streams\/usage$/);
    await expect(page.getByRole('heading', { name: 'Intake usage' })).toBeVisible();
    await expect(page.getByRole('navigation').getByRole('link', { name: 'Intake usage' })).toHaveCount(0);
  });

  test('shows what each credential took in and where its budget stands', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/streams/usage');

    const over = row(page, 'checkout-prod');
    await expect(over.getByText('20 GiB', { exact: true }).first()).toBeVisible();
    await expect(over.getByText('10 GiB / day')).toBeVisible();
    await expect(over.getByText('200%')).toBeVisible();
    await expect(over.getByText('Over budget')).toBeVisible();

    await expect(row(page, 'otel-collector').getByText('Nearly used')).toBeVisible();
    await expect(row(page, 'otel-collector').getByText('85%')).toBeVisible();
    await expect(row(page, 'Cloudflare edge').getByText('Within budget')).toBeVisible();

    // Sent nothing over a window metering covers: that is zero, and it has no budget.
    const idle = row(page, 'ci-pipeline');
    await expect(idle.getByText('0 B', { exact: true })).toBeVisible();
    await expect(idle.getByText('Not set')).toBeVisible();

    await expect(page.getByTestId('usage-status')).toContainText('90 GiB');
  });

  test('the three views add up to the same total', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/streams/usage');
    const status = page.getByTestId('usage-status');

    await expect(status).toContainText('90 GiB');

    await view(page, 'Stream').click();
    await expect(page).toHaveURL(/dimension=stream/);
    await expect(row(page, 'app-logs')).toBeVisible();
    await expect(status).toContainText('90 GiB');

    await view(page, 'Service').click();
    await expect(page).toHaveURL(/dimension=service/);
    await expect(row(page, 'checkout')).toBeVisible();
    await expect(row(page, 'No service named')).toBeVisible();
    await expect(row(page, 'Other services')).toBeVisible();
    await expect(status).toContainText('90 GiB');
  });

  test('a longer window reads more, and the address keeps it', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/streams/usage?window=7d&dimension=stream');

    await expect(page.getByTestId('usage-status')).toContainText('630 GiB');
    await expect(row(page, 'app-logs').getByText('350 GiB', { exact: true })).toBeVisible();
  });

  test('figures that cannot be read show as unknown, never as zero', async ({ page, mockServer }) => {
    await setMockIntakeUsage(mockServer.port, { available: false });
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/streams/usage');

    await expect(page.getByTestId('usage-status')).toContainText('Usage cannot be read right now');

    const over = row(page, 'checkout-prod');
    await expect(over.getByText('–', { exact: true }).first()).toBeVisible();
    await expect(over.getByText('10 GiB / day')).toBeVisible();
    await expect(over.getByText('200%')).toHaveCount(0);
    await expect(page.getByRole('table').getByText('0 B', { exact: true })).toHaveCount(0);
  });

  test('a window metering does not cover shows lower bounds and says since when', async ({ page, mockServer }) => {
    await setMockIntakeUsage(mockServer.port, { metering_days_ago: 3 });
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/streams/usage?window=30d');

    await expect(page.getByTestId('usage-status')).toContainText('Metered since');
    await expect(row(page, 'otel-collector').getByText('≥ 1.76 TiB')).toBeVisible();

    // Sent nothing, but metering began inside the window: that says nothing.
    await expect(row(page, 'ci-pipeline').getByText('–', { exact: true }).first()).toBeVisible();
    await expect(row(page, 'ci-pipeline').getByText('0 B', { exact: true })).toHaveCount(0);
  });

  test('sets a budget on a source, then changes and removes it', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/streams/usage');

    const idle = row(page, 'ci-pipeline');
    await idle.getByRole('button', { name: 'Set budget' }).click();

    const drawer = page.getByRole('dialog');
    await expect(drawer.getByText('ci-pipeline')).toBeVisible();
    await expect(drawer.getByRole('button', { name: 'Save budget' })).toBeDisabled();

    await drawer.getByTestId('budget-limit').fill('5');
    await drawer.getByRole('radio', { name: /Each month/ }).click();
    await drawer.getByRole('checkbox', { name: /Dana Reyes/ }).click();
    await drawer.getByRole('button', { name: 'Save budget' }).click();

    await expect(page.getByRole('dialog')).toHaveCount(0);
    await expect(idle.getByText('5 GiB / month')).toBeVisible();
    await expect(idle.getByText('Within budget')).toBeVisible();

    await idle.getByRole('button', { name: 'Change' }).click();
    await expect(drawer.getByTestId('budget-limit')).toHaveValue('5');
    await drawer.getByTestId('budget-limit').fill('1024');
    await drawer.getByRole('button', { name: 'Save budget' }).click();
    await expect(idle.getByText('1 TiB / month').or(idle.getByText('1,024 GiB / month'))).toBeVisible();

    await idle.getByRole('checkbox').click();
    await page.getByTestId('usage-bulk').getByRole('button', { name: 'Remove budget' }).click();
    await page.getByRole('dialog').getByRole('button', { name: 'Remove budget' }).click();
    await expect(idle.getByText('Not set')).toBeVisible();
  });

  test('sets one budget on several sources at once', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/streams/usage?dimension=stream');

    await row(page, 'app-logs').getByRole('checkbox').click();
    await row(page, 'host-metrics').getByRole('checkbox').click();
    await page.getByTestId('usage-bulk').getByRole('button', { name: 'Set budget' }).click();

    const drawer = page.getByRole('dialog');
    await expect(drawer.getByText('The same budget on 2 sources')).toBeVisible();
    await drawer.getByTestId('budget-limit').fill('2');
    await drawer.getByRole('checkbox', { name: /Dana Reyes/ }).click();
    await drawer.getByRole('button', { name: 'Save budget' }).click();

    await expect(row(page, 'app-logs').getByText('2 GiB / day')).toBeVisible();
    await expect(row(page, 'host-metrics').getByText('2 GiB / day')).toBeVisible();
    await expect(row(page, 'api-traces').getByText('Not set')).toBeVisible();
    await expect(page.getByTestId('usage-bulk')).toHaveCount(0);
  });

  test('the platform and what names no one cannot take a budget', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/streams/usage');

    const internal = row(page, 'Platform internal');
    await expect(internal.getByRole('button', { name: 'Set budget' })).toHaveCount(0);
    await expect(internal.getByRole('checkbox')).toHaveCount(0);

    await view(page, 'Service').click();
    await expect(row(page, 'No service named').getByRole('button', { name: 'Set budget' })).toHaveCount(0);
    await expect(row(page, 'Other services').getByRole('button', { name: 'Set budget' })).toHaveCount(0);
  });

  test('a row opens what the source is made of', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/streams/usage');

    await row(page, 'otel-collector').getByText('otel-collector', { exact: true }).click();

    const drawer = page.getByRole('dialog');
    await expect(drawer.getByTestId('usage-trend')).toBeVisible();
    await expect(drawer.getByTestId('usage-breakdown-stream').getByText('app-logs')).toBeVisible();
    await expect(drawer.getByTestId('usage-breakdown-service').getByText('checkout')).toBeVisible();
    await expect(drawer.getByTestId('usage-breakdown-credential')).toHaveCount(0);
    await expect(drawer.getByTestId('usage-detail-budget')).toContainText('70 GiB / day');
    await expect(drawer.getByTestId('usage-detail-budget')).toContainText('Nearly used');
  });

  test('exports what the list shows', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/streams/usage');
    await expect(row(page, 'checkout-prod')).toBeVisible();

    const download = page.waitForEvent('download');
    await page.getByRole('button', { name: /Export/ }).click();

    expect((await download).suggestedFilename()).toContain('molesignal-intake-usage-credential');
  });

  test('the bell rings for a budget past or near its limit', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/streams');

    await page.getByRole('button', { name: /^Notifications/ }).click();

    await expect(page.getByText('Intake budget exceeded: checkout-prod')).toBeVisible();
    await expect(page.getByText('Intake budget nearly used: otel-collector')).toBeVisible();
    await expect(page.getByText('Intake budget exceeded: Cloudflare edge')).toHaveCount(0);

    await page.getByText('Intake budget exceeded: checkout-prod').click();
    await expect(page).toHaveURL(/\/streams\/usage$/);
  });

  test('offers no way in to someone without the permission', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { role: 'Viewer' });
    await page.goto('/streams');

    await expect(page.getByRole('button', { name: 'Create stream' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Intake usage' })).toHaveCount(0);

    await page.goto('/streams/usage');
    await expect(page.getByTestId('access-denied')).toBeVisible();
  });
});

test.describe('Intake usage in the platform organization', () => {
  test('shows the figures, and nothing about budgets', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, {
      scope: 'system',
      platformPermissions: ['sys.telemetry.read'],
      capabilityPermissions: ['sys.telemetry.read'],
    });
    await page.goto('/streams/usage');

    await expect(row(page, 'otel-collector').getByText('60 GiB', { exact: true })).toBeVisible();
    await expect(page.getByRole('columnheader', { name: 'Budget', exact: true })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Set budget' })).toHaveCount(0);
    await expect(page.getByRole('checkbox')).toHaveCount(0);
  });
});

test.describe('Intake usage settings', () => {
  const days = (page: Page) => page.getByRole('spinbutton', { name: 'Keep records for' });
  const services = (page: Page) => page.getByRole('spinbutton', { name: 'Services per organization' });
  const save = (page: Page) => page.getByRole('button', { name: 'Save changes' });

  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, {
      scope: 'system',
      platformPermissions: ['sys.settings.limits.read', 'sys.settings.limits.manage'],
      capabilityPermissions: ['sys.settings.limits.read', 'sys.settings.limits.manage'],
    });
    await page.goto('/settings/intake_usage');
    await expect(days(page)).toHaveValue('90');
  });

  test('saves how long records are kept and how many services are counted apart', async ({ page }) => {
    await expect(services(page)).toHaveValue('1000');
    await expect(save(page)).toBeDisabled();

    await days(page).fill('120');
    await services(page).fill('500');
    await save(page).click();

    await expect(page.getByText('Intake usage settings saved')).toBeVisible();
    await page.reload();
    await expect(days(page)).toHaveValue('120');
    await expect(services(page)).toHaveValue('500');
  });

  test('holds back a retention too short to read back a month of budget', async ({ page }) => {
    await days(page).fill('7');

    await expect(save(page)).toBeDisabled();
    await expect(page.getByText(/Keep records for 35 to 3650 days/)).toBeVisible();
  });
});
