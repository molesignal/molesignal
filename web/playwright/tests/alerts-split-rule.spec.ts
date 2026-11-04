import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test('a query rule split by host checks the column in its preview and is saved with it', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  await page.route(/\/api\/v1\/query(\?.*)?$/, (route) =>
    route.fulfill({
      json: {
        columns: ['host', 'errors'],
        rows: [
          ['web-1', 12],
          ['web-2', 3],
          ['web-1', 7],
        ],
        scannedRows: 3,
        tookMs: 4,
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
  await page.getByLabel('Rule name').fill('Errors per host');
  const splitBy = page.getByLabel('Split by');
  await splitBy.fill('host, service');
  await page.getByRole('button', { name: 'Test rule', exact: true }).click();

  // The result has no service column: the rule could never be split by it.
  const preview = page.locator('[data-alert-query-preview]');
  await expect(preview).toContainText('The query result has no column to split by: service');

  await splitBy.fill('host');
  await page.getByRole('button', { name: 'Test rule', exact: true }).click();
  await expect(preview).toContainText('2 groups by host');

  // Testing scrolled the page down to the preview; the save button is in the page head.
  await page.mouse.move(400, 400);
  await page.mouse.wheel(0, -10_000);
  await page.getByRole('button', { name: 'Create rule', exact: true }).click();
  await expect.poll(() => saved.length).toBe(1);
  expect(saved[0]).toMatchObject({ kind: 'scheduled', query: { groupBy: ['host'] } });
});
