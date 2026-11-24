import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const MINUTE = 60_000_000;

test('the infrastructure list tells what runs now, and an entity leads to its node, its services and their changes', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const now = Date.now() * 1_000;
  const asked: URLSearchParams[] = [];
  const pod = {
    kind: 'pod',
    key: 'prod/shop/checkout-1',
    name: 'checkout-1',
    attributes: { 'k8s.cluster.name': 'prod', 'k8s.namespace.name': 'shop', 'k8s.node.name': 'node-3', 'k8s.pod.name': 'checkout-1' },
    firstSeenAt: now - 600 * MINUTE,
    lastSeenAt: now - 2 * MINUTE,
  };

  await page.route((url) => url.pathname === '/api/v1/entities', (route) => {
    asked.push(new URL(route.request().url()).searchParams);
    return route.fulfill({
      json: [
        { ...pod, services: 1 },
        { kind: 'host', key: 'db-1', name: 'db-1', attributes: { 'host.name': 'db-1' }, firstSeenAt: now - 900 * MINUTE, lastSeenAt: now - 180 * MINUTE, services: 0 },
      ],
    });
  });
  await page.route('**/api/v1/entities/detail?**', (route) =>
    route.fulfill({
      json: {
        entity: pod,
        parents: [{ kind: 'node', key: 'prod/node-3', name: 'node-3' }],
        children: [],
        services: [{ namespace: 'shop', name: 'checkout', environment: 'prod' }],
      },
    }),
  );
  await page.route('**/api/v1/changes?**', (route) =>
    route.fulfill({
      json: [{ id: 'd1', kind: 'deployment', at: now - 30 * MINUTE, service: 'checkout', namespace: 'shop', environment: 'prod', version: '2.0.0', status: 'succeeded' }],
    }),
  );

  await page.goto('/infrastructure');

  const rows = page.locator('tbody tr');
  await expect(rows).toHaveCount(2);
  await expect(rows.nth(0)).toContainText('checkout-1');
  await expect(rows.nth(0)).toContainText('prod / shop / node-3');
  await expect(rows.nth(1)).toContainText('Host');

  // Choosing a kind asks for it, and keeps it in the address.
  await page.getByRole('combobox', { name: 'Kind' }).click();
  await page.getByRole('option', { name: 'Pod' }).click();
  await expect.poll(() => asked.at(-1)?.get('kind')).toBe('pod');
  await expect(page).toHaveURL(/kind=pod/);

  await page.getByRole('link', { name: 'Open checkout-1' }).click();
  await expect(page).toHaveURL(/\/infrastructure\/pod\?key=prod%2Fshop%2Fcheckout-1/);
  await expect(page.getByText('Running now')).toBeVisible();
  await expect(page.locator('[data-entity-link="prod/node-3"]')).toHaveAttribute('href', '/infrastructure/node?key=prod%2Fnode-3');
  await expect(page.locator('[data-entity-service="checkout"]')).toHaveAttribute('href', '/apm/services/checkout?namespace=shop&environment=prod');
  await expect(page.locator('[data-entity-change="deployment"]')).toContainText('checkout 2.0.0');
  await expect(page.locator('[data-entity-logs]')).toHaveAttribute('href', /^\/logs\?pod=checkout-1&from=/);
});

test('an entity not seen says so', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.goto('/infrastructure/node?key=prod%2Fgone');
  await expect(page.getByText('Not seen')).toBeVisible();
});
