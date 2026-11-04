/**
 * The ways a rule fires besides a threshold: a composite of other rules, a heartbeat that waits for
 * check-ins, a forecast of the query's trend, and groups of a split query compared with each other.
 */
import type { Page } from '@playwright/test';

import { FROZEN_NOW_ISO, expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1000;

/** Saves are answered with what was sent; `extra` is added to the reply (a heartbeat rule's address). */
async function captureSaves(page: Page, extra: Record<string, unknown> = {}) {
  const saved: Array<Record<string, unknown>> = [];
  await page.route(/\/api\/v1\/alerts\/rules$/, async (route) => {
    if (route.request().method() !== 'POST') return route.fallback();
    const body = route.request().postDataJSON() as Record<string, unknown>;
    saved.push(body);
    return route.fulfill({ json: { ...body, id: 'rule-new', orgId: 'acme-prod', escalationPolicyId: 'default', ...extra } });
  });
  return saved;
}

async function createRule(page: Page) {
  await page.mouse.move(400, 400);
  await page.mouse.wheel(0, -10_000);
  await page.getByRole('button', { name: 'Create rule', exact: true }).click();
}

test('a composite rule combines other rules by letter', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const saved = await captureSaves(page);
  const rules = await (await page.request.get(`http://127.0.0.1:${mockServer.port}/api/v1/alerts/rules`)).json();
  const [first, second] = rules as Array<{ id: string; name: string }>;

  await page.goto('/alerts/rules/new');
  await page.getByLabel('Rule name').fill('Checkout down and payments slow');
  await page.getByRole('radio', { name: 'Composite' }).click();

  // The query, its source and thresholds are not this rule's: it reads other rules.
  await expect(page.getByRole('button', { name: 'Test rule', exact: true })).toHaveCount(0);
  await expect(page.locator('[data-composite-problem]')).toContainText('Choose a rule for A, B in the expression.');

  await page.getByLabel('Expression').fill('A && !B');
  await page.getByRole('combobox', { name: 'Rule A' }).click();
  await page.getByRole('option', { name: first!.name }).click();
  await page.getByRole('combobox', { name: 'Rule B' }).click();
  await page.getByRole('option', { name: second!.name }).click();
  await expect(page.locator('[data-composite-problem]')).toHaveCount(0);
  await page.getByRole('switch', { name: "Merge these rules' incidents under the composite incident" }).click();

  await createRule(page);
  await expect.poll(() => saved.length).toBe(1);
  expect(saved[0]).toMatchObject({
    kind: 'composite',
    query: { composite: { expression: 'A && !B', rules: { A: first!.id, B: second!.id }, mergeChildren: true } },
    thresholds: [],
    severity: 'warning',
  });
  expect((saved[0]!.trigger as Record<string, unknown>).noData).toBeNull();
});

test('a heartbeat rule shows its check-in address once after saving', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const saved = await captureSaves(page, {
    heartbeatAddress: { path: '/api/v1/intake/heartbeats/9f1c2b3a', url: null },
  });

  await page.goto('/alerts/rules/new');
  await page.getByLabel('Rule name').fill('Nightly backup');
  await page.getByRole('radio', { name: 'Heartbeat' }).click();
  await expect(page.getByText('The check-in address is shown once, after saving.')).toBeVisible();
  await page.getByLabel('Check-in period').fill('1440');
  await page.getByLabel('Grace').fill('30');

  await createRule(page);
  await expect.poll(() => saved.length).toBe(1);
  expect(saved[0]).toMatchObject({
    kind: 'heartbeat',
    query: { heartbeat: { periodSecs: 86_400, graceSecs: 1_800 } },
    trigger: { forPeriods: 1 },
  });

  const dialog = page.getByRole('dialog', { name: 'Check-in address' });
  const origin = new URL(page.url()).origin;
  await expect(dialog.locator('[data-heartbeat-address]')).toContainText(`${origin}/api/v1/intake/heartbeats/9f1c2b3a`);
  await expect(dialog).toContainText('?status=fail');
  // The footer's button, not the corner's.
  await dialog.getByRole('button', { name: 'Close' }).last().click();
  await expect(page).toHaveURL(/\/alerts\/rules$/);
});

test('a saved heartbeat rule shows its last check-in and gives a new address', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  const rule = {
    id: 'rule-beat',
    orgId: 'acme-prod',
    name: 'Nightly backup',
    description: '',
    enabled: true,
    kind: 'heartbeat',
    query: { language: 'sql', statement: 'heartbeat', periodSecs: 60, stream: null, heartbeat: { periodSecs: 86_400, graceSecs: 1_800 } },
    trigger: { operator: 'gt', threshold: 0, forPeriods: 1, silenceSecs: 300 },
    thresholds: [],
    severity: 'error',
    escalationPolicyId: 'default',
    labels: {},
    annotations: {},
  };
  await page.route(/\/api\/v1\/alerts\/rules\/rule-beat$/, (route) => route.fulfill({ json: rule }));
  await page.route(/\/api\/v1\/alerts\/rules\/rule-beat\/heartbeat$/, (route) =>
    route.fulfill({
      json: {
        lastCheckinAt: NOW - 3_600_000_000,
        lastStatus: 'fail',
        lastMessage: 'disk full',
        dueAt: NOW + 22 * 3_600_000_000,
        alarming: true,
        issuedAt: NOW - 30 * 86_400_000_000,
      },
    }),
  );
  await page.route(/\/api\/v1\/alerts\/rules\/rule-beat\/heartbeat-token$/, (route) =>
    route.fulfill({ json: { path: '/api/v1/intake/heartbeats/reset-1', url: 'https://ms.example.com/api/v1/intake/heartbeats/reset-1' } }),
  );

  await page.goto('/alerts/rules/rule-beat/edit');
  const status = page.locator('[data-heartbeat-status]');
  await expect(status).toContainText('Reported a failure');
  await expect(status).toContainText('Next check-in due by');

  await status.getByRole('button', { name: 'Reset check-in address' }).click();
  const dialog = page.getByRole('dialog', { name: 'Check-in address' });
  await expect(dialog).toContainText('https://ms.example.com/api/v1/intake/heartbeats/reset-1');
  // The footer's button, not the corner's.
  await dialog.getByRole('button', { name: 'Close' }).last().click();
  await expect(page).toHaveURL(/\/alerts\/rules\/rule-beat\/edit$/);
});

test('a forecast rule carries the trend to a line within its lead time', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.route(/\/api\/v1\/query(\?.*)?$/, (route) =>
    route.fulfill({ json: { columns: ['value'], rows: [[61], [64], [70]], scannedRows: 3, tookMs: 2 } }),
  );
  const saved = await captureSaves(page);

  await page.goto('/alerts/rules/new');
  await page.getByLabel('Rule name').fill('Disk fills within two days');
  await page.getByRole('radio', { name: 'Forecast' }).click();
  await page.locator('[data-forecast]').getByRole('spinbutton').fill('95');
  await page.getByRole('combobox', { name: 'Lead time' }).click();
  await page.getByRole('option', { name: '2 days' }).click();
  await page.getByRole('combobox', { name: 'Model' }).click();
  await page.getByRole('option', { name: 'Weekly pattern' }).click();
  await page.getByRole('button', { name: 'Test rule', exact: true }).click();

  await createRule(page);
  await expect.poll(() => saved.length).toBe(1);
  expect(saved[0]).toMatchObject({
    kind: 'forecast',
    query: { forecast: { leadSecs: 172_800, lookbackDays: 7, model: 'weekly' } },
    trigger: { operator: 'gte', threshold: 95 },
    thresholds: [],
  });
});

test('an outlier rule compares the groups of a split query', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.route(/\/api\/v1\/query(\?.*)?$/, (route) =>
    route.fulfill({
      json: { columns: ['host', 'p99'], rows: [['web-1', 120], ['web-2', 118], ['web-3', 410]], scannedRows: 3, tookMs: 2 },
    }),
  );
  const saved = await captureSaves(page);

  await page.goto('/alerts/rules/new');
  await page.getByLabel('Rule name').fill('One host slower than the rest');
  await page.getByRole('radio', { name: 'Outlier' }).click();

  // Figures of APM and RUM watch one scope; there are no groups to compare.
  await expect(page.getByRole('button', { name: 'APM' })).toHaveCount(0);
  await page.getByLabel('Split by').fill('host');
  await page.getByLabel('Tolerance').fill('2.5');
  await page.getByRole('combobox', { name: 'Which side' }).click();
  await page.getByRole('option', { name: 'Higher only' }).click();
  await page.getByRole('button', { name: 'Test rule', exact: true }).click();
  await expect(page.locator('[data-alert-query-preview]')).toContainText('3 groups by host');

  await createRule(page);
  await expect.poll(() => saved.length).toBe(1);
  expect(saved[0]).toMatchObject({
    kind: 'outlier',
    query: { groupBy: ['host'], outlier: { tolerance: 2.5, direction: 'above' } },
    thresholds: [],
  });
});
