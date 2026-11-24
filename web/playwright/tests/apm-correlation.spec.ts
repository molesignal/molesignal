import type { Page } from '@playwright/test';

import { installApmRoutes } from '../fixtures/mockApm';
import { installApmSettingsRoutes } from '../fixtures/mockApmSettings';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

async function boot(page: Page, port: number) {
  await mountMockRoutes(page, port);
  await installApmRoutes(page);
  await installApmSettingsRoutes(page);
}

test('the catalog shows a dot for each way a service ties to other data', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/apm/services');

  const dots = page.locator('[data-apm-service-row="checkout"] [data-apm-correlation]');
  await expect(dots).toBeVisible();
  expect(await dots.locator('[data-aspect]').evaluateAll((items) => items.map((item) => `${item.getAttribute('data-aspect')}:${item.getAttribute('data-grade')}`))).toEqual([
    'traceIds:partial',
    'resource:partial',
    'metrics:missing',
    'changes:good',
  ]);

  const label = await dots.getAttribute('aria-label');
  expect(label).toContain('Trace ids in logs: partial. 40% of 1,200 lines carry a trace id. 9 of 10 sampled traces were found.');
  expect(label).toContain('Resource attributes: partial. Not reported: host or pod.');
  expect(label).toContain('Changes: good. 3 changes in the past week.');
});

test('collection quality names the services that cannot be tied to the rest of their data', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/apm/settings');

  const quality = page.locator('[data-apm-card="settings-quality"]');
  const partly = quality.locator('[data-apm-quality-finding="correlation:partly_traced"]');
  await expect(partly).toContainText('1 service ties only part of its logs to kept traces: checkout.');
  await expect(partly.getByRole('link', { name: 'Set it up' })).toHaveAttribute('href', '/datasource/applications/opentelemetry?signal=logs');

  await expect(quality.locator('[data-apm-quality-finding="correlation:unplaced"]')).toContainText('(host, pod, node or container): checkout.');
  await expect(quality.locator('[data-apm-quality-finding="correlation:unmatched"]')).toContainText('Logs or metrics name 1 service APM does not know: checkout-svc.');
  // Checkout reported its changes: nothing to suggest there.
  await expect(quality.locator('[data-apm-quality-finding="correlation:no_changes"]')).toHaveCount(0);

  await quality.locator('[data-apm-quality-finding="correlation:unmatched"]').getByRole('button', { name: 'Naming rules' }).click();
  await expect(page.locator('[data-apm-card="settings-naming"]')).toBeInViewport();
});
