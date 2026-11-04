import type { Page } from '@playwright/test';

import { type Captured, installApmRoutes, SERVICE_PATH } from '../fixtures/mockApm';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { serveExplorerResults } from '../fixtures/mockExplorers';

async function boot(page: Page, port: number, options: { idle?: boolean } = {}): Promise<Captured> {
  await mountMockRoutes(page, port);
  return installApmRoutes(page, options);
}

const section = (name: string) => SERVICE_PATH.replace('?', `/${name}?`);

test('the line after a service name says what it is, and follows the version in view', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto(SERVICE_PATH);

  const facts = page.locator('[data-apm-service-facts]');
  await expect(facts.locator('[data-apm-service-health="warning"]')).toHaveText('Warning');
  await expect(facts.locator('[data-apm-service-tier="p0"]')).toHaveText('P0');
  for (const text of ['shop', 'prod', 'HTTP · Consumer', 'Rust / Axum', 'Payments', 'ap-southeast-1']) {
    await expect(facts).toContainText(text);
  }
  // A rollout: both versions run at once.
  await expect(facts.locator('[data-apm-service-fact="version"]')).toContainText('2.0.0 · 1.0.0');
  await expect(facts.locator('[data-apm-service-repository]')).toHaveAttribute('href', 'https://github.com/acme/checkout');

  await page.locator('[data-apm-version]').click();
  await page.getByRole('option', { name: '1.0.0' }).click();
  await expect(facts.locator('[data-apm-service-fact="version"]')).toContainText('1.0.0');
  await expect(facts.locator('[data-apm-service-fact="version"]')).not.toContainText('2.0.0');

  // The instances lead to the runtime section, where each says its version and where it runs.
  await facts.locator('[data-apm-service-instances]').click();
  await expect(page).toHaveURL(/\/apm\/services\/checkout\/runtime\?/);
  const instance = page.locator('[data-apm-instance="checkout-7d9f-abcde"]');
  await expect(instance).toContainText('2.0.0');
  await expect(instance).toContainText('shop / checkout / checkout-7d9f-abcde');
  await expect(instance).toContainText('rustc 1.86.0');
});

test('a service with no data in the range is not called unhealthy', async ({ page, mockServer }) => {
  await boot(page, mockServer.port, { idle: true });
  await page.goto(SERVICE_PATH);

  const facts = page.locator('[data-apm-service-facts]');
  await expect(facts.locator('[data-apm-service-health="no_data"]')).toHaveText('No data in this range');
  await expect(facts).toContainText('Rust / Axum');
});

test("a service's settings keep its catalog entry beside what it reports", async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto(section('settings'));

  const reported = page.locator('[data-apm-card="service-reported"]');
  for (const text of ['Axum', 'rustc 1.86.0', 'prod-sg', 'https://github.com/acme/checkout', '2.0.0 (1), 1.0.0 (1)']) {
    await expect(reported).toContainText(text);
  }

  const form = page.locator('[data-apm-card="service-catalog-entry"]');
  await form.locator('[data-apm-definition-repository]').fill('https://git.example.com/acme/checkout');
  await form.locator('[data-apm-definition-save]').click();
  await expect.poll(() => captured.definitions.length).toBe(1);
  expect(captured.definitions[0]).toMatchObject({ repositoryUrl: 'https://git.example.com/acme/checkout', tier: 'p0', teamId: 'team-pay' });
});

test('endpoints and traces are paged with the sizes of the Traces page', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await serveExplorerResults(page);

  await page.goto(section('endpoints'));
  await expect(page.locator('[data-apm-card="endpoints"] [data-page-size]')).toHaveText('100');

  const first = page.waitForRequest((request) => request.url().includes('/api/v1/web/traces?') && request.url().includes('limit=100'));
  await page.goto(section('traces'));
  await first;

  const size = page.locator('[data-apm-card="service-traces"] [data-page-size]');
  await expect(size).toHaveText('100');
  const larger = page.waitForRequest((request) => request.url().includes('/api/v1/web/traces?') && request.url().includes('limit=200'));
  await size.click();
  await page.getByRole('option', { name: '200' }).click();
  await larger;
});
