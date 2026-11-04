/**
 * An incident opened by hand from the incidents tab: what happened, how bad, and who takes it; the
 * page it lands on says who opened it.
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW = Date.now() * 1000;

test('an incident is opened by hand for a team and lands on its page', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  let sent: Record<string, unknown> | null = null;
  const created = {
    id: 'inc-hand',
    orgId: 'acme-prod',
    ruleId: 'manual',
    escalationPolicyId: 'esc-payments',
    status: 'open',
    severity: 'critical',
    summary: 'Customers in the north cannot pay',
    fingerprint: 'fp-hand',
    currentStep: 0,
    currentLoop: 0,
    currentStepStartedAt: NOW,
    assignees: [],
    labels: { 'alert.source': 'manual', env: 'prod' },
    annotations: { createdBy: 'u2', description: 'Three tickets in ten minutes.' },
    traceIds: [],
    hostIds: [],
    affectedServices: ['checkout'],
    triggeringQuery: null,
    createdAt: NOW,
    teamId: 'team-payments',
    routing: { reason: 'manual' },
    priority: 'P1',
    urgency: 'high',
  };
  await page.route(/\/api\/v1\/alerts\/incidents(\?.*)?$/, (route) => {
    if (route.request().method() === 'POST') {
      sent = route.request().postDataJSON() as Record<string, unknown>;
      return route.fulfill({ json: created });
    }
    return route.fallback();
  });
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-hand$/, (route) => route.fulfill({ json: created }));

  await page.goto('/alerts/incidents');
  await page.getByTestId('incident-create').click();

  const drawer = page.getByRole('dialog', { name: 'New incident' });
  await drawer.getByRole('textbox', { name: 'Title' }).fill('Customers in the north cannot pay');
  await drawer.getByRole('textbox', { name: 'Description' }).fill('Three tickets in ten minutes.');
  await drawer.getByRole('combobox', { name: 'Severity' }).click();
  await page.getByRole('option', { name: 'Critical' }).click();
  await drawer.getByRole('combobox', { name: 'Priority' }).click();
  await page.getByRole('option', { name: 'P1' }).click();
  await drawer.getByRole('textbox', { name: 'Affected services' }).fill('checkout');
  await drawer.getByRole('textbox', { name: 'Labels' }).fill('env=prod');

  // Given to a team: the create button waits until one is chosen.
  await drawer.getByText('A team', { exact: true }).click();
  await expect(page.getByRole('button', { name: 'Create incident' })).toBeDisabled();
  await drawer.getByRole('combobox', { name: 'Team' }).click();
  await page.getByRole('option', { name: 'Payments' }).click();
  await page.getByRole('button', { name: 'Create incident' }).click();

  await expect.poll(() => sent).toEqual({
    title: 'Customers in the north cannot pay',
    description: 'Three tickets in ten minutes.',
    severity: 'critical',
    priority: 'P1',
    assign: { kind: 'team', teamId: 'team-payments' },
    services: ['checkout'],
    labels: { env: 'prod' },
    declareMajor: false,
  });
  await expect(page).toHaveURL(/\/alerts\/incidents\/inc-hand$/);
  await expect(page.locator('[data-incident-created-by]')).toBeVisible();
});
