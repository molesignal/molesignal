import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW = Date.now() * 1000;
const MINUTE = 60_000_000;

const rule = (id: string, position: number, fields: Record<string, unknown>) => ({
  id,
  orgId: 'acme-prod',
  enabled: true,
  position,
  conditions: [],
  minSeverity: null,
  origins: [],
  createdBy: 'u1',
  createdAt: NOW - 60 * MINUTE,
  updatedAt: NOW - 60 * MINUTE,
  ...fields,
});

const RULES = [
  rule('rr-pay', 1, {
    name: 'Payments in production',
    conditions: [
      { key: 'env', operator: 'equals', values: ['prod'] },
      { key: 'service.name', operator: 'in', values: ['checkout', 'cart'] },
    ],
    minSeverity: 'error',
    target: { kind: 'team', teamId: 'team-payments' },
  }),
  rule('rr-db', 2, {
    name: 'Database pages',
    conditions: [{ key: 'db.system', operator: 'matches', values: ['postgres.*'] }],
    origins: ['external'],
    target: { kind: 'policy', policyId: 'esc-platform' },
  }),
  rule('rr-gone', 3, { name: 'Search team', target: { kind: 'team', teamId: 'team-gone' } }),
];

const TEAMS = [
  {
    teamId: 'team-payments',
    name: 'Payments',
    memberCount: 1,
    escalationPolicyId: 'esc-payments',
    services: ['default/checkout', 'default/cart', 'shop/basket', 'default/search'],
  },
  { teamId: 'team-platform', name: 'Platform', memberCount: 2, escalationPolicyId: null, services: [] },
];

const incident = (id: string, summary: string, fields: Record<string, unknown>) => ({
  id,
  orgId: 'acme-prod',
  ruleId: 'rule-checkout-latency',
  escalationPolicyId: 'esc-payments',
  status: 'open',
  severity: 'error',
  summary,
  fingerprint: `fp-${id}`,
  currentStep: 0,
  currentLoop: 0,
  currentStepStartedAt: NOW - 10 * MINUTE,
  assignees: [],
  labels: {},
  annotations: {},
  traceIds: [],
  hostIds: [],
  affectedServices: [],
  triggeringQuery: null,
  createdAt: NOW - 10 * MINUTE,
  ...fields,
});

test('routing rules are tried in order and moved, switched and tried from the page', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  let order: string[] | null = null;
  let toggled: Record<string, unknown> | null = null;
  let teamPolicy: Record<string, unknown> | null = null;
  let probe: Record<string, unknown> | null = null;

  await page.route(/\/api\/v1\/alerts\/routing\/rules$/, (route) => route.fulfill({ json: RULES }));
  await page.route(/\/api\/v1\/alerts\/routing\/rules\/order$/, (route) => {
    order = (route.request().postDataJSON() as { ids: string[] }).ids;
    return route.fulfill({
      json: order.map((id, index) => ({ ...RULES.find((item) => item.id === id), position: index + 1 })),
    });
  });
  await page.route(/\/api\/v1\/alerts\/routing\/rules\/rr-pay$/, (route) => {
    toggled = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({ json: { ...RULES[0], ...toggled } });
  });
  await page.route(/\/api\/v1\/alerts\/routing\/teams$/, (route) => route.fulfill({ json: TEAMS }));
  await page.route(/\/api\/v1\/alerts\/routing\/teams\/team-platform$/, (route) => {
    teamPolicy = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({ status: 204, body: '' });
  });
  await page.route(/\/api\/v1\/alerts\/routing\/test$/, (route) => {
    probe = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({
      json: {
        decision: {
          escalationPolicyId: 'esc-payments',
          teamId: 'team-payments',
          routing: { reason: 'rule', ruleId: 'rr-pay', ruleName: 'Payments in production' },
        },
        matchedRuleId: 'rr-pay',
        service: 'default/checkout',
        serviceTeamId: 'team-payments',
        labelTeamId: null,
      },
    });
  });

  await page.goto('/alerts/routing');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Alerts');

  // The rules in the order they are tried, each with what it matches and where it sends.
  const rows = page.locator('[data-routing-rules] > li');
  await expect(rows).toHaveCount(3);
  await expect(rows.first()).toHaveAttribute('data-routing-rule', 'Payments in production');
  await expect(rows.first()).toContainText('env = prod · service.name ∈ checkout, cart · Error or worse');
  await expect(rows.first().locator('[data-routing-target]')).toHaveText('TeamPayments');
  await expect(rows.nth(1)).toContainText('db.system ~ postgres.* · External alerts only');
  await expect(rows.nth(1).locator('[data-routing-target]')).toHaveText('PolicyPlatform on-call');
  // A rule whose team was deleted says so; routing passes it over.
  await expect(rows.nth(2).locator('[data-routing-target]')).toHaveText('Target deleted');

  // Moved from its handle with the arrow keys; the new order is saved at once.
  await rows.first().getByRole('button', { name: 'Reorder “Payments in production”' }).focus();
  await page.keyboard.press('ArrowDown');
  await expect.poll(() => order).toEqual(['rr-db', 'rr-pay', 'rr-gone']);
  await expect(rows.first()).toHaveAttribute('data-routing-rule', 'Database pages');
  await expect(rows.nth(1)).toHaveAttribute('data-routing-rule', 'Payments in production');

  // Switched off in place, the rest of the rule as it was.
  await page.getByRole('switch', { name: 'Enable “Payments in production”' }).click();
  await expect.poll(() => toggled).toMatchObject({
    name: 'Payments in production',
    enabled: false,
    minSeverity: 'error',
    target: { kind: 'team', teamId: 'team-payments' },
  });

  // Each team: its services, linked to the catalog under the team, and its on-call policy.
  const teams = page.locator('[data-routing-teams]');
  await expect(teams.getByRole('link', { name: '4 services' })).toHaveAttribute(
    'href',
    '/apm/services?team=team-payments',
  );
  await expect(teams).toContainText('checkout, cart, shop/basket, …');
  await teams.getByRole('combobox', { name: 'On-call policy of Platform' }).click();
  await page.getByRole('option', { name: 'Platform on-call' }).click();
  await expect.poll(() => teamPolicy).toEqual({ escalationPolicyId: 'esc-platform' });

  // A set of labels tried against both: where it goes, and what each step saw.
  await page.getByRole('textbox', { name: 'Labels' }).fill('service.name=checkout\nenv=prod');
  await page.getByRole('combobox', { name: 'Origin' }).click();
  await page.getByRole('option', { name: 'External alerts' }).click();
  await page.getByRole('combobox', { name: 'Severity' }).click();
  await page.getByRole('option', { name: 'Critical' }).click();
  await page.getByRole('button', { name: 'Try', exact: true }).click();
  await expect.poll(() => probe).toEqual({
    labels: { 'service.name': 'checkout', env: 'prod', 'alert.source': 'external' },
    severity: 'critical',
  });

  const outcome = page.locator('[data-routing-outcome]');
  await expect(outcome).toHaveAttribute('data-routing-outcome', 'rule');
  await expect(outcome.locator('[data-routing-outcome-team]')).toHaveText('Payments');
  await expect(outcome.locator('[data-routing-outcome-policy]')).toHaveText('Payments');
  await expect(outcome).toContainText('Matched rule “Payments in production”');
  await expect(outcome.locator('[data-routing-step="rule"]')).toHaveAttribute('data-state', 'used');
  await expect(outcome.locator('[data-routing-step="service"]')).toContainText('checkout belongs to Payments');
  await expect(outcome.locator('[data-routing-step="default"]')).toHaveAttribute('data-state', 'passed');
});

test('a routing rule is written from its drawer', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  let created: Record<string, unknown> | null = null;
  await page.route(/\/api\/v1\/alerts\/routing\/rules$/, (route) => {
    if (route.request().method() === 'POST') {
      created = route.request().postDataJSON() as Record<string, unknown>;
      return route.fulfill({ json: rule('rr-new', 1, created) });
    }
    return route.fulfill({ json: created ? [rule('rr-new', 1, created)] : [] });
  });
  await page.route(/\/api\/v1\/alerts\/routing\/teams$/, (route) => route.fulfill({ json: TEAMS }));

  await page.goto('/alerts/routing');
  await expect(page.getByText('No routing rules yet.')).toBeVisible();

  await page.getByRole('button', { name: 'New rule' }).click();
  const drawer = page.getByRole('dialog');
  await drawer.getByRole('textbox', { name: 'Name', exact: true }).fill('Payments in production');
  await drawer.getByRole('button', { name: 'Add condition' }).click();
  await drawer.getByRole('textbox', { name: 'Label', exact: true }).fill('env');
  await drawer.getByRole('textbox', { name: 'Value', exact: true }).fill('prod');
  await drawer.getByRole('combobox', { name: 'Minimum severity' }).click();
  await page.getByRole('option', { name: 'Error' }).click();
  // Only the two heaviest priorities, and only outside office hours.
  await drawer.locator('[data-routing-priorities] label', { hasText: 'P2' }).click();
  await drawer.locator('[data-routing-priorities] label', { hasText: 'P1' }).click();
  await drawer.getByRole('combobox', { name: 'Time', exact: true }).click();
  await page.getByRole('option', { name: 'Only outside these hours' }).click();
  await expect(drawer.locator('[data-weekly-windows]')).toBeVisible();
  await drawer.getByRole('combobox', { name: 'Team', exact: true }).click();
  await page.getByRole('option', { name: 'Payments' }).click();
  await drawer.getByRole('button', { name: 'Create', exact: true }).click();

  await expect.poll(() => created).toMatchObject({
    name: 'Payments in production',
    enabled: true,
    conditions: [{ key: 'env', operator: 'equals', values: ['prod'] }],
    minSeverity: 'error',
    origins: [],
    // Ticked in any order, kept heaviest first.
    priorities: ['P1', 'P2'],
    time: { mode: 'outside', ranges: [{ weekdayMask: 0b011_1110, start: '09:00', end: '18:00' }] },
    target: { kind: 'team', teamId: 'team-payments' },
  });
  await expect(page.locator('[data-routing-rules] > li')).toHaveCount(1);
  await expect(page.locator('[data-routing-rules] > li')).toContainText('priority P1, P2');
});

test('incidents name the team routing gave them, and the reader sees their teams’ own', async ({
  page,
  mockServer,
}) => {
  await mountMockRoutes(page, mockServer.port);

  const payments = incident('inc-pay', 'Checkout p95 above 800 ms', {
    teamId: 'team-payments',
    routing: { reason: 'rule', ruleId: 'rr-pay', ruleName: 'Payments in production' },
  });
  const platform = incident('inc-api', 'Public API 5xx above 2%', {
    severity: 'critical',
    escalationPolicyId: 'esc-platform',
    teamId: 'team-platform',
    routing: { reason: 'service_owner', service: 'default/api-gateway' },
  });

  // The reader is in Payments.
  await page.route(/\/api\/v1\/teams$/, (route) =>
    route.fulfill({
      json: [
        { id: 'team-payments', orgId: 'acme-prod', name: 'Payments', memberIds: ['dev'] },
        { id: 'team-platform', orgId: 'acme-prod', name: 'Platform', memberIds: ['u1'] },
      ],
    }),
  );
  await page.route(/\/api\/v1\/alerts\/incidents(\?.*)?$/, (route) => route.fulfill({ json: [platform, payments] }));
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-pay$/, (route) => route.fulfill({ json: payments }));

  await page.goto('/alerts/incidents');
  const rows = page.locator('tbody tr');
  await expect(rows).toHaveCount(2);
  await expect(rows.first().locator('[data-incident-team-name]')).toHaveText('Platform');
  await expect(rows.nth(1).locator('[data-incident-team-name]')).toHaveText('Payments');

  await page.locator('[data-alert-filter-tabs]').getByRole('button', { name: 'My teams' }).click();
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toContainText('Checkout p95 above 800 ms');

  // The incident's own page says which team has it and why.
  await page.goto('/alerts/incidents/inc-pay');
  const team = page.locator('[data-incident-team]');
  await expect(team).toContainText('Payments');
  await expect(team).toContainText('Matched rule “Payments in production”');
});
