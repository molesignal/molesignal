/**
 * What automatic discovery found without a rule, and the rule that turns findings like one into
 * incidents.
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test('discovery lists what it found and writes a rule for findings like one', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  let created: Record<string, unknown> | null = null;
  await page.route(/\/api\/v1\/alerts\/rules$/, (route) => {
    if (route.request().method() !== 'POST') return route.fallback();
    created = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({ json: { id: 'rule-discovery', orgId: 'acme-prod', ...created } });
  });

  await page.goto('/alerts/discovery');
  const rows = page.locator('tbody tr');
  await expect(rows).toHaveCount(2);
  await expect(rows.first()).toContainText('The error rate of checkout is 12.0% (was 0.40%)');
  await expect(rows.nth(1)).toContainText('New error in Web shop: TypeError: cart is undefined');

  // The ended one is under its own view.
  await page.locator('[data-discovery-views]').getByRole('button', { name: 'Ended' }).click();
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toContainText('search 1.8.0 does worse than the version before');
  await page.locator('[data-discovery-views]').getByRole('button', { name: 'Open' }).click();

  // A finding says what it compares and what changed near when it began.
  await rows.first().click();
  const drawer = page.getByRole('dialog');
  await expect(drawer.locator('[data-finding-changes]')).toContainText('checkout 2.14.0');
  await drawer.getByTestId('discovery-rule-from-finding').click();

  const form = page.getByRole('dialog', { name: 'New discovery rule' });
  await expect(form.getByRole('textbox', { name: 'Name', exact: true })).toHaveValue('Discovery: checkout');
  await expect(form.getByRole('textbox', { name: 'Services or applications' })).toHaveValue('default/checkout');
  await page.getByRole('button', { name: 'Create rule' }).click();

  await expect.poll(() => created).toMatchObject({
    name: 'Discovery: checkout',
    query: {
      statement: 'discovery',
      discovery: {
        kinds: ['apm_anomaly'],
        subjects: ['default/checkout'],
        environments: ['production'],
        minSeverity: null,
      },
    },
    escalationPolicyId: 'default',
  });
});
