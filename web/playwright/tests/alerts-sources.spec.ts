import type { Locator } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW = Date.now() * 1000;
const MINUTE = 60_000_000;

// What the server answers for an Alertmanager source: never its token's hash, only the prefix.
const source = {
  id: 'src-1',
  name: 'Production Prometheus',
  kind: 'alertmanager',
  enabled: true,
  escalationPolicyId: 'default',
  severity: 'warning',
  labels: { env: 'prod' },
  tokenPrefix: 'mspat-c16jb8m5zr',
  homeCluster: '',
  intakePath: '/api/v1/intake/alerts',
  createdBy: 'u1',
  createdAt: NOW - 60 * MINUTE,
  updatedAt: NOW - 60 * MINUTE,
  lastIntakeAt: NOW - MINUTE,
};

// The whole token: in the answer that made it, and nowhere else.
const TOKEN = 'mspat-c16jb8m5zrzf174p8g8b7m5p4jz0kp9y1hxr';

const DEDUP = 'http://alertmanager.example.com:9093|{}:{alertname="HighCPU"}';

const requests = [
  {
    id: 'req-2',
    receivedAt: NOW - MINUTE,
    viaCluster: '',
    status: 'accepted',
    httpStatus: 200,
    message: null,
    alertCount: 1,
    alerts: [
      { action: 'trigger', dedupKey: DEDUP, summary: 'HighCPU', severity: 'critical', outcome: 'opened', incidentId: 'inc-ext' },
    ],
    bodyExcerpt: null,
  },
  {
    id: 'req-1',
    receivedAt: NOW - 2 * MINUTE,
    viaCluster: '',
    status: 'rejected',
    httpStatus: 400,
    message: 'not a Alertmanager notification: expected value at line 1 column 1',
    alertCount: 0,
    alerts: [],
    bodyExcerpt: 'not json',
  },
];

// An incident the source opened: its `rule_id` is the source's.
const incident = {
  id: 'inc-ext',
  orgId: 'acme-prod',
  ruleId: 'src-1',
  escalationPolicyId: 'esc-platform',
  status: 'open',
  severity: 'critical',
  summary: 'HighCPU',
  fingerprint: 'fp-ext',
  currentStep: 0,
  currentLoop: 0,
  currentStepStartedAt: NOW - 10 * MINUTE,
  assignees: [],
  labels: { 'alert.source': 'external', 'external.kind': 'alertmanager', alertname: 'HighCPU', job: 'node', env: 'prod' },
  annotations: { alertmanagerUrl: 'http://alertmanager.example.com:9093' },
  traceIds: [],
  hostIds: [],
  affectedServices: [],
  triggeringQuery: null,
  createdAt: NOW - 10 * MINUTE,
  acknowledgedAt: null,
  acknowledgedBy: null,
  resolvedAt: null,
  resolvedBy: null,
};

/** The part is drawn inside the dialog: a token too long for one line wraps instead of widening it. */
async function expectWithin(dialog: Locator, part: Locator) {
  const outer = await dialog.boundingBox();
  const inner = await part.boundingBox();
  if (!outer || !inner) throw new Error('the dialog or its part is not drawn');

  expect(inner.x).toBeGreaterThanOrEqual(outer.x);
  expect(inner.x + inner.width).toBeLessThanOrEqual(outer.x + outer.width);
}

const incidentSource = {
  source,
  dedupKey: DEDUP,
  lastEventAt: NOW - 5 * MINUTE,
  lastIntakeAt: NOW - MINUTE,
  members: [
    {
      status: 'firing',
      labels: { alertname: 'HighCPU', instance: 'web-1:9100', job: 'node' },
      summary: 'CPU above 90% on web-1',
      startsAt: NOW - 10 * MINUTE,
      url: 'http://prometheus.example.com:9090/graph',
    },
    {
      status: 'firing',
      labels: { alertname: 'HighCPU', instance: 'web-2:9100', job: 'node' },
      summary: 'CPU above 90% on web-2',
      startsAt: NOW - 5 * MINUTE,
    },
  ],
};

test('a new alert source shows its token once, written into the setup it goes in', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  let created: Record<string, unknown> | null = null;
  await page.route(/\/api\/v1\/alerts\/sources$/, async (route) => {
    if (route.request().method() === 'POST') {
      created = route.request().postDataJSON() as Record<string, unknown>;
      return route.fulfill({ json: { ...source, token: TOKEN } });
    }
    return route.fulfill({ json: created ? [source] : [] });
  });
  await page.route(/\/api\/v1\/alerts\/sources\/src-1$/, (route) => route.fulfill({ json: source }));
  await page.route(/\/api\/v1\/alerts\/sources\/src-1\/requests(\?.*)?$/, (route) =>
    route.fulfill({ json: requests }),
  );

  await page.goto('/alerts/sources');
  await expect(page.getByText('No alert sources yet')).toBeVisible();

  await page.getByRole('button', { name: 'New alert source' }).first().click();
  await page.getByRole('textbox', { name: 'Name', exact: true }).fill('Production Prometheus');
  await page.getByRole('button', { name: 'Create', exact: true }).click();

  await expect(page).toHaveURL(/\/alerts\/sources\/src-1$/);
  expect(created).toMatchObject({
    kind: 'alertmanager',
    name: 'Production Prometheus',
    escalationPolicyId: 'default',
    severity: 'warning',
  });

  // The token, once, in the setup ready to paste.
  const dialog = page.getByRole('dialog');
  await expect(dialog).toContainText(TOKEN);
  await expect(dialog.locator('[data-source-snippet]')).toContainText(`credentials: ${TOKEN}`);
  await expectWithin(dialog, dialog.locator('[data-source-snippet]'));
  await expectWithin(dialog, dialog.getByRole('button', { name: 'Done' }));
  await dialog.getByRole('button', { name: 'Done' }).click();
  await expect(dialog).toBeHidden();

  // The source is receiving, so its setup steps are checked.
  await expect(page.locator('[data-source-standing]')).toHaveAttribute('data-source-standing', 'receiving');
  await expect(page.locator('[data-setup-step][data-state="done"]')).toHaveCount(3);

  // The page says where the token goes, never what it is.
  await expect(page.locator('[data-source-snippet]')).toContainText('credentials: <TOKEN>');
  await expect(page.getByText(TOKEN)).toHaveCount(0);
  await expect(page.locator('code', { hasText: '/api/v1/intake/alerts' })).toBeVisible();

  // What the source received, the latest first; a refused request opens to its body.
  const rows = page.locator('[data-source-requests] tbody tr');
  await expect(rows).toHaveCount(2);
  await expect(rows.first()).toContainText('Handled');
  await expect(rows.first()).toContainText('Opened 1');
  await expect(rows.nth(1)).toContainText('Refused');
  await rows.nth(1).click();
  await expect(page.locator('[data-source-requests]')).toContainText('not json');

  // Coming back to the page does not bring the token back.
  await page.reload();
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Production Prometheus');
  await expect(page.getByRole('dialog')).toHaveCount(0);
});

test('an Aliyun source is set up by its address alone, the token in it', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  const aliyun = { ...source, id: 'src-2', name: 'Aliyun production', kind: 'aliyun_cms', lastIntakeAt: null };
  await page.route(/\/api\/v1\/alerts\/sources\/src-2$/, (route) => route.fulfill({ json: aliyun }));
  await page.route(/\/api\/v1\/alerts\/sources\/src-2\/requests(\?.*)?$/, (route) => route.fulfill({ json: [] }));
  await page.route(/\/api\/v1\/alerts\/sources\/src-2\/token$/, (route) =>
    route.fulfill({ json: { ...aliyun, token: TOKEN } }),
  );

  await page.goto('/alerts/sources/src-2');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Aliyun production');

  // Nothing received yet: the page waits for the sender, and lists no requests.
  await expect(page.locator('[data-source-standing]')).toHaveAttribute('data-source-standing', 'waiting');
  await expect(page.locator('[data-setup-step][data-state="todo"]')).toHaveCount(3);
  await expect(page.locator('[data-source-requests]')).toHaveCount(0);

  // The address says where the token goes; no setup repeats it.
  await expect(page.locator('code', { hasText: '/api/v1/intake/alerts?token=<TOKEN>' })).toBeVisible();
  await expect(page.getByText('Replace <TOKEN> with the token of this source.')).toBeVisible();
  await expect(page.locator('[data-source-snippet]')).toHaveCount(0);

  await page.getByTestId('alert-source-rotate').click();
  await page.getByRole('dialog').getByRole('button', { name: 'Rotate token' }).click();

  // The new token and the address with it, whole and inside the dialog.
  const dialog = page.getByRole('dialog').filter({ has: page.locator('[data-source-token]') });
  const address = dialog.locator('[data-source-snippet]');
  await expect(address).toHaveText(new RegExp(`/api/v1/intake/alerts\\?token=${TOKEN}$`));
  await expectWithin(dialog, address);
  await expectWithin(dialog, dialog.getByRole('button', { name: 'Done' }));
});

test('an incident an alert source opened names the source and lists its group, not a rule', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  const ruleReads: string[] = [];
  page.on('request', (request) => {
    if (request.url().includes('/api/v1/alerts/rules/src-1')) ruleReads.push(request.url());
  });
  await page.route(/\/api\/v1\/alerts\/incidents(\?.*)?$/, (route) => route.fulfill({ json: [incident] }));
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-ext$/, (route) => route.fulfill({ json: incident }));
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-ext\/source$/, (route) => route.fulfill({ json: incidentSource }));
  await page.route(/\/api\/v1\/alerts\/sources$/, (route) => route.fulfill({ json: [source] }));

  await page.goto('/alerts/incidents');
  const row = page.locator('tbody tr').first();
  await expect(row).toContainText('Alertmanager');
  await expect(row).toContainText('Production Prometheus');

  await page.goto('/alerts/incidents/inc-ext');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('HighCPU');
  await expect(page.getByText('Source', { exact: true }).first()).toBeVisible();
  await expect(page.locator('a[href="/alerts/sources/src-1"]').first()).toHaveText('Production Prometheus');

  // The group the incident stands for, each alert by what tells it apart.
  const members = page.locator('[data-incident-members] li');
  await expect(members).toHaveCount(2);
  await expect(members.first()).toContainText('instance=web-1:9100');
  await expect(members.first()).toContainText('Firing');

  // There is no rule behind it to read.
  expect(ruleReads).toEqual([]);
});
