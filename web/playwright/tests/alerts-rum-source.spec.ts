import type { Page } from '@playwright/test';

import { installRumAlertRoutes, type RumAlertsCaptured } from '../fixtures/mockRumAlerts';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

async function boot(page: Page, port: number): Promise<RumAlertsCaptured> {
  await mountMockRoutes(page, port);

  return installRumAlertRoutes(page);
}

const COMBOBOX = (page: Page, name: string) => page.locator('[data-alert-rum-source]').getByRole('combobox', { name, exact: true });

test('a RUM page opens the rule editor on its figure, which previews and is saved with its labels', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);

  await page.goto('/alerts/rules/new?source=rum&app=storefront&environment=prod&page=%2Fcheckout&metric=lcp_p75');

  // The RUM source is chosen and filled in as the page said.
  await expect(page.locator('[data-alert-source="rum"]')).toHaveClass(/bg-indigo-dim/);
  await expect(COMBOBOX(page, 'Application')).toContainText('Storefront');
  await expect(COMBOBOX(page, 'Environment')).toContainText('prod');
  await expect(COMBOBOX(page, 'Page')).toContainText('/checkout');
  await expect(COMBOBOX(page, 'Figure')).toContainText('LCP (p75)');
  // A percentile is watched over an hour, and needs fifty views that had a value.
  await expect(COMBOBOX(page, 'Window')).toContainText('1 hour');
  await expect(page.locator('[data-alert-rum-source]')).toContainText('Default 50');

  await page.getByLabel('Rule name').fill('Storefront checkout is slow');
  // How far back the preview reaches is the server's choice, so nothing is promised before it ran.
  await expect(page.locator('[data-alert-query-preview]')).not.toContainText('last 6 hours');
  await page.getByRole('button', { name: 'Test rule' }).click();

  // The preview is the server's, for the rule's window.
  await expect.poll(() => captured.previews.length).toBe(1);
  expect(captured.previews[0]).toMatchObject({
    periodSecs: 3_600,
    query: { applicationId: 'storefront', environment: 'prod', page: '/checkout', metric: 'lcp_p75' },
  });
  await expect(page.locator('[data-alert-query-preview]')).toContainText('Current value');
  // Twelve windows of an hour.
  await expect(page.locator('[data-alert-query-preview]')).toContainText('last 12 hours');

  await page.getByRole('button', { name: 'Create rule' }).click();

  await expect.poll(() => captured.rules.length).toBe(1);
  expect(captured.rules[0]).toMatchObject({
    name: 'Storefront checkout is slow',
    kind: 'scheduled',
    query: {
      stream: null,
      periodSecs: 3_600,
      rum: { applicationId: 'storefront', environment: 'prod', page: '/checkout', metric: 'lcp_p75' },
    },
    labels: { 'rum.application': 'storefront', 'deployment.environment': 'prod', 'rum.page': '/checkout' },
    thresholds: [{ operator: 'gt', threshold: 2_500 }],
  });
});

test('a figure that cannot be narrowed to a page or a version says why, and another figure starts at its own line', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);

  await page.goto('/alerts/rules/new');
  await page.locator('[data-alert-source="rum"]').click();
  await COMBOBOX(page, 'Application').click();
  await page.getByRole('option', { name: 'Storefront', exact: true }).click();

  await COMBOBOX(page, 'Figure').click();
  await page.getByRole('option', { name: 'Crash-free sessions' }).click();

  // A session is not on one page: the page is out of reach, the version is not.
  await expect(COMBOBOX(page, 'Page')).toBeDisabled();
  await expect(page.locator('[data-alert-rum-source]')).toContainText('A session is not on one page.');
  await expect(COMBOBOX(page, 'Version')).toBeEnabled();
  // Below its line, not above it, and over the hour that a share of sessions needs.
  await expect(page.getByRole('combobox', { name: 'When result' })).toContainText('<');
  await expect(page.getByRole('spinbutton', { name: 'Threshold', exact: true })).toHaveValue('99.5');
  await expect(COMBOBOX(page, 'Window')).toContainText('1 hour');

  await COMBOBOX(page, 'Figure').click();
  await page.getByRole('option', { name: 'New issues' }).click();

  await expect(COMBOBOX(page, 'Page')).toBeDisabled();
  await expect(COMBOBOX(page, 'Version')).toBeDisabled();
  await expect(page.locator('[data-alert-rum-source]')).toContainText('An issue turns up in many versions.');
  // A count needs no minimum of rows.
  await expect(page.locator('[data-alert-rum-source]')).not.toContainText('Rows needed');
  await expect(page.getByRole('spinbutton', { name: 'Threshold', exact: true })).toHaveValue('0');
});

test('an app is told by its screens, and an application is needed before the rule is tested', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);

  await page.goto('/alerts/rules/new?source=rum&app=storefront-app&metric=error_rate');

  await expect(COMBOBOX(page, 'Screen')).toBeVisible();
  await expect(COMBOBOX(page, 'Page')).toHaveCount(0);

  await COMBOBOX(page, 'Application').click();
  await page.getByRole('option', { name: 'Storefront', exact: true }).click();
  await expect(COMBOBOX(page, 'Page')).toBeVisible();

  // Without an application there is nothing to read.
  await page.goto('/alerts/rules/new?source=rum&metric=error_rate');
  await page.getByRole('button', { name: 'Test rule' }).click();
  await expect(page.getByText('Choose the application to alert on').first()).toBeVisible();
  expect(captured.previews).toHaveLength(0);
});

test('the anomaly editor offers RUM figures against the days before, without new or regressed issues', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);

  await page.goto('/alerts/anomaly/add?source=rum&app=storefront&metric=new_issues');

  // New issues are watched against thresholds, never against the days before.
  await expect(COMBOBOX(page, 'Figure')).toContainText('Error rate');
  await COMBOBOX(page, 'Figure').click();
  await expect(page.getByRole('option', { name: 'New issues' })).toHaveCount(0);
  await expect(page.getByRole('option', { name: 'LCP (p75)' })).toBeVisible();
  await page.keyboard.press('Escape');

  await page.getByLabel('Rule name').fill('Storefront errors against last week');
  await page.getByRole('button', { name: 'Create rule' }).click();

  await expect.poll(() => captured.rules.length).toBe(1);
  expect(captured.rules[0]).toMatchObject({
    kind: 'anomaly',
    query: { stream: null, rum: { applicationId: 'storefront', metric: 'error_rate' } },
    labels: { 'rum.application': 'storefront' },
  });
});

test('a rule watches what the application measures itself, by its name and a statistic', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);

  await page.goto('/alerts/rules/new?source=rum&app=storefront&metric=custom_vital');

  await expect(COMBOBOX(page, 'Figure')).toContainText('Custom duration (vital)');

  await page.locator('[data-alert-rum-source]').getByRole('combobox', { name: 'Name of the vital' }).click();
  await page.keyboard.type('checkout_ready');
  await page.keyboard.press('Enter');
  await expect(page.locator('[data-alert-rum-source]').getByRole('combobox', { name: 'Name of the vital' })).toContainText('checkout_ready');

  await COMBOBOX(page, 'Statistic').click();
  await page.getByRole('option', { name: 'P95' }).click();

  await page.getByLabel('Rule name').fill('Checkout is slow to be ready');
  await page.getByRole('button', { name: 'Test rule' }).click();
  await expect.poll(() => captured.previews.length).toBe(1);
  expect(captured.previews[0]).toMatchObject({ query: { metric: 'custom_vital', customName: 'checkout_ready', statistic: 'p95' } });
  await page.getByRole('button', { name: 'Create rule' }).click();

  await expect.poll(() => captured.rules.length).toBe(1);

  const rule = captured.rules[0] as { query: { rum: Record<string, unknown> } };

  expect(rule.query.rum).toMatchObject({ applicationId: 'storefront', metric: 'custom_vital', customName: 'checkout_ready', statistic: 'p95' });
});

test('a rule watches the sessions the worst issue touched', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);

  await page.goto('/alerts/rules/new?source=rum&app=storefront&metric=issue_impact');

  await expect(COMBOBOX(page, 'Figure')).toContainText('Sessions hit by the worst issue');
  // A page narrows it: the errors of an issue are on pages.
  await expect(COMBOBOX(page, 'Page')).toBeVisible();

  await page.getByLabel('Rule name').fill('An issue hits many sessions');
  await page.getByRole('button', { name: 'Test rule' }).click();
  await expect.poll(() => captured.previews.length).toBe(1);
  await page.getByRole('button', { name: 'Create rule' }).click();

  await expect.poll(() => captured.rules.length).toBe(1);

  const rule = captured.rules[0] as { query: { rum: Record<string, unknown> } };

  expect(rule.query.rum).toMatchObject({ applicationId: 'storefront', metric: 'issue_impact' });
});
