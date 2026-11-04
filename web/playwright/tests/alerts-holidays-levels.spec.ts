/**
 * The fields P4 added across alerting: holiday calendars in silences and notify policies, the
 * organization's urgency levels in an escalation policy that is edited before they load, the home
 * cluster of a real-time rule, automations on new discovery findings, and log findings.
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const CALENDAR = {
  id: 'cal-cn',
  orgId: 'acme-prod',
  name: 'CN 2026',
  description: '',
  days: [{ date: '2026-10-01', kind: 'holiday', name: 'National Day' }],
  createdBy: null,
  createdAt: 1,
  updatedAt: 1,
};

const LEVELS = {
  priorities: ['P1', 'P2', 'P3', 'P4', 'P5'].map((priority) => ({ priority, name: '', description: '' })),
  urgencies: [
    { key: 'urgent', name: 'Urgent', escalates: true, quietHours: 'switch', steps: 'high', severities: ['critical'] },
    { key: 'high', name: '', escalates: true, quietHours: 'switch', steps: 'high', severities: ['error'] },
    { key: 'low', name: '', escalates: false, quietHours: 'block', steps: 'low', severities: ['warning', 'info'] },
  ],
};

test('a policy at an added urgency level keeps it when the levels load after the policy', async ({
  page,
  mockServer,
}) => {
  await mountMockRoutes(page, mockServer.port);

  await page.route(/\/api\/v1\/alerts\/importance$/, async (route) => {
    await new Promise((resolve) => setTimeout(resolve, 800));
    return route.fulfill({ json: { levels: LEVELS, updatedBy: null, updatedAt: 1 } });
  });
  await page.route(/\/api\/v1\/alerts\/escalations$/, (route) => {
    if (route.request().method() !== 'GET') return route.fallback();
    return route.fulfill({
      json: [
        {
          id: 'esc-urgent',
          orgId: 'acme-prod',
          name: 'Always urgent',
          steps: [{ targets: [{ kind: 'user', userId: 'u1' }], ackTimeoutSecs: 300, minSeverity: null }],
          repeat: false,
          maxLoops: 1,
          ackRetriggerSecs: null,
          urgencyRule: { mode: 'always', urgency: 'urgent' },
          escalateLowUrgency: false,
        },
      ],
    });
  });
  let saved: Record<string, unknown> | null = null;
  await page.route(/\/api\/v1\/alerts\/escalations\/esc-urgent$/, (route) => {
    if (route.request().method() !== 'PUT') return route.fallback();
    saved = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({ json: { id: 'esc-urgent', orgId: 'acme-prod', ...saved } });
  });

  await page.goto('/alerts/escalations');
  await page.getByRole('row', { name: /Always urgent/ }).getByRole('button', { name: 'Edit' }).click();
  const drawer = page.getByRole('dialog');
  await expect(drawer.locator('[data-urgency-rule="always"]')).toBeVisible();
  await expect(drawer.getByRole('combobox', { name: 'Urgency' })).toContainText('Urgent');

  await drawer.getByRole('button', { name: 'Save policy' }).click();
  await expect.poll(() => saved).toMatchObject({ urgencyRule: { mode: 'always', urgency: 'urgent' } });
});

test('a silence covers the days off of a holiday calendar', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  await page.route(/\/api\/v1\/alerts\/holiday-calendars$/, (route) => route.fulfill({ json: [CALENDAR] }));
  let created: Record<string, unknown> | null = null;
  await page.route(/\/api\/v1\/alerts\/mutes$/, (route) => {
    if (route.request().method() !== 'POST') return route.fallback();
    created = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({ status: 201, json: { id: 'mute-holidays', orgId: 'acme-prod', active: false, ...created } });
  });

  await page.goto('/alerts/silences');
  await page.getByRole('button', { name: 'New silence policy' }).click();
  const drawer = page.getByRole('dialog');
  await drawer.getByRole('textbox', { name: 'Name', exact: true }).fill('Quiet on public holidays');
  await drawer.getByRole('button', { name: 'Add matcher' }).click();
  await drawer.getByRole('textbox', { name: 'Matcher label' }).fill('env');
  await drawer.getByRole('textbox', { name: 'Matcher value' }).fill('test');
  await drawer.getByRole('combobox', { name: 'Window type' }).click();
  await page.getByRole('option', { name: 'Holidays', exact: true }).click();
  await expect(drawer.getByRole('combobox', { name: 'Holiday calendar' })).toContainText('CN 2026');
  await drawer.getByRole('textbox', { name: 'Reason' }).fill('Test systems stay quiet on public holidays');
  await drawer.getByRole('button', { name: 'Create silence' }).click();

  await expect.poll(() => created).toMatchObject({
    name: 'Quiet on public holidays',
    matchers: [{ label: 'env', op: 'eq', value: 'test' }],
    window: {
      type: 'weekly',
      ranges: [{ weekdayMask: 127, start: '00:00', end: '24:00' }],
      calendar: { calendarId: 'cal-cn', holidays: 'only' },
    },
  });
});

const POLICY = {
  id: 'own-1',
  organizationId: 'org-1',
  name: 'Payments on call',
  builtinKey: null,
  eventType: 'alert.triggered',
  category: 'alert',
  matchers: {},
  recipientResolver: 'alert_owner',
  resolverConfig: {},
  deliveryMode: 'prefer_user',
  deliveryConfig: { connectorIds: [] },
  templateId: null,
  channelTemplates: {},
  fallbackConfig: { useUserFallbacks: true, useTeamDefaults: true, useOrganizationDefaults: true },
  ackTimeoutSeconds: null,
  escalationConfig: null,
  mergeWindowSecs: null,
  enabled: true,
  priority: 50,
  createdAt: 1,
  updatedAt: 1,
};

test('a notify policy sends on workdays only, holidays left out', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  await page.route(/\/api\/v1\/alerts\/holiday-calendars$/, (route) => route.fulfill({ json: [CALENDAR] }));
  await page.route('**/api/v1/notify/policies', (route) => route.fulfill({ json: [POLICY] }));
  await page.route('**/api/v1/notify/policies/preview', (route) =>
    route.fulfill({ json: { policyId: 'own-1', matched: true, recipients: [] } }),
  );
  let saved: Record<string, unknown> | null = null;
  await page.route('**/api/v1/notify/policies/own-1', (route) => {
    if (route.request().method() !== 'PUT') return route.fallback();
    saved = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({ json: { ...POLICY, ...saved } });
  });

  await page.goto('/settings/notify/policies');
  await page.getByRole('row', { name: /Payments on call/ }).getByRole('button', { name: 'Edit' }).click();
  const drawer = page.getByRole('dialog', { name: /Payments on call/ });
  await drawer.getByRole('combobox', { name: 'Time' }).click();
  await page.getByRole('option', { name: 'Only inside these hours' }).click();
  await drawer.getByRole('combobox', { name: 'Holidays' }).click();
  await page.getByRole('option', { name: 'Leave holidays out' }).click();
  await expect(drawer.getByRole('combobox', { name: 'Holiday calendar' })).toContainText('CN 2026');
  await drawer.getByRole('button', { name: 'Save policy' }).click();

  await expect.poll(() => saved).toMatchObject({
    time: { mode: 'inside', calendar: { calendarId: 'cal-cn', holidays: 'exclude' } },
  });
});

test('a real-time rule is decided in one cluster of the federation', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  const rule = {
    id: 'rt-1',
    orgId: 'acme-prod',
    name: 'Payment errors',
    description: '',
    enabled: true,
    kind: 'real_time',
    query: {
      language: 'sql',
      statement: "SELECT count(*) FROM app WHERE level = 'error'",
      periodSecs: 60,
      stream: { name: 'app', streamType: 'logs' },
      homeCluster: 'west',
    },
    trigger: { operator: 'gte', threshold: 5, forPeriods: 1, silenceSecs: 300 },
    thresholds: [],
    severity: null,
    escalationPolicyId: 'default',
    labels: {},
    annotations: {},
  };
  await page.route(/\/api\/v1\/query(\?.*)?$/, (route) =>
    route.fulfill({ json: { columns: ['value'], rows: [[2], [7], [3]], scannedRows: 3, tookMs: 2 } }),
  );
  await page.route(/\/api\/v1\/alerts\/sources\/clusters$/, (route) =>
    route.fulfill({
      json: {
        local: 'east',
        clusters: [
          { id: 'east', name: 'east', local: true },
          { id: 'west', name: 'west', local: false },
        ],
      },
    }),
  );
  let saved: Record<string, unknown> | null = null;
  await page.route(/\/api\/v1\/alerts\/rules\/rt-1$/, (route) => {
    if (route.request().method() === 'GET') return route.fulfill({ json: rule });
    if (route.request().method() !== 'PUT') return route.fallback();
    saved = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({ json: { ...rule, ...saved } });
  });

  await page.goto('/alerts/rules/rt-1/edit');
  const field = page.getByRole('combobox', { name: 'Decided in' });
  await expect(field).toContainText('west');
  await field.click();
  await page.getByRole('option', { name: 'east (this cluster)' }).click();
  // Saving waits for a test run of the query.
  await page.getByRole('button', { name: 'Test rule', exact: true }).click();
  await page.getByRole('button', { name: 'Save and enable' }).click();

  await expect.poll(() => saved).toMatchObject({ kind: 'real_time', query: { homeCluster: 'east' } });
});

test('an automation looks into new findings of automatic discovery', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  await page.goto('/agent/automations');
  await page.getByRole('button', { name: 'New automation' }).click();
  await page.locator('[data-automation-template="blank"]').click();

  const editor = page.locator('[data-automation-editor="new"]');
  await editor.getByLabel('Name').fill('Look into new errors');
  await editor.getByLabel('What to do').fill('Find out why, and say what to do next.');
  await editor.getByRole('combobox', { name: 'Starts' }).click();
  await page.getByRole('option', { name: 'New discovery findings' }).click();
  await editor.locator('[data-finding-kinds]').getByText('New error log', { exact: true }).click();
  await editor.getByLabel('Services, hosts or containers').fill('checkout, db-1');

  const created = page.waitForRequest(
    (request) => request.method() === 'POST' && request.url().endsWith('/api/v1/agent/automations'),
  );
  await page.getByRole('dialog').getByRole('button', { name: 'Create' }).click();

  expect((await created).postDataJSON()).toMatchObject({
    name: 'Look into new errors',
    trigger: { kind: 'finding', kinds: ['log_new_error'], subjects: ['checkout', 'db-1'] },
  });
});

test('a new error log opens its stream and is handed to Mole Agent', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  const now = Date.now() * 1000;
  await page.route(/\/api\/v1\/alerts\/discovery\/findings(\?.*)?$/, (route) =>
    route.fulfill({
      json: [
        {
          id: 'finding-log',
          orgId: 'acme-prod',
          kind: 'log_new_error',
          key: 'log_new_error|app_errors|checkout|abc',
          severity: 'warning',
          subject: 'checkout',
          environment: null,
          metric: null,
          title: 'New error in checkout: payment failed for order #, request #',
          value: 30,
          baseline: null,
          startedAt: now - 600_000_000,
          lastSeenAt: now,
          endedAt: null,
          missedRounds: 0,
          detail: { stream: 'app_errors', template: 'payment failed for order #, request #', service: 'checkout' },
        },
      ],
    }),
  );

  await page.goto('/alerts/discovery');
  const row = page.locator('tbody tr').first();
  await expect(row).toContainText('New error in checkout: payment failed for order #, request #');
  await row.click();

  const drawer = page.getByRole('dialog');
  await drawer.getByTestId('discovery-investigate').click();
  await expect(page.getByTestId('composer-input')).toContainText('Look into this finding of automatic discovery');

  await page.goto('/alerts/discovery');
  await expect(row).toContainText('New error in checkout');
  await row.click();
  await page.getByRole('dialog').getByRole('button', { name: 'Open the logs' }).click();
  await expect(page).toHaveURL(/\/logs\?stream=app_errors&streamType=logs/);
});
