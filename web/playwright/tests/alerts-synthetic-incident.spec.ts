import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW = Date.now() * 1000;

// What the server answers for an incident a failing synthetic check opened: its `rule_id` is the check's.
const incident = {
  id: 'inc-synth',
  orgId: 'acme-prod',
  ruleId: 'monitor-1',
  escalationPolicyId: '',
  status: 'open',
  severity: 'error',
  summary: 'Checkout API',
  fingerprint: 'fp-synth',
  currentStep: 0,
  currentLoop: 0,
  currentStepStartedAt: NOW - 600_000_000,
  assignees: [],
  labels: { 'alert.source': 'synthetic', 'synthetic.state': 'failing', 'synthetic.kind': 'http' },
  annotations: {},
  traceIds: [],
  hostIds: [],
  affectedServices: [],
  triggeringQuery: null,
  createdAt: NOW - 600_000_000,
  acknowledgedAt: null,
  acknowledgedBy: null,
  resolvedAt: null,
  resolvedBy: null,
};

test('an incident a synthetic check opened names the check and links to it, not to a rule', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  const ruleReads: string[] = [];
  page.on('request', (request) => {
    if (request.url().includes('/api/v1/alerts/rules/monitor-1')) ruleReads.push(request.url());
  });
  await page.route(/\/api\/v1\/alerts\/incidents(\?.*)?$/, (route) => route.fulfill({ json: [incident] }));
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-synth$/, (route) => route.fulfill({ json: incident }));

  await page.goto('/alerts/incidents');
  await expect(page.locator('tbody tr').first()).toContainText('Synthetic');

  await page.goto('/alerts/incidents/inc-synth');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Checkout API');
  await expect(page.getByText('Synthetic check', { exact: true })).toBeVisible();
  await expect(page.locator('a[href="/synthetics/checks/monitor-1"]')).toHaveText('Checkout API');

  // There is no rule behind it to read.
  expect(ruleReads).toEqual([]);
});
