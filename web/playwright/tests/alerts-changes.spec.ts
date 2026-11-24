import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const MINUTE = 60_000_000;

test('the changes page lists every change of the range, the latest first, each kind as itself', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const now = Date.now() * 1_000;
  const asked: URLSearchParams[] = [];

  await page.route('**/api/v1/changes?**', (route) => {
    asked.push(new URL(route.request().url()).searchParams);
    return route.fulfill({
      json: [
        { id: 'rule:r1:1', kind: 'rule', at: now - 5 * MINUTE, title: 'Checkout errors', ruleId: 'r1' },
        {
          id: 'deploy-2',
          kind: 'deployment',
          at: now - 30 * MINUTE,
          service: 'checkout',
          namespace: 'shop',
          environment: 'prod',
          version: '2.0.0',
          status: 'failed',
          author: 'ada',
          commit: '9f2c1e7a4b3d5e6f',
        },
        { id: 'a1', kind: 'annotation', at: now - 90 * MINUTE, end: now - 60 * MINUTE, title: 'Load test', dashboardId: 'dash-1' },
      ],
    });
  });

  await page.goto('/alerts/changes');

  const rows = page.locator('tbody tr');
  await expect(rows).toHaveCount(3);
  await expect(rows.nth(0)).toContainText('Rule edited');
  await expect(rows.nth(0)).toContainText('Checkout errors');
  await expect(rows.nth(1)).toContainText('checkout 2.0.0');
  await expect(rows.nth(1)).toContainText('failed');
  await expect(rows.nth(1)).toContainText('ada · 9f2c1e7');
  await expect(rows.nth(1).getByRole('link', { name: 'Open checkout 2.0.0' })).toHaveAttribute(
    'href',
    '/apm/services/checkout/deployments?namespace=shop&environment=prod',
  );
  await expect(rows.nth(2).getByRole('link', { name: 'Open Load test' })).toHaveAttribute('href', '/dashboards/dash-1');

  // The rail marks all three across the week asked for.
  await expect(page.locator('[data-change-rail] [data-change-mark]')).toHaveCount(3);
  const range = asked.at(-1)!;
  expect(Number(range.get('to')) - Number(range.get('from'))).toBe(7 * 24 * 60 * MINUTE);

  // Choosing a kind asks for that kind alone.
  await page.getByRole('combobox', { name: 'Kind' }).click();
  await page.getByRole('option', { name: 'Deployment' }).click();
  await expect.poll(() => asked.at(-1)?.get('kinds')).toBe('deployment');
});
