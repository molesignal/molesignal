import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test('a rule recovers past its own line and opens an incident when its data stops', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  await page.route(/\/api\/v1\/query(\?.*)?$/, (route) =>
    route.fulfill({
      json: {
        columns: ['_timestamp', 'value'],
        rows: [
          [1_700_000_000_000_000, 0.2],
          [1_700_000_060_000_000, 0.07],
        ],
        scannedRows: 2,
        tookMs: 3,
      },
    }),
  );
  const saved: Array<Record<string, unknown>> = [];
  await page.route(/\/api\/v1\/alerts\/rules$/, async (route) => {
    if (route.request().method() !== 'POST') return route.fallback();
    const body = route.request().postDataJSON() as Record<string, unknown>;
    saved.push(body);
    return route.fulfill({ json: { ...body, id: 'rule-1', orgId: 'default', escalationPolicyId: 'default' } });
  });

  await page.goto('/alerts/rules/new');
  await page.getByLabel('Rule name').fill('Error rate');

  const conditions = page.locator('[data-alert-conditions]');
  const line = conditions.getByLabel('Recovery threshold');
  // The default band fires above 0.05: a line above it is on the wrong side.
  await line.fill('0.5');
  await expect(conditions).toContainText('The recovery threshold has to be on the healthy side');
  await line.fill('0.01');
  await expect(conditions).not.toContainText('healthy side');

  // Waiting for data only matters for a no-data incident.
  await expect(conditions.getByLabel('Without data for')).toHaveCount(0);
  await conditions.getByRole('combobox', { name: 'Count it as' }).click();
  await page.getByRole('option', { name: 'Open a no-data incident' }).click();
  await expect(conditions.getByLabel('Without data for')).toHaveValue('5');

  await page.getByRole('button', { name: 'Test rule', exact: true }).click();
  await expect(page.locator('[data-alert-query-preview]')).toContainText('Current value');

  // Testing scrolled the page down to the preview; the save button is in the page head.
  await page.mouse.move(400, 400);
  await page.mouse.wheel(0, -10_000);
  await page.getByRole('button', { name: 'Create rule', exact: true }).click();

  await expect.poll(() => saved.length).toBe(1);
  expect(saved[0]).toMatchObject({
    trigger: {
      recovery: { threshold: 0.01, forPeriods: 1 },
      noData: { action: 'alert', forPeriods: 5 },
    },
  });
});
