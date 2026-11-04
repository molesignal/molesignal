import type { Page } from '@playwright/test';

import { type Captured, installApmRoutes, SERVICE_PATH } from '../fixtures/mockApm';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

async function boot(page: Page, port: number): Promise<Captured> {
  await mountMockRoutes(page, port);
  return installApmRoutes(page);
}

test('an endpoint says how it answered and where its time goes', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto(`${SERVICE_PATH.replace('?', '/endpoints?')}&endpoint=${encodeURIComponent('POST /orders')}&kind=http`);

  const drawer = page.locator('[data-apm-drawer="endpoint"]');
  const codes = drawer.locator('[data-apm-card="endpoint-status-codes"]');
  await expect(codes.locator('[data-apm-status]')).toHaveCount(3);
  await expect(codes.locator('[data-apm-status="http:201"]')).toContainText('201');
  await expect(codes.locator('[data-apm-status="http:503"]')).toContainText('18');

  // The parts of the time, the largest first, each with its share and its time a request.
  const spent = drawer.locator('[data-apm-card="endpoint-time-spent"]');
  await expect(spent).toContainText('From the latest 120 requests in the trace store');
  await expect(spent.locator('[data-apm-time-part]').first()).toHaveAttribute('data-apm-time-part', 'database');
  await expect(spent.locator('[data-apm-time-part="database"]')).toContainText('50%');
  await expect(spent.locator('[data-apm-time-part="database"]')).toContainText('255 ms a request');
});

test('long tasks are an endpoint filter, and spans of their own are named workers', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto(SERVICE_PATH.replace('?', '/endpoints?'));

  const long = page.locator('[data-apm-endpoints-long-tasks]');
  await expect(long).toHaveText('Long tasks 1');
  await long.click();
  await expect(long).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('[data-apm-endpoint-row="POST /orders"]')).toBeVisible();

  await page.getByRole('combobox', { name: 'Kind' }).click();
  await expect(page.getByRole('option', { name: 'Workers' })).toBeVisible();
});

test('each version is told against the one before it, and a verdict opens the two in the comparison', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto(SERVICE_PATH.replace('?', '/deployments?'));

  const newer = page.locator('[data-apm-deployment="2.0.0"]');
  await expect(newer).toContainText('Current');
  await expect(newer.locator('[data-apm-verdict="regressed"]')).toContainText('Regressed');
  await expect(newer.locator('[data-apm-verdict="regressed"]')).toContainText('Error rate +1.73%');
  await expect(newer).toContainText('510 ms');
  // The oldest has nothing before it.
  await expect(page.locator('[data-apm-deployment="1.0.0"] [data-apm-verdict]')).toHaveCount(0);

  await newer.locator('[data-apm-compare-pair="1.0.0:2.0.0"]').click();
  await expect(page.locator('[data-apm-card="compare"]')).toContainText('Regressed');

  // The overview flags it among the deployments.
  await page.goto('/apm/overview');
  await expect(page.locator('[data-apm-deployment="2.0.0"] [data-apm-verdict="regressed"]')).toBeVisible();
});

test('an error group says where it happened, opens its logs, and can be given to a team under investigation', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto('/apm/errors');

  const listed = page.locator('[data-apm-card="errors-breakdown"]');
  await expect(listed.locator('[data-apm-error-breakdown="versions"] [data-apm-error-share="2.0.0"]')).toContainText('18');
  await expect(listed.locator('[data-apm-error-breakdown="languages"]')).toContainText('Rust');

  await page.locator('[data-apm-error-row="fp-checkout"]').click();
  const drawer = page.locator('[data-apm-drawer="error"]');
  await expect(drawer.locator('[data-apm-card="error-breakdown"] [data-apm-error-share="prod"]')).toContainText('24');
  await expect(drawer.locator('[data-apm-error-logs]')).toHaveAttribute('href', /^\/logs\?service=checkout&namespace=shop&environment=prod/);
  await expect(drawer.locator('[data-apm-error-sample-logs]')).toHaveAttribute('href', /^\/logs\?traceId=0123456789abcdef0123456789abcdef&/);

  await drawer.locator('[data-apm-issue-status]').click();
  await page.getByRole('option', { name: 'Investigating' }).click();
  await expect.poll(() => captured.issues.length).toBe(1);
  expect(captured.issues[0]).toMatchObject({ status: 'investigating', teamId: null });

  await drawer.locator('[data-apm-issue-team]').click();
  await page.getByRole('option', { name: 'Payments' }).click();
  await expect.poll(() => captured.issues.length).toBe(2);
  expect(captured.issues[1]).toMatchObject({ teamId: 'team-pay' });
});

test('error grouping rules are set, ordered and deleted beside the errors they group', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto('/apm/errors');

  await page.locator('[data-apm-error-rules-open]').click();
  const sheet = page.locator('[data-apm-error-rules]');
  await expect(sheet.locator('[data-apm-error-rule="error-rule-1"]')).toContainText('Payment timeouts');
  await expect(sheet.locator('[data-apm-error-rule="error-rule-1"]')).toContainText('Merge');
  await expect(sheet.locator('[data-apm-error-rule="error-rule-1"]')).toContainText('type *Timeout*');

  await sheet.locator('[data-apm-error-rule-new]').click();
  await sheet.locator('[data-apm-error-rule-name]').fill('Declines');
  await expect(sheet.locator('[data-apm-error-rule-save]')).toBeDisabled();
  await sheet.locator('[data-apm-error-rule-pattern="error_type"]').fill('payment*');
  // The listed group is like what the rule takes.
  await expect(sheet.locator('[data-apm-error-rule-preview]')).toHaveAttribute('data-apm-error-rule-preview', '1');
  await sheet.locator('[data-apm-error-rule-save]').click();
  await expect.poll(() => captured.errorRules.length).toBe(1);
  expect(captured.errorRules[0]).toMatchObject({ name: 'Declines', errorType: 'payment*', action: 'merge', enabled: true, serviceName: '' });

  // The new rule is tried last; moved up, it is tried first.
  const added = sheet.locator('[data-apm-error-rule="error-rule-2"]');
  await expect(sheet.locator('[data-apm-error-rule]').nth(1)).toHaveAttribute('data-apm-error-rule', 'error-rule-2');
  await added.getByRole('button', { name: 'Move Declines up' }).click();
  await expect.poll(() => captured.errorRules.length).toBe(2);
  expect(captured.errorRules[1]).toEqual({ ids: ['error-rule-2', 'error-rule-1'] });
  await expect(sheet.locator('[data-apm-error-rule]').first()).toHaveAttribute('data-apm-error-rule', 'error-rule-2');

  await added.getByRole('button', { name: 'Delete Declines' }).click();
  await page.getByRole('button', { name: 'Delete', exact: true }).click();
  await expect.poll(() => captured.removed).toEqual(['error-rule:error-rule-2']);
  await expect(sheet.locator('[data-apm-error-rule]')).toHaveCount(1);
});

test('a rule starts from an error group with its type and service', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto('/apm/errors?error=fp-checkout');

  await page.locator('[data-apm-drawer="error"] [data-apm-error-rule-from]').click();
  const sheet = page.locator('[data-apm-error-rules]');
  await expect(sheet.locator('[data-apm-error-rule-name]')).toHaveValue('PaymentDeclined');
  await expect(sheet.locator('[data-apm-error-rule-pattern="error_type"]')).toHaveValue('PaymentDeclined');
  await expect(sheet.locator('[data-apm-error-rule-preview]')).toHaveAttribute('data-apm-error-rule-preview', '1');
  await sheet.locator('[data-apm-error-rule-save]').click();
  await expect.poll(() => captured.errorRules.length).toBe(1);
  expect(captured.errorRules[0]).toMatchObject({ errorType: 'PaymentDeclined', serviceNamespace: 'shop', serviceName: 'checkout' });
});
