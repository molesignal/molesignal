import type { Page } from '@playwright/test';

import { type Captured, installApmRoutes, SERVICE_PATH } from '../fixtures/mockApm';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

async function boot(page: Page, port: number): Promise<Captured> {
  await mountMockRoutes(page, port);
  return installApmRoutes(page);
}

const section = (name: string) => SERVICE_PATH.replace('?', `/${name}?`);

test('the catalog keeps archived services out until asked, groups by team or tag, and filters by tag', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/apm/services');

  const rows = page.locator('[data-apm-service-row]');
  await expect(rows).toHaveCount(2);
  await expect(page.locator('[data-apm-service-row="legacy-cart"]')).toHaveCount(0);

  // The archived service comes back with its word, and the toggle goes into the address.
  await page.locator('[data-apm-catalog-archived]').click();
  await expect(page.locator('[data-apm-service-row="legacy-cart"] [data-apm-archived]')).toHaveText('Archived');
  await expect(page).toHaveURL(/[?&]archived=1/);
  await page.locator('[data-apm-catalog-archived]').click();

  await page.locator('[data-apm-catalog-grouping]').click();
  await page.getByRole('option', { name: 'Team' }).click();
  const heads = page.locator('[data-apm-catalog-group-head]');
  await expect(heads).toHaveCount(2);
  await expect(heads.nth(0)).toContainText('Payments');
  await expect(heads.nth(0)).toContainText('1');
  await expect(heads.nth(1)).toContainText('No team');
  await expect(page).toHaveURL(/[?&]group=team/);

  await page.locator('[data-apm-catalog-grouping]').click();
  await page.getByRole('option', { name: 'Tag domain' }).click();
  await expect(heads.nth(0)).toContainText('domain: orders');
  await expect(heads.nth(1)).toContainText('domain: payments');

  await page.locator('[data-apm-catalog-tag]').click();
  await page.getByRole('option', { name: 'pci' }).click();
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toHaveAttribute('data-apm-service-row', 'checkout');

  // A shared address opens on the same view.
  await page.reload();
  await expect(rows).toHaveCount(1);
  await expect(heads.first()).toContainText('domain: orders');
});

test('a starred service is kept at the top of the catalog, for this reader, and can be listed alone', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/apm/services');

  const rows = page.locator('[data-apm-service-row]');
  await expect(rows.first()).toHaveAttribute('data-apm-service-row', 'checkout');

  const star = page.locator('[data-apm-service-row="billing"] [data-apm-star]');
  const kept = page.waitForResponse((response) => response.url().includes('/me/ui-preference/apm.favorites/services') && response.request().method() === 'PUT');
  await star.click();
  await expect(star).toHaveAttribute('data-apm-star', 'on');
  // The star is the star's, not the row's: the catalog stays where it is.
  await expect(page).toHaveURL(/\/apm\/services$/);
  await expect(rows.first()).toHaveAttribute('data-apm-service-row', 'billing');

  await page.locator('[data-apm-catalog-favorites]').click();
  await expect(rows).toHaveCount(1);

  // Kept on the server: another visit shows the same star.
  await kept;
  await page.goto('/apm/services?favorites=1');
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toHaveAttribute('data-apm-service-row', 'billing');

  await page.goto(SERVICE_PATH);
  const favorite = page.locator('[data-apm-service-favorite]');
  await expect(favorite).toHaveText('Favorite');
  await favorite.click();
  await expect(favorite).toHaveAttribute('data-apm-service-favorite', 'on');
  await expect(favorite).toHaveText('Favorited');
});

test('an endpoint is starred as a key transaction, listed alone and in the settings', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto(section('endpoints'));

  const row = page.locator('[data-apm-endpoint-row="POST /orders"]');
  await row.locator('[data-apm-star]').click();
  await expect(row.locator('[data-apm-star]')).toHaveAttribute('data-apm-star', 'on');
  await expect.poll(() => captured.keyTransactions).toEqual([{ transaction: { name: 'POST /orders', kind: 'http' }, key: true }]);
  // The star does not open the endpoint.
  await expect(page.locator('[data-apm-drawer="endpoint"]')).toHaveCount(0);

  await page.locator('[data-apm-endpoints-key-only]').click();
  await expect(page.locator('[data-apm-endpoint-row]')).toHaveCount(1);

  // The drawer says it too, and its Apdex says the service's threshold.
  await row.click();
  const drawer = page.locator('[data-apm-drawer="endpoint"]');
  await expect(drawer.locator('[data-apm-star="on"]')).toBeVisible();
  await expect(drawer.locator('[data-apm-apdex]')).toContainText('T = 300 ms');

  await page.goto(section('settings'));
  const starred = page.locator('[data-apm-key-transaction="POST /orders"]');
  await expect(starred).toContainText('HTTP');
  await starred.locator('[data-apm-star]').click();
  await expect(page.locator('[data-apm-card="service-key-transactions"]')).toContainText('No key transactions yet');
  await expect.poll(() => captured.keyTransactions.at(-1)).toEqual({ transaction: { name: 'POST /orders', kind: 'http' }, key: false });
});

test('the settings keep tags, an Apdex threshold and a git repository, and archive the service', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto(section('settings'));

  const form = page.locator('[data-apm-service-settings] [data-apm-definition-form]');
  const tags = form.locator('[data-apm-definition-tags]');
  await expect(tags).toHaveValue('domain=orders, pci');
  await expect(form.locator('[data-apm-definition-apdex]')).toHaveValue('300');

  // A key the backend would refuse is said at once, and the form waits.
  await tags.fill('Domain=orders');
  await expect(form).toContainText('The tag key "Domain"');
  await expect(form.locator('[data-apm-definition-save]')).toBeDisabled();

  // A git address is a repository as well as an https one; something else is said at once.
  const repository = form.locator('[data-apm-definition-repository]');
  await repository.fill('github.com/acme/checkout');
  await expect(form).toContainText('Enter an http, https, ssh or git address');
  await repository.fill('git@github.com:acme/checkout.git');

  await tags.fill('domain=orders, team=payments eu');
  await form.locator('[data-apm-definition-apdex]').fill('250');
  await form.locator('[data-apm-definition-save]').click();
  await expect.poll(() => captured.definitions.length).toBe(1);
  expect(captured.definitions[0]).toMatchObject({
    tags: { domain: 'orders', team: 'payments eu' },
    apdexThreshold: 250_000,
    repositoryUrl: 'git@github.com:acme/checkout.git',
    archived: false,
  });

  await form.locator('[data-apm-definition-archive]').click();
  await expect.poll(() => captured.definitions.length).toBe(2);
  expect(captured.definitions[1]).toMatchObject({ archived: true });
});

test('the other signals of a service are lit where they carry it, and lead to connecting them where not', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto(SERVICE_PATH);

  const signals = page.locator('[data-apm-related-signals]');
  const logs = signals.locator('[data-apm-related="logs"]');
  await expect(logs).toHaveAttribute('data-coverage', 'reported');
  await expect(logs).toHaveAttribute('href', /^\/logs\?.*stream=default/);

  const metrics = signals.locator('[data-apm-related="metrics"]');
  await expect(metrics).toHaveAttribute('data-coverage', 'missing');
  await expect(metrics).toHaveAttribute('href', '/datasource/applications/opentelemetry?signal=metrics');

  await expect(signals.locator('[data-apm-related="profiles"]')).toHaveAttribute('data-coverage', 'reported');
  await expect(signals.locator('[data-apm-related="rum"]')).toHaveAttribute('href', /^\/rum\/sessions\/view\/session-1\?/);
  await expect(page.locator('[data-apm-latency-distribution] [data-apm-apdex]')).toContainText('T = 300 ms');
});
