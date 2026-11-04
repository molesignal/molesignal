/**
 * Remediation actions and tickets: actions set up beforehand run on an incident once approved; Jira
 * and Meegle connectors make tickets for incidents.
 */
import type { Page } from '@playwright/test';

import { FROZEN_NOW_ISO, expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1000;

const restart = {
  id: 'act-restart',
  name: 'Restart checkout pods',
  description: 'Rolling restart through the deploy pipeline.',
  target: { kind: 'http', method: 'POST', url: 'https://ci.example.com/job/restart', headers: { Authorization: '' }, body: '' },
  parameters: [{ key: 'region', label: 'Region', kind: 'select', options: ['eu', 'us'], required: true, default: 'eu' }],
  services: [],
  teamIds: [],
  risk: 'l2',
  verifyAfterSecs: 600,
  verifyBy: 'incident_resolved',
  enabled: true,
  secretsSet: ['header:Authorization'],
  createdAt: NOW - 86_400_000_000,
  updatedAt: NOW - 86_400_000_000,
};

function run(fields: Record<string, unknown>) {
  return {
    id: 'run-1',
    incidentId: 'inc-2292',
    actionId: 'act-restart',
    actionName: 'Restart checkout pods',
    approvalId: 'appr-1',
    inputs: { region: 'us' },
    status: 'awaiting_approval',
    output: '',
    verifyAt: null,
    verification: '',
    requestedBy: 'u2',
    executedBy: null,
    createdAt: NOW - 60_000_000,
    updatedAt: NOW - 60_000_000,
    ...fields,
  };
}

async function capture(page: Page, pattern: RegExp, reply: (body: Record<string, unknown>) => unknown) {
  const sent: Array<Record<string, unknown>> = [];
  await page.route(pattern, async (route) => {
    if (route.request().method() === 'GET') return route.fallback();
    const body = (route.request().postDataJSON() ?? {}) as Record<string, unknown>;
    sent.push(body);
    return route.fulfill({ json: reply(body) });
  });
  return sent;
}

test('an incident runs a remediation action once someone approves it', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  let runs: unknown[] = [];
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-2292\/remediations$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: { actions: [restart], runs } }) : route.fallback(),
  );
  const requested = await capture(page, /\/api\/v1\/alerts\/incidents\/inc-2292\/remediations$/, () => {
    runs = [run({})];
    return { run: run({}), approval: { id: 'appr-1', requiredApprovals: 1 } };
  });
  const reviewed = await capture(page, /\/api\/v1\/agent\/approvals\/appr-1\/review$/, () => {
    runs = [run({ status: 'verifying', output: 'HTTP 201: queued', executedBy: 'dev' })];
    return { approval: { id: 'appr-1', status: 'executed' } };
  });

  await page.goto('/alerts/incidents/inc-2292');
  const section = page.locator('[data-incident-remediation]');
  await section.locator('[data-remediation-action="act-restart"]').getByRole('button', { name: 'Run' }).click();

  const dialog = page.getByRole('dialog', { name: 'Run “Restart checkout pods”' });
  await expect(dialog).toContainText('One person who can approve has to approve it');
  await dialog.getByRole('combobox', { name: 'Region' }).click();
  await page.getByRole('option', { name: 'us' }).click();
  await dialog.getByRole('textbox', { name: 'Reason' }).fill('Pool exhausted');
  await dialog.getByRole('button', { name: 'Send for approval' }).click();

  await expect(page.getByText('Sent for approval; 1 approver needed')).toBeVisible();
  expect(requested[0]).toEqual({ actionId: 'act-restart', inputs: { region: 'us' }, reason: 'Pool exhausted' });

  // Someone else asked; whoever can approve sees it waiting and approves.
  const waiting = section.locator('[data-remediation-run="run-1"]');
  await expect(waiting).toContainText('Awaiting approval');
  await expect(waiting).toContainText('started by Kenji Watanabe');
  await waiting.getByRole('button', { name: 'Approve' }).click();
  await expect(page.getByText('Approved', { exact: true })).toBeVisible();
  expect(reviewed[0]).toMatchObject({ approve: true });
  await expect(waiting).toContainText('Awaiting check');
  await expect(waiting).toContainText('HTTP 201: queued');
});

test('a remediation action calls an address with a secret header and a JSON body', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const saved = await capture(page, /\/api\/v1\/alerts\/remediation\/actions$/, (body) => ({
    ...restart,
    ...body,
    id: 'act-new',
    secretsSet: ['header:Authorization'],
  }));

  await page.goto('/alerts/remediation');
  await page.getByRole('button', { name: 'New remediation action' }).click();
  const drawer = page.locator('[data-remediation-drawer]');
  await drawer.getByRole('textbox', { name: 'Name', exact: true }).fill('Restart checkout pods');
  await drawer.getByRole('textbox', { name: 'Address' }).fill('https://ci.example.com/job/restart?service={{incident.service}}');
  // The body is a JSON editor: a click in it focuses it, and the text goes in as one input. A placeholder in
  // quotes is a string; a bare one stands for a number.
  const body = drawer.getByRole('textbox', { name: 'Body', exact: true });
  await drawer.locator('.monaco-editor .view-lines').first().click();
  await expect(body).toBeFocused();
  await page.keyboard.insertText('{"service": "{{incident.service}}", "replicas": {{params.replicas}}}');
  await drawer.getByRole('button', { name: 'Add a header' }).click();
  await drawer.getByRole('textbox', { name: 'Header name' }).fill('Authorization');
  await drawer.getByLabel('Header value').fill('Bearer t0ken');
  await drawer.getByRole('textbox', { name: 'Services' }).fill('checkout, payments');
  await page.getByRole('radio', { name: 'L3 · two approvers' }).click();
  await page.getByRole('button', { name: 'Save' }).click();

  await expect.poll(() => saved.length).toBe(1);
  expect(saved[0]).toMatchObject({
    name: 'Restart checkout pods',
    target: {
      kind: 'http',
      method: 'POST',
      url: 'https://ci.example.com/job/restart?service={{incident.service}}',
      headers: { Authorization: 'Bearer t0ken' },
      body: '{"service": "{{incident.service}}", "replicas": {{params.replicas}}}',
    },
    services: ['checkout', 'payments'],
    risk: 'l3',
  });
});

test('a Jira connector makes a ticket for an incident', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const jira = {
    id: 'tc-jira',
    name: 'Ops Jira',
    system: 'jira',
    settings: {
      siteUrl: 'https://acme.atlassian.net',
      deployment: 'cloud',
      email: 'ops@acme.com',
      projectKey: 'OPS',
      issueType: 'Incident',
      priorities: {},
      labels: [],
      resolveTransition: '',
    },
    secretsSet: ['apiToken', 'webhookSecret'],
    comments: { acknowledged: true, resolved: true, notes: false },
    transitionOnResolve: true,
    resolveIncidentWhenDone: false,
    auto: { enabled: false, conditions: [], minSeverity: null },
    onMajorIncident: false,
    enabled: true,
    webhookPath: '/api/v1/public/tickets/jira/tc-jira',
    createdAt: NOW,
    updatedAt: NOW,
  };
  let connectors: unknown[] = [];
  await page.route(/\/api\/v1\/alerts\/tickets\/connectors$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: { connectors } }) : route.fallback(),
  );
  const saved = await capture(page, /\/api\/v1\/alerts\/tickets\/connectors$/, () => {
    connectors = [jira];
    return jira;
  });

  await page.goto('/alerts/tickets');
  await page.getByRole('button', { name: 'New ticket connector' }).click();
  const drawer = page.locator('[data-ticket-drawer]');
  await drawer.getByRole('textbox', { name: 'Name', exact: true }).fill('Ops Jira');
  await drawer.getByRole('textbox', { name: 'Site address' }).fill('https://acme.atlassian.net');
  await drawer.getByRole('textbox', { name: 'Account email' }).fill('ops@acme.com');
  await drawer.getByLabel('API token').fill('atl-123');
  await drawer.getByRole('textbox', { name: 'Project key' }).fill('OPS');
  await drawer.getByLabel('Webhook secret').fill('hook-secret');
  await page.getByRole('checkbox', { name: 'Move the ticket to done when the incident is resolved' }).click();
  await page.getByRole('button', { name: 'Save' }).click();

  await expect.poll(() => saved.length).toBe(1);
  expect(saved[0]).toMatchObject({
    name: 'Ops Jira',
    system: 'jira',
    settings: { siteUrl: 'https://acme.atlassian.net', email: 'ops@acme.com', projectKey: 'OPS', issueType: 'Incident' },
    secrets: { apiToken: 'atl-123', webhookSecret: 'hook-secret' },
    transitionOnResolve: true,
  });
  await expect(page.locator('[data-jira-webhook]')).toContainText('/api/v1/public/tickets/jira/tc-jira');

  let tickets: unknown[] = [];
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-2292\/tickets$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: { tickets } }) : route.fallback(),
  );
  const ticket = {
    id: 't-1',
    incidentId: 'inc-2292',
    connectorId: 'tc-jira',
    system: 'jira',
    externalId: '10042',
    externalKey: 'OPS-42',
    url: 'https://acme.atlassian.net/browse/OPS-42',
    status: 'To Do',
    done: false,
    createdBy: 'dev',
    createdAt: NOW,
    syncedAt: NOW,
    lastError: '',
  };
  const made = await capture(page, /\/api\/v1\/alerts\/incidents\/inc-2292\/tickets$/, () => {
    tickets = [ticket];
    return ticket;
  });

  await page.goto('/alerts/incidents/inc-2292');
  await page.locator('[data-incident-tickets]').getByRole('button', { name: 'Create in Ops Jira' }).click();
  await expect(page.getByText('Created OPS-42')).toBeVisible();
  expect(made[0]).toEqual({ connectorId: 'tc-jira' });
  await expect(page.locator('[data-incident-ticket="OPS-42"]').getByRole('link', { name: 'OPS-42' })).toHaveAttribute(
    'href',
    'https://acme.atlassian.net/browse/OPS-42',
  );
});

test('a Meegle connector picks its platform, which fixes the address, and names its plugin, space and work item type', async ({
  page,
  mockServer,
}) => {
  await mountMockRoutes(page, mockServer.port);
  const saved = await capture(page, /\/api\/v1\/alerts\/tickets\/connectors$/, (body) => ({
    ...body,
    id: 'tc-meegle',
    secretsSet: ['pluginSecret'],
    createdAt: NOW,
    updatedAt: NOW,
  }));

  await page.goto('/alerts/tickets');
  await page.getByRole('button', { name: 'New ticket connector' }).click();
  await page.getByRole('radio', { name: /Meegle/ }).click();
  const drawer = page.locator('[data-ticket-drawer]');
  await drawer.getByRole('textbox', { name: 'Name', exact: true }).fill('Feishu Project');
  // The address follows the platform and is shown to be copied, not typed.
  const address = drawer.locator('[data-meegle-address]');
  await expect(address).toContainText('https://project.feishu.cn');
  await expect(address.getByRole('textbox')).toHaveCount(0);
  await drawer.getByRole('combobox', { name: 'Platform' }).click();
  await page.getByRole('option', { name: 'Lark' }).click();
  await expect(address).toContainText('https://meegle.com');
  await drawer.getByRole('textbox', { name: 'Plugin ID' }).fill('MII_1234');
  await drawer.getByLabel('Plugin secret').fill('plugin-secret');
  await drawer.getByRole('textbox', { name: 'Acting as (user_key)' }).fill('7300000001');
  await drawer.getByRole('textbox', { name: 'Space (project_key)' }).fill('ops');
  await drawer.getByRole('textbox', { name: 'States that count as done' }).fill('CLOSED, RESOLVED');
  await page.getByRole('button', { name: 'Save' }).click();

  await expect.poll(() => saved.length).toBe(1);
  expect(saved[0]).toMatchObject({
    system: 'meegle',
    settings: {
      site: 'lark',
      pluginId: 'MII_1234',
      userKey: '7300000001',
      projectKey: 'ops',
      workItemTypeKey: 'issue',
      doneStates: ['CLOSED', 'RESOLVED'],
    },
    secrets: { pluginSecret: 'plugin-secret' },
  });
});

test('a ServiceNow connector signs in its chosen way and maps severities to impact and urgency', async ({
  page,
  mockServer,
}) => {
  await mountMockRoutes(page, mockServer.port);
  const saved = await capture(page, /\/api\/v1\/alerts\/tickets\/connectors$/, (body) => ({
    ...body,
    id: 'tc-snow',
    secretsSet: ['clientSecret'],
    createdAt: NOW,
    updatedAt: NOW,
  }));

  await page.goto('/alerts/tickets');
  await page.getByRole('button', { name: 'New ticket connector' }).click();
  await page.getByRole('radio', { name: /ServiceNow/ }).click();

  const drawer = page.locator('[data-ticket-drawer]');
  await drawer.getByRole('textbox', { name: 'Name', exact: true }).fill('Ops ServiceNow');
  await drawer.getByRole('textbox', { name: 'Instance address' }).fill('https://acme.service-now.com');
  await expect(drawer.getByRole('textbox', { name: 'Table' })).toHaveValue('incident');

  // Each way of signing in asks for its own secret.
  await expect(drawer.getByRole('textbox', { name: 'User name' })).toBeVisible();
  await drawer.getByRole('radio', { name: /OAuth client credentials/ }).click();
  await expect(drawer.getByRole('textbox', { name: 'User name' })).toHaveCount(0);
  await drawer.getByRole('textbox', { name: 'Client ID' }).fill('client-1');
  await drawer.getByLabel('Client secret').fill('shh');

  await drawer.getByRole('textbox', { name: 'Assignment group' }).fill('287ebd7da9fe198100f92cc8d1d2fbcd');
  await drawer.locator('[data-ticket-priorities]').getByRole('textbox', { name: 'Warning', exact: true }).fill('3');
  await drawer.getByRole('textbox', { name: 'Resolution code' }).fill('Solved (Permanently)');
  await page.getByRole('button', { name: 'Save' }).click();

  await expect.poll(() => saved.length).toBe(1);
  expect(saved[0]).toMatchObject({
    system: 'servicenow',
    settings: {
      instanceUrl: 'https://acme.service-now.com',
      auth: 'oauth',
      clientId: 'client-1',
      table: 'incident',
      assignmentGroup: '287ebd7da9fe198100f92cc8d1d2fbcd',
      priorities: { critical: '1/1', error: '2/1', warning: '3', info: '3/3' },
      resolveState: '6',
      closeCode: 'Solved (Permanently)',
      doneStates: ['6', '7', '8'],
    },
    secrets: { clientSecret: 'shh' },
  });
});
