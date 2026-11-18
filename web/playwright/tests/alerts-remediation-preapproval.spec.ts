/**
 * Pre-approval of remediation actions: the organization turns it on, people who may approve say
 * beforehand that an action may run on some services' incidents a few times in a window, an L3
 * action waits for a second person, and a request it covers runs at once.
 */
import type { Page } from '@playwright/test';

import { FROZEN_NOW_ISO, expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1000;

const restart = {
  id: 'act-restart',
  name: 'Restart checkout pods',
  description: '',
  target: { kind: 'http', method: 'POST', url: 'https://ci.example.com/job/restart', headers: {}, body: '' },
  parameters: [],
  services: ['checkout'],
  teamIds: [],
  risk: 'l2',
  verifyAfterSecs: 0,
  verifyBy: 'incident_resolved',
  enabled: true,
  secretsSet: [],
  preapproval: null as unknown,
  createdAt: NOW - 86_400_000_000,
  updatedAt: NOW - 86_400_000_000,
};

const feishu = {
  id: 'feishu-1',
  organizationId: 'org-1',
  name: 'Ops Feishu',
  connectorType: 'lark_app',
  config: {},
  capabilities: { group: true },
  enabled: true,
  status: 'connected',
  createdAt: NOW,
  updatedAt: NOW,
};

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

test('an organization turns pre-approval on and pre-approves an action', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  let action: typeof restart = { ...restart };
  let overview: Record<string, unknown> = { enabled: false, actions: {} };

  await page.route(/\/api\/v1\/alerts\/remediation\/actions$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: { actions: [action] } }) : route.fallback(),
  );
  await page.route(/\/api\/v1\/notify\/connectors$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: [feishu] }) : route.fallback(),
  );
  await page.route(/\/api\/v1\/alerts\/remediation\/preapproval$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: overview }) : route.fallback(),
  );
  const switched = await capture(page, /\/api\/v1\/alerts\/remediation\/preapproval$/, (body) => {
    overview = { ...overview, enabled: body.enabled };
    return { enabled: body.enabled };
  });
  const set = await capture(page, /\/api\/v1\/alerts\/remediation\/actions\/act-restart\/preapproval$/, (body) => {
    const preapproval = { ...body, approvers: [{ userId: 'test-user', at: NOW }], setBy: 'test-user', setAt: NOW };
    action = { ...action, preapproval };
    overview = { ...overview, actions: { 'act-restart': { used: 0, state: 'active' } } };
    return { preapproval };
  });

  await page.goto('/alerts/remediation');
  const section = page.locator('[data-remediation-preapproval]');
  const toggle = section.getByRole('switch', { name: 'Let pre-approved actions run without waiting' });
  await expect(toggle).not.toBeChecked();
  await toggle.click();
  await expect.poll(() => switched.length).toBe(1);
  expect(switched[0]).toEqual({ enabled: true });
  await expect(page.getByText('Pre-approval turned on')).toBeVisible();

  const row = page.locator('[data-remediation-action="act-restart"]');
  await expect(row.locator('[data-preapproval-state="none"]')).toContainText('Not pre-approved');
  await row.getByRole('button', { name: 'Pre-approve' }).click();

  const dialog = page.locator('[data-preapproval-dialog]');
  await expect(dialog).toContainText('Saving counts as the one approval an L2 action needs');
  await expect(dialog.getByRole('textbox', { name: 'Services' })).toHaveValue('checkout');
  await dialog.getByRole('spinbutton', { name: 'Runs at most' }).fill('2');
  await dialog.getByRole('button', { name: 'Add a channel' }).click();
  await dialog.getByRole('textbox', { name: 'Group or address of channel 1' }).fill('oc_ops');
  await dialog.getByRole('button', { name: 'Save pre-approval' }).click();

  await expect.poll(() => set.length).toBe(1);
  expect(set[0]).toMatchObject({
    services: ['checkout'],
    maxRuns: 2,
    windowSecs: 3600,
    channels: [{ connectorId: 'feishu-1', targetType: 'fixed_group', target: 'oc_ops' }],
  });
  expect(typeof set[0]?.expiresAt).toBe('number');
  await expect(row.locator('[data-preapproval-state="active"]')).toContainText('0/2 runs in the window');
});

test('an L3 pre-approval waits for a second person to confirm it', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const preapproval = {
    services: ['checkout'],
    maxRuns: 1,
    windowSecs: 3600,
    expiresAt: NOW + 86_400_000_000,
    channels: [],
    approvers: [{ userId: 'u-other', at: NOW }],
    setBy: 'u-other',
    setAt: NOW,
  };
  let action = { ...restart, risk: 'l3', preapproval };
  let overview: Record<string, unknown> = {
    enabled: true,
    actions: { 'act-restart': { used: 0, state: 'awaiting_approver' } },
  };

  await page.route(/\/api\/v1\/alerts\/remediation\/actions$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: { actions: [action] } }) : route.fallback(),
  );
  await page.route(/\/api\/v1\/alerts\/remediation\/preapproval$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: overview }) : route.fallback(),
  );
  const confirmed = await capture(
    page,
    /\/api\/v1\/alerts\/remediation\/actions\/act-restart\/preapproval\/confirm$/,
    () => {
      const both = { ...preapproval, approvers: [...preapproval.approvers, { userId: 'test-user', at: NOW }] };
      action = { ...action, preapproval: both };
      overview = { enabled: true, actions: { 'act-restart': { used: 0, state: 'active' } } };
      return { preapproval: both };
    },
  );

  await page.goto('/alerts/remediation');
  const row = page.locator('[data-remediation-action="act-restart"]');
  await expect(row.locator('[data-preapproval-state="awaiting_approver"]')).toContainText(
    'waits for a second person',
  );
  await row.getByRole('button', { name: 'Confirm pre-approval' }).click();

  await expect.poll(() => confirmed.length).toBe(1);
  await expect(page.getByText('Pre-approval confirmed')).toBeVisible();
  await expect(row.locator('[data-preapproval-state="active"]')).toBeVisible();
  await expect(row.getByRole('button', { name: 'Confirm pre-approval' })).toHaveCount(0);
});

test('a request a pre-approval covers runs at once', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const ran = {
    id: 'run-1',
    incidentId: 'inc-2292',
    actionId: 'act-restart',
    actionName: 'Restart checkout pods',
    approvalId: 'appr-1',
    inputs: {},
    status: 'succeeded',
    output: 'HTTP 201: queued',
    verifyAt: null,
    verification: '',
    requestedBy: 'test-user',
    executedBy: 'test-user',
    preapproved: true,
    createdAt: NOW - 60_000_000,
    updatedAt: NOW - 60_000_000,
  };
  let runs: unknown[] = [];

  await page.route(/\/api\/v1\/alerts\/incidents\/inc-2292\/remediations$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: { actions: [restart], runs } }) : route.fallback(),
  );
  const requested = await capture(page, /\/api\/v1\/alerts\/incidents\/inc-2292\/remediations$/, () => {
    runs = [ran];
    return { run: ran, approval: { id: 'appr-1', requiredApprovals: 1 }, preapproved: true };
  });

  await page.goto('/alerts/incidents/inc-2292');
  const section = page.locator('[data-incident-remediation]');
  await section.locator('[data-remediation-action="act-restart"]').getByRole('button', { name: 'Run' }).click();
  const dialog = page.getByRole('dialog', { name: 'Run “Restart checkout pods”' });
  await dialog.getByRole('textbox', { name: 'Reason' }).fill('Pool exhausted');
  await dialog.getByRole('button', { name: 'Send for approval' }).click();

  await expect.poll(() => requested.length).toBe(1);
  await expect(page.getByText('Ran at once under the pre-approval')).toBeVisible();
  const run = section.locator('[data-remediation-run="run-1"]');
  await expect(run.locator('[data-remediation-preapproved]')).toHaveText('Pre-approved');
  await expect(run).toContainText('HTTP 201: queued');
});
