/**
 * A rule's investigation guide: written in the rule workbench, shown on the
 * rule's incidents with its queries opened over the incident's time range.
 */
import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

/** Answers every preview query with two points, enough to save a rule. */
async function answerPreviews(page: Page): Promise<void> {
  await page.route(/\/api\/v1\/query(\?.*)?$/, (route) =>
    route.fulfill({
      json: {
        columns: ['_timestamp', 'value'],
        rows: [
          [1_700_000_000_000_000, 0.2],
          [1_700_000_060_000_000, 0.07],
        ],
        scannedRows: 2,
        tookMs: 3,
      },
    }),
  );
}

/** Keeps every rule the page creates, answering as the backend does. */
async function keepCreatedRules(page: Page): Promise<Array<Record<string, unknown>>> {
  const saved: Array<Record<string, unknown>> = [];
  await page.route(/\/api\/v1\/alerts\/rules$/, async (route) => {
    if (route.request().method() !== 'POST') return route.fallback();
    const body = route.request().postDataJSON() as Record<string, unknown>;
    saved.push(body);
    return route.fulfill({ json: { ...body, id: 'rule-1', orgId: 'default', escalationPolicyId: 'default' } });
  });
  return saved;
}

/** Tests the rule and creates it from the page head. */
async function createRule(page: Page): Promise<void> {
  await page.getByRole('button', { name: 'Test rule', exact: true }).click();
  await expect(page.locator('[data-alert-query-preview]')).toContainText('Current value');
  // Testing scrolled the page down to the preview; the save button is in the page head.
  await page.mouse.move(400, 400);
  await page.mouse.wheel(0, -10_000);
  await page.getByRole('button', { name: 'Create rule', exact: true }).click();
}

test('an incident shows its rule guide with the queries opened over its time range', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.goto('/alerts/incidents/inc-2291');

  const guide = page.locator('[data-incident-guide]');
  await expect(guide).toContainText('Check which routes return the 5xx.');
  await expect(guide).toContainText('Look for an edge proxy change in the last hour.');

  const promql = new URL(
    (await guide.getByRole('link', { name: /5xx by route/ }).getAttribute('href'))!,
    'https://molesignal.test',
  );
  expect(promql.pathname).toBe('/metrics');
  expect(promql.searchParams.get('promql')).toContain('sum by (route)');
  expect(promql.searchParams.get('time')).toMatch(/\.\./);

  const sql = new URL(
    (await guide.getByRole('link', { name: /Gateway error logs/ }).getAttribute('href'))!,
    'https://molesignal.test',
  );
  expect(sql.pathname).toBe('/logs');
  expect(sql.searchParams.get('stream')).toBe('app_logs');
  expect(sql.searchParams.get('sql')).toContain("level = 'ERROR'");

  await expect(guide.getByRole('link', { name: 'API latency' })).toHaveAttribute('href', '/dashboards/d2');
});

test('an incident of a rule without a guide shows none', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.goto('/alerts/incidents/inc-2292');

  await expect(page.locator('[data-incident-section]').first()).toBeVisible();
  await expect(page.locator('[data-incident-guide]')).toHaveCount(0);
});

test('a rule is saved with the steps, queries and dashboards of its guide', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await answerPreviews(page);
  const saved = await keepCreatedRules(page);

  await page.goto('/alerts/rules/new');
  await page.getByLabel('Rule name').fill('Error rate');

  const guide = page.locator('[data-rule-guide]');
  await guide.getByRole('textbox', { name: 'Steps', exact: true }).click();
  await page.keyboard.type('Check the gateway first.');

  await guide.getByRole('button', { name: 'Add query', exact: true }).click();
  const query = guide.locator('[data-guide-query]');
  await query.getByLabel('Name').fill('Errors by route');
  const editor = query.locator('.code-editor-shell');
  await editor.locator('.view-lines').click();
  await expect(editor.locator('.monaco-editor')).toHaveClass(/\bfocused\b/);
  await page.keyboard.insertText('sum(rate(http_requests_total[5m]))');

  await guide.getByRole('combobox', { name: 'Add a dashboard' }).click();
  await page.getByRole('option', { name: 'API latency' }).click();
  await expect(guide.locator('[data-guide-dashboard]')).toHaveText('API latency');

  await createRule(page);

  await expect.poll(() => saved.length).toBe(1);
  expect(saved[0]).toMatchObject({
    guide: {
      steps: 'Check the gateway first.',
      queries: [{ name: 'Errors by route', language: 'promql', statement: 'sum(rate(http_requests_total[5m]))' }],
      dashboards: ['d2'],
    },
  });
});

test('a SQL query of a guide has to name the stream it reads', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await answerPreviews(page);
  const saved = await keepCreatedRules(page);

  await page.goto('/alerts/rules/new');
  await page.getByLabel('Rule name').fill('Error rate');

  const guide = page.locator('[data-rule-guide]');
  await guide.getByRole('button', { name: 'Add query', exact: true }).click();
  const query = guide.locator('[data-guide-query]');
  await query.getByLabel('Name').fill('Gateway errors');
  await query.getByRole('button', { name: 'sql', exact: true }).click();
  await expect(query.getByLabel('Stream type')).toBeVisible();
  const editor = query.locator('.code-editor-shell');
  await editor.locator('.view-lines').click();
  await expect(editor.locator('.monaco-editor')).toHaveClass(/\bfocused\b/);
  await page.keyboard.insertText('SELECT * FROM app_logs');

  await createRule(page);

  await expect(page.getByText('Query 1 is SQL: name the stream it reads.')).toBeVisible();
  expect(saved).toHaveLength(0);
});
