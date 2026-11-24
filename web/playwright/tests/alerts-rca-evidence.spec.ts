import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test('a root cause analysis lists the evidence it cites, each piece opening where it came from', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.goto('/alerts/incidents/inc-2291');
  await page.getByTestId('incident-rca-generate').click();

  const evidence = page.locator('[data-rca-evidence]');
  await expect(evidence).toBeVisible();
  expect(await evidence.locator('[data-rca-evidence-label]').evaluateAll((rows) => rows.map((row) => row.getAttribute('data-rca-evidence-label')))).toEqual([
    'E1',
    'E2',
    'E3',
  ]);

  const insight = evidence.locator('[data-rca-evidence-label="E1"]');
  await expect(insight).toContainText('APM insight');
  await expect(insight).toContainText('api-gateway');
  await expect(insight).toContainText('now 16%, before 0.4%, a week ago 0.3%');
  await expect(insight.getByRole('link', { name: 'Open E1' })).toHaveAttribute('href', '/apm/services/api-gateway?namespace=edge&environment=prod');

  // Figures read from logs the member may not read are left out, and the row says so.
  const logs = evidence.locator('[data-rca-evidence-label="E2"]');
  await expect(logs).toContainText('Figures hidden: you cannot read this data');
  await expect(logs.getByRole('link', { name: 'Open E2' })).toHaveAttribute('href', /^\/logs\?stream=default&/);

  await expect(evidence.locator('[data-rca-evidence-label="E3"]')).toContainText('deployment · edge-proxy 2.4.0 (succeeded) · by ada');
  await expect(evidence.locator('[data-rca-evidence-label="E3"]').getByRole('link')).toHaveCount(0);
});
