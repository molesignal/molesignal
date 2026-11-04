import AxeBuilder from '@axe-core/playwright';
import type { Page } from '@playwright/test';

import { type Captured, installApmRoutes, SERVICE_PATH } from '../fixtures/mockApm';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

async function boot(page: Page, port: number, theme: 'light' | 'dark' = 'light'): Promise<Captured> {
  await mountMockRoutes(page, port, { theme });
  return installApmRoutes(page);
}

test('SLOs are listed with what they met, and one opens with its burn rates and burn-rate alerts', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto('/apm/slos');

  await expect(page.locator('[data-apm-slo-summary]')).toContainText('1 SLO');
  const row = page.locator('[data-apm-slo-row="slo-1"]');
  await expect(row).toContainText('Checkout availability');
  await expect(row).toContainText('99.93%');
  await expect(row.locator('[data-apm-slo-state]')).toHaveText('At risk');

  await row.click();
  const drawer = page.locator('[data-apm-drawer="slo"]');
  await expect(drawer.locator('[data-apm-slo-burn="one_hour"]')).toContainText('16×');
  await expect(drawer.locator('[data-apm-slo-facts]')).toContainText('30%');

  // The two burn-rate rules of the SRE workbook, both on the objective.
  const alerts = drawer.locator('[data-apm-card="slo-alerts"]');
  await expect(alerts).toContainText('No alert watches this SLO yet.');
  await alerts.locator('[data-apm-slo-alerts]').click();
  await expect.poll(() => captured.rules.length).toBe(2);
  expect(captured.rules[0]).toMatchObject({
    kind: 'scheduled',
    query: { periodSecs: 3_600, stream: null, apm: { metric: 'slo_burn_rate', sloId: 'slo-1', shortWindowSecs: 300, service: 'checkout' } },
    thresholds: [{ severity: 'critical', operator: 'gt', threshold: 14.4 }],
    labels: { 'service.name': 'checkout', 'apm.slo': 'slo-1' },
  });
  expect(captured.rules[1]).toMatchObject({ query: { periodSecs: 21_600, apm: { shortWindowSecs: 1_800 } }, thresholds: [{ severity: 'warning', threshold: 6 }] });

  // Once set, the rules are listed instead of the button that sets them.
  await expect(alerts.locator('[data-apm-slo-rule="rule-1"]')).toContainText('Past 14.4× over 1 hour');
  await expect(alerts.locator('[data-apm-slo-rule="rule-2"]')).toContainText('Past 6× over 6 hours');
  await expect(alerts.locator('[data-apm-slo-alerts]')).toHaveCount(0);

  // Deleting the objective deletes the alerts that watch it first.
  await drawer.getByRole('button', { name: 'Delete SLO' }).click();
  const confirm = page.getByRole('dialog', { name: 'Delete SLO Checkout availability?' });
  await expect(confirm).toContainText('Its 2 burn-rate alerts are deleted with it');
  await confirm.getByRole('button', { name: 'Delete SLO' }).click();
  await expect.poll(() => captured.removed).toEqual(['rule:rule-1', 'rule:rule-2', 'slo:slo-1']);
  await expect(drawer).toBeHidden();
});

test('an SLO is set on a service from its form', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto('/apm/slos');

  await page.locator('[data-apm-slo-new]').click();
  const form = page.locator('[data-apm-slo-form]');
  await form.locator('[data-apm-slo-name]').fill('Checkout availability');
  await form.getByRole('combobox', { name: 'Service', exact: true }).click();
  await page.getByRole('option', { name: 'checkout' }).click();
  await expect(form.getByRole('combobox', { name: 'Environment', exact: true })).toHaveText('prod');
  await expect(form.getByRole('combobox', { name: 'Good when', exact: true })).toHaveText('Request succeeds');
  await form.locator('[data-apm-slo-objective]').fill('99.5');
  await form.locator('[data-apm-slo-save]').click();

  await expect.poll(() => captured.slos.length).toBe(1);
  expect(captured.slos[0]).toMatchObject({
    name: 'Checkout availability',
    namespace: 'shop',
    service: 'checkout',
    environment: 'prod',
    kind: 'availability',
    objective: 0.995,
    windowDays: 30,
    transaction: null,
  });
});

test('an SLO opens in its form as it was set', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto('/apm/slos?slo=slo-1');

  await page.locator('[data-apm-drawer="slo"]').getByRole('button', { name: 'Edit SLO' }).click();
  const form = page.locator('[data-apm-slo-form]');
  await expect(form.getByRole('combobox', { name: 'Service', exact: true })).toHaveText('checkout');
  await expect(form.getByRole('combobox', { name: 'Environment', exact: true })).toHaveText('prod');
  await expect(form.getByRole('combobox', { name: 'Good when', exact: true })).toHaveText('Request succeeds');
  await expect(form.locator('[data-apm-slo-objective]')).toHaveValue('99.9');
  await form.locator('[data-apm-slo-objective]').fill('99.95');
  await form.locator('[data-apm-slo-save]').click();

  await expect.poll(() => captured.slos.length).toBe(1);
  expect(captured.slos[0]).toMatchObject({ service: 'checkout', environment: 'prod', objective: 0.9995, windowDays: 30 });
});

test('the service overview lists its SLOs and leads to them', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto(SERVICE_PATH);

  const card = page.locator('[data-apm-card="service-slos"]');
  await expect(card.locator('[data-apm-service-slo="slo-1"]')).toContainText('At risk');
  await card.locator('[data-apm-service-slo="slo-1"]').click();
  await expect(page).toHaveURL(/\/apm\/slos\?slo=slo-1$/);
  await expect(page.locator('[data-apm-drawer="slo"]')).toContainText('Checkout availability');
});

test('alerts going on show in the catalog and on the service, which opens a new APM alert rule', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto('/apm/services');
  await expect(page.locator('[data-apm-service-row="checkout"] [data-apm-alert-count="1"]')).toHaveText('1 alert');

  await page.goto(SERVICE_PATH);
  await expect(page.locator('[data-apm-navigation="service"] [data-apm-alert-count="1"]')).toBeVisible();
  await page.locator('[data-apm-navigation="service"] [data-apm-new-alert]').click();
  await page.locator('[data-apm-new-alert-kind="threshold"]').click();
  await expect(page).toHaveURL(/\/alerts\/rules\/new\?source=apm&namespace=shop&service=checkout&environment=prod&metric=error_rate$/);

  // The rule editor opens on the APM source with the service filled in.
  await expect(page.locator('[data-alert-source="apm"]')).toHaveClass(/bg-indigo-dim/);
  await expect(page.locator('[data-alert-apm-source]').getByRole('combobox', { name: 'Service' })).toContainText('checkout');
  await page.getByLabel('Rule name').fill('Checkout errors');
  await page.getByRole('button', { name: 'Test rule' }).click();
  await page.getByRole('button', { name: 'Create rule' }).click();

  await expect.poll(() => captured.rules.length).toBe(1);
  expect(captured.rules[0]).toMatchObject({
    name: 'Checkout errors',
    kind: 'scheduled',
    query: { stream: null, apm: { service: 'checkout', namespace: 'shop', environment: 'prod', metric: 'error_rate' } },
    labels: { 'service.name': 'checkout', 'service.namespace': 'shop', 'deployment.environment': 'prod' },
    thresholds: [{ operator: 'gt', threshold: 5 }],
  });
});

for (const theme of ['light', 'dark'] as const) {
  test(`the SLO page is accessible in ${theme} theme`, async ({ page, mockServer }) => {
    await boot(page, mockServer.port, theme);
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.goto('/apm/slos');
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
    await expect(page.locator('[data-apm-slo-row="slo-1"]')).toBeVisible();
    const results = await new AxeBuilder({ page }).exclude('[role="status"]').exclude('[aria-live]').analyze();
    const serious = results.violations.filter((violation) => violation.impact === 'critical' || violation.impact === 'serious');
    expect(serious, JSON.stringify(serious, null, 2)).toEqual([]);
  });
}
