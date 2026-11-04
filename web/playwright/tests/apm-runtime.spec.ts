import type { Page } from '@playwright/test';

import { type Captured, installApmRoutes, SERVICE_PATH } from '../fixtures/mockApm';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

async function boot(page: Page, port: number): Promise<Captured> {
  await mountMockRoutes(page, port);
  return installApmRoutes(page, { runtimeMetrics: true });
}

const RUNTIME = SERVICE_PATH.replace('?', '/runtime?');

test('each instance says what it served, and those past the limit are one row', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto(RUNTIME);

  const instance = page.locator('[data-apm-instance="checkout-7d9f-abcde"]');
  await expect(instance).toContainText('2.0.0');
  await expect(instance).toContainText('1.94%');
  await expect(instance).toContainText('410 ms');
  await expect(page.locator('[data-apm-instance="__other__"]')).toContainText('Other instances');
  // The other row is no instance of its own: the count leaves it out.
  await expect(page.locator('[data-apm-card="service-instances"]')).toContainText('2 instances');

  // Its logs are those of its pod, its metrics those with its pod's label.
  await expect(instance.locator('[data-apm-instance-logs]')).toHaveAttribute('href', /\/logs\?.*pod=checkout-7d9f-abcde/);
  const metrics = await instance.locator('[data-apm-instance-metrics]').getAttribute('href');
  expect(new URL(metrics!, 'http://x').searchParams.get('promql')).toBe('{"service.name"="checkout","k8s.pod.name"="checkout-7d9f-abcde"}');
});

test('runtime charts come by family, the process first, and keep to one instance when it is chosen', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto(RUNTIME);

  // The service runs Rust: the process's charts, then Tokio's.
  const families = page.locator('[data-apm-runtime-family]');
  await expect(families).toHaveCount(2);
  await expect(families.nth(0)).toHaveAttribute('data-apm-runtime-family', 'process');
  await expect(families.nth(1)).toHaveAttribute('data-apm-runtime-family', 'rust');
  await expect(page.locator('[data-apm-card="runtime-rust-tasks"]')).toContainText('tokio_num_alive_tasks');
  await expect.poll(() => captured.runtimeQueries.some((statement) => statement.includes('by ("service.instance.id","k8s.pod.name","host.name")'))).toBe(true);

  await page.locator('[data-apm-instance="checkout-7d9f-abcde"]').click();
  await expect(page).toHaveURL(/[?&]instance=checkout-7d9f-abcde/);
  await expect(page.locator('[data-apm-instance="checkout-7d9f-abcde"]')).toHaveAttribute('data-chosen', 'true');
  await expect.poll(() => captured.runtimeQueries.some((statement) => statement.includes('"k8s.pod.name"="checkout-7d9f-abcde"'))).toBe(true);

  // Again, and every instance is drawn.
  await page.locator('[data-apm-instance="checkout-7d9f-abcde"]').click();
  await expect(page).not.toHaveURL(/[?&]instance=/);
});

test('the runtime section draws where the time goes, from the profiles of the range', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto(RUNTIME);

  const profile = page.locator('[data-apm-card="runtime-profile"]');
  const types = profile.getByRole('group', { name: 'Profile type' });
  await expect(types.getByRole('button', { name: 'CPU' })).toHaveAttribute('aria-pressed', 'true');
  await expect(types.getByRole('button', { name: 'Allocated memory' })).toHaveAttribute('aria-pressed', 'false');
  await expect(profile).toContainText('checkout::orders::submit');
  await expect(profile.getByRole('link', { name: 'Open in Profiles' })).toHaveAttribute('href', /^\/profiles\?/);
});
