/**
 * What else an incident's page offers to go on: the past incidents like it, how they ended and what
 * was found, and the changes reported near its trigger.
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const MINUTE = 60_000_000;

test('an incident names past ones like it and the changes near its trigger', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  const incident = await (await page.request.get(`http://127.0.0.1:${mockServer.port}/api/v1/alerts/incidents/inc-2292`)).json();
  const triggered: number = incident.createdAt;

  await page.route(/\/api\/v1\/alerts\/incidents\/inc-2292\/similar$/, (route) =>
    route.fulfill({
      json: {
        incidents: [
          {
            id: 'inc-2259',
            summary: 'Checkout p95 above 800 ms during the sale',
            severity: 'error',
            status: 'resolved',
            createdAt: triggered - 4 * 24 * 60 * MINUTE,
            resolvedAt: triggered - 4 * 24 * 60 * MINUTE + 50 * MINUTE,
            resolvedBy: 'u2',
            score: 9.4,
            reasons: ['fingerprint', 'services', 'title'],
            notes: 2,
            rootCause: 'The payments pool ran out of connections during the sale.',
          },
        ],
      },
    }),
  );
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-2292\/changes$/, (route) =>
    route.fulfill({
      json: {
        from: triggered - 30 * MINUTE,
        to: triggered + 10 * MINUTE,
        changes: [
          {
            kind: 'deployment',
            at: triggered - 12 * MINUTE,
            service: 'checkout',
            downstream: false,
            version: 'v2026.10.28',
            status: 'succeeded',
            author: 'dana',
            link: 'https://ci.example.com/run/7',
          },
          { kind: 'feature_flag', at: triggered - 5 * MINUTE, service: 'payments', downstream: true, status: 'failed' },
          { kind: 'rule', at: triggered - 6 * 60 * MINUTE, downstream: false, ruleId: 'rule-checkout-latency', ruleName: 'Checkout p95 latency' },
        ],
      },
    }),
  );

  await page.goto('/alerts/incidents/inc-2292');

  const similar = page.locator('[data-incident-similar]');
  const past = similar.locator('[data-similar-incident="inc-2259"]');
  await expect(past.getByRole('link', { name: 'Checkout p95 above 800 ms during the sale' })).toHaveAttribute(
    'href',
    '/alerts/incidents/inc-2259',
  );
  await expect(past).toContainText('lasted 50m');
  await expect(past).toContainText('resolved by Kenji Watanabe');
  await expect(past).toContainText('2 notes');
  await expect(past).toContainText('same alert, same services, similar title');
  await expect(past).toContainText('The payments pool ran out of connections during the sale.');

  const changes = page.locator('[data-incident-changes]');
  const deployment = changes.locator('[data-incident-change="deployment"]');
  await expect(deployment).toContainText('12m before');
  await expect(deployment).toContainText('Deployment checkout v2026.10.28');
  await expect(deployment.getByRole('link', { name: 'Open pipeline' })).toHaveAttribute('href', 'https://ci.example.com/run/7');
  const flag = changes.locator('[data-incident-change="feature_flag"]');
  await expect(flag).toContainText('payments');
  await expect(flag).toContainText('downstream');
  await expect(flag).toContainText('failed');
  await expect(changes.locator('[data-incident-change="rule"]').getByRole('link', { name: 'Checkout p95 latency' })).toHaveAttribute(
    'href',
    '/alerts/rules/rule-checkout-latency/edit',
  );
});

test('an incident without history or changes says so', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.goto('/alerts/incidents/inc-2292');

  await expect(page.getByText('No similar incident in the last 90 days.')).toBeVisible();
  await expect(page.getByText(/Nothing was reported for these services/)).toBeVisible();
});
