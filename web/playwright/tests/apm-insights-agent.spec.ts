import type { Page } from '@playwright/test';

import { type Captured, installApmRoutes, SERVICE_PATH } from '../fixtures/mockApm';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

async function boot(page: Page, port: number): Promise<Captured> {
  await mountMockRoutes(page, port);
  return installApmRoutes(page);
}

const insights = (page: Page) => page.locator('[data-apm-card="insights"]');

test('the overview says what moved against the hour before and the week before, and the changes near then', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/apm/overview');

  const card = insights(page);
  const moved = card.locator('[data-apm-insight="anomaly"]');
  await expect(moved).toContainText('checkout');
  await expect(moved).toContainText('Error rate rose to 1.94%');
  await expect(moved).toContainText('0.40% the stretch before, 0.30% the same stretch a week before');
  await expect(moved.locator('[data-apm-insight-changes]')).toContainText('Deployment of checkout 2.0.0 (9f2c1e7)');
  await expect(moved.locator('[data-apm-insight-changes]')).toContainText('Configuration change of payments');

  await expect(card.locator('[data-apm-baseline-line="last_week"]')).toContainText('error rate +1.6 pt');
  await expect(card.locator('[data-apm-insight="repeated_call"]')).toContainText('24 times a request');
  // The deployments card says which version did worse.
  await expect(card.locator('[data-apm-insight="regression"]')).toHaveCount(0);
  await expect(page.locator('[data-apm-card="overview-anomalies"]')).toHaveCount(0);
});

test('a finding on a service is handed to Mole Agent with the service attached', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto(SERVICE_PATH);

  const card = insights(page);
  await expect(card.locator('[data-apm-insight="regression"]')).toContainText('2.0.0 did worse than 1.0.0: P95 510 ms, was 220 ms');
  // Where the time goes, the downstream and endpoint cards of the page say.
  await expect(card.locator('[data-apm-insight="hot_endpoint"]')).toHaveCount(0);
  await expect(card.locator('[data-apm-insight="slow_dependency"]')).toHaveCount(0);

  await card.locator('[data-apm-insight="anomaly"] [data-apm-investigate]').click();

  const panel = page.getByRole('complementary', { name: 'Mole Agent' });
  await expect(panel).toHaveAttribute('aria-hidden', 'false');
  await expect(panel.locator('.ProseMirror')).toContainText('Investigate this finding about checkout in prod');
  await expect(panel.locator('.ProseMirror')).toContainText('Error rate rose to 1.94%');
});

test('a service and its endpoints and errors can be investigated from where they are read', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto(SERVICE_PATH);

  await page.locator('[data-apm-navigation="service"] [data-apm-investigate]').click();
  const panel = page.getByRole('complementary', { name: 'Mole Agent' });
  await expect(panel.locator('.ProseMirror')).toContainText('Investigate checkout in prod');
});

test('an incident on a service says whom it reaches and where it may come from', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/alerts/incidents/inc-2292');

  const radius = page.locator('[data-incident-blast-radius]');
  await expect(radius.locator('[data-blast-node="service:gateway"]')).toContainText('gateway');
  await expect(radius.locator('[data-blast-node="service:gateway"]')).toContainText('direct');
  // The failing callee first: where the trouble may come from.
  await expect(radius.locator('[data-blast-node]').nth(1)).toHaveAttribute('data-blast-node', 'service:payments');
  await expect(radius.locator('[data-blast-node="service:payments"]')).toContainText('Critical');
  await expect(radius.getByRole('link', { name: 'Open on the service map' })).toHaveAttribute('href', /\/apm\/map\?.*node=service%3Acheckout/);
});

test('an APM page opens an anomaly rule on its figure', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto(SERVICE_PATH);

  await page.locator('[data-apm-navigation="service"] [data-apm-new-alert]').click();
  await page.locator('[data-apm-new-alert-kind="anomaly"]').click();
  await expect(page).toHaveURL(/\/alerts\/anomaly\/add\?source=apm&namespace=shop&service=checkout&environment=prod&metric=error_rate$/);

  // New error groups are watched against thresholds, never against the days before.
  await page.getByRole('combobox', { name: 'Figure', exact: true }).click();
  await expect(page.getByRole('option', { name: 'Error rate' })).toBeVisible();
  await expect(page.getByRole('option', { name: 'New error groups' })).toHaveCount(0);
  await page.keyboard.press('Escape');

  await page.getByLabel('Rule name').fill('Checkout errors against last week');
  await page.getByRole('button', { name: 'Create rule' }).click();

  await expect.poll(() => captured.rules.length).toBe(1);
  expect(captured.rules[0]).toMatchObject({
    kind: 'anomaly',
    query: { stream: null, apm: { service: 'checkout', namespace: 'shop', environment: 'prod', metric: 'error_rate' } },
    labels: { 'service.name': 'checkout', 'deployment.environment': 'prod' },
  });
});
