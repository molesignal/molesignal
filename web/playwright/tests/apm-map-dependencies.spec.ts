import type { Page } from '@playwright/test';

import { type Captured, installApmRoutes, SERVICE_PATH } from '../fixtures/mockApm';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

async function boot(page: Page, port: number): Promise<Captured> {
  await mountMockRoutes(page, port);
  return installApmRoutes(page);
}

const node = (page: Page, name: string) => page.locator(`[data-apm-map-node="${name}"]`);

test('the map draws services, what they call and the topics between them, new and gone calls said', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto('/apm/map');

  await expect(node(page, 'checkout')).toHaveAttribute('data-health', 'warning');
  await expect(node(page, 'postgres/orders')).toHaveAttribute('data-shape', 'database');
  await expect(node(page, 'orders')).toHaveAttribute('data-shape', 'topic');
  await expect(node(page, 'legacy-cart')).toContainText('Gone');

  // A namespace narrows the map, through the address.
  await page.getByRole('combobox', { name: 'Namespace' }).click();
  await page.getByRole('option', { name: 'shop' }).click();
  await expect(page).toHaveURL(/[?&]namespace=shop/);
  await expect.poll(() => captured.mapQueries.at(-1)?.searchParams.get('namespace')).toBe('shop');
});

test('a node chosen lights what calls it and what it calls, and opens its calls and its service', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto('/apm/map');

  await node(page, 'checkout').click();
  const chosen = page.locator('[data-apm-map-chosen="service:checkout"]');
  await expect(chosen).toContainText('Reached from 1 · reaches 4');
  // Upstream and downstream stay lit; nothing else is near it.
  await expect(node(page, 'gateway')).toHaveAttribute('data-dimmed', 'false');
  await expect(node(page, 'mailer')).toHaveAttribute('data-dimmed', 'false');
  await expect(node(page, 'legacy-cart')).toHaveAttribute('data-dimmed', 'true');

  await chosen.locator('[data-apm-map-call="service:checkout->service:payments"]').click();
  const drawer = page.locator('[data-apm-drawer="call"]');
  await expect(drawer).toContainText('checkout → payments');
  await expect(drawer.locator('[data-apm-card="call-operations"]')).toContainText('POST /orders');
  await expect(drawer.locator('[data-apm-card="call-operations"]')).toContainText('As every caller asked them');
  await expect(drawer.locator('[data-apm-call-error="fp-checkout"]')).toContainText('PaymentDeclined');
  expect(captured.mapEdges).toEqual(['service:checkout->service:payments']);

  await drawer.locator('[data-apm-call-error="fp-checkout"]').click();
  await expect(page.locator('[data-apm-drawer="error"]')).toContainText('checkout::payment::authorize');
});

test('a call through a topic says how long its messages waited', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto(`/apm/map?call=${encodeURIComponent('topic:orders->service:mailer')}`);

  const drawer = page.locator('[data-apm-drawer="call"]');
  await expect(drawer).toContainText('Consuming from a topic');
  await expect(drawer.locator('[data-apm-card="call-wait"]')).toContainText('96 of the latest 120 consumptions matched their publishing');
  await expect(drawer.locator('[data-apm-call-wait]')).toContainText('42 ms');
});

test('a namespace folds into one node, and unfolds again', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/apm/map');

  await page.locator('[data-apm-map-fold]').click();
  await expect(node(page, 'shop')).toHaveAttribute('data-shape', 'group');
  await expect(node(page, 'checkout')).toHaveCount(0);

  await node(page, 'shop').click();
  await page.locator('[data-apm-map-unfold]').click();
  await expect(node(page, 'checkout')).toBeVisible();
});

test('dependencies say each target health, calls out of time and the slowest statements', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto(`${SERVICE_PATH.replace('?', '/dependencies?')}&compare=day`);

  const target = page.locator('[data-apm-dependency="postgres/orders"]');
  await expect(target).toContainText('3 timed out');
  await expect(target).toContainText('2 retried');
  await expect(target.locator('[data-apm-dependency-health]')).toHaveText('96');
  await expect(target.locator('[data-apm-dependency-change]')).toBeVisible();

  const slow = page.locator('[data-apm-card="slow-sql"] [data-apm-slow-sql]');
  await expect(slow.first()).toHaveAttribute('data-apm-slow-sql', 'SELECT * FROM orders WHERE id = ? AND status IN (?)');
  await expect(slow.first()).toContainText('380 ms');
});
