/**
 * Business keys (a tenant, a plan): declared on the APM settings page, the
 * service page splits its requests by them, and an incident says which values
 * its trouble hit, the important ones flagged.
 */
import type { Page } from '@playwright/test';

import { installApmRoutes, SERVICE_PATH } from '../fixtures/mockApm';
import { installApmSettingsRoutes } from '../fixtures/mockApmSettings';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const LOADED = { timeout: 15_000 };
const TENANT = { key: 'tenant', label: 'Tenant', span: 'tenant.id', important: ['acme'] };

async function serveContext(page: Page, keys: unknown[]) {
  const saved: unknown[] = [];

  await page.route(/\/api\/v1\/business-context$/, async (route) => {
    if (route.request().method() === 'PUT') {
      const body = route.request().postDataJSON();
      saved.push(body);
      await route.fulfill({ json: body });
      return;
    }
    await route.fulfill({ json: { keys } });
  });

  return saved;
}

test('a business key is declared with the field each signal carries it in', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await installApmRoutes(page);
  await installApmSettingsRoutes(page);
  const saved = await serveContext(page, []);
  await page.goto('/apm/settings');

  const card = page.locator('[data-apm-card="settings-business-context"]');
  await expect(card).toBeVisible(LOADED);
  await card.getByRole('button', { name: 'Add a key' }).click();

  await card.getByLabel('Key').fill('Tenant');
  await card.getByLabel('Span attribute').fill('tenant.id');
  await expect(card).toContainText('A key starts with a lower-case letter');
  await expect(page.locator('[data-apm-settings-save="business-context"]')).toBeDisabled();

  await card.getByLabel('Key').fill('tenant');
  await card.getByLabel('Important values').fill('acme, globex');
  await page.locator('[data-apm-settings-save="business-context"]').click();

  await expect.poll(() => saved.length).toBe(1);
  expect(saved[0]).toEqual({ keys: [{ key: 'tenant', label: '', span: 'tenant.id', important: ['acme', 'globex'] }] });
});

test('the service page splits its requests by a business key', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await installApmRoutes(page);
  await serveContext(page, [TENANT]);
  const asked: URLSearchParams[] = [];
  await page.route(/\/api\/v1\/web\/traces\/red\/breakdown(\?.*)?$/, async (route) => {
    const params = new URL(route.request().url()).searchParams;
    asked.push(params);
    await route.fulfill({
      json: {
        field: params.get('by'),
        bucketTime: 1,
        originTime: 0,
        scope: 'entry',
        more: false,
        groups: [
          { value: 'acme', requests: 300, errors: 30, p50Ns: 1e6, p95Ns: 9e6, p99Ns: 2e7, series: [] },
          { value: 'globex', requests: 100, errors: 0, p50Ns: 1e6, p95Ns: 4e6, p99Ns: 5e6, series: [] },
        ],
      },
    });
  });

  await page.goto(SERVICE_PATH);

  const card = page.locator('[data-apm-card="business-breakdown"]');
  await expect(card.locator('[data-apm-business-value="acme"]')).toContainText('Important', LOADED);
  await expect(card.locator('[data-apm-business-value="acme"]')).toContainText('10.0%');
  expect(asked.at(-1)?.get('by')).toBe('tenant.id');
  expect(JSON.parse(asked.at(-1)?.get('filters') ?? '[]')).toEqual([{ field: 'service.name', op: '=', value: 'checkout' }]);
  await expect(card.getByRole('link', { name: 'Split by any field' })).toHaveAttribute('href', /tab=red&by=tenant\.id/);
});

test('an incident says which values of a business key its trouble hit', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.route(/\/api\/v1\/alerts\/incidents\/inc-2292\/business-impact$/, (route) =>
    route.fulfill({
      json: {
        from: 1,
        to: 2,
        services: ['checkout'],
        declared: true,
        keys: [
          {
            key: 'tenant',
            label: 'Tenant',
            field: 'tenant.id',
            more: true,
            values: [
              { value: 'acme', requests: 300, errors: 120, important: true },
              { value: 'globex', requests: 90, errors: 4, important: false },
            ],
          },
        ],
      },
    }),
  );

  await page.goto('/alerts/incidents/inc-2292');

  const section = page.locator('[data-incident-business-impact]');
  await expect(section.locator('[data-business-value="acme"]')).toContainText('Important', LOADED);
  await expect(section.locator('[data-business-value="acme"]')).toContainText('120 of 300 failed');
  await expect(section.getByRole('link', { name: 'acme' })).toHaveAttribute('href', /tab=red/);
  await expect(section).toContainText('More values had requests');
});
