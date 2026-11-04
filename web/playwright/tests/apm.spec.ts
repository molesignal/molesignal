import AxeBuilder from '@axe-core/playwright';
import type { Page } from '@playwright/test';

import { type Captured, installApmRoutes, SERVICE_PATH } from '../fixtures/mockApm';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

async function boot(page: Page, port: number, options: { theme?: 'light' | 'dark'; runtimeMetrics?: boolean } = {}): Promise<Captured> {
  await mountMockRoutes(page, port, { theme: options.theme ?? 'light' });
  return installApmRoutes(page, options);
}

test('the service catalog lists every service with its owners, and the services declared before they report', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/apm/services');

  const checkout = page.locator('[data-apm-service-row="checkout"]');
  await expect(checkout).toContainText('Payments');
  await expect(checkout).toContainText('P0');
  await expect(checkout).toContainText('2.0.0');
  await expect(page.locator('[data-apm-service-row="billing"]')).toContainText('Not reporting');
  await expect(page.locator('[data-apm-catalog-summary]')).toContainText('2 services');

  await checkout.click();
  await expect(page).toHaveURL(/\/apm\/services\/checkout\?namespace=shop&environment=prod$/);
});

test('the catalog columns are chosen in its column menu and kept', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/apm/services');
  await expect(page.locator('[data-apm-service-row="checkout"]')).toBeVisible();

  await page.locator('[data-apm-columns]').click();
  await page.getByRole('menuitemcheckbox', { name: 'Owner' }).click();
  await page.keyboard.press('Escape');
  await expect(page.locator('[data-apm-catalog-table] [role="columnheader"]', { hasText: 'Owner' })).toBeVisible();
  await expect(page.locator('[data-apm-service-row="checkout"]')).toContainText('Ada Lovelace');

  await page.reload();
  await expect(page.locator('[data-apm-catalog-table] [role="columnheader"]', { hasText: 'Owner' })).toBeVisible();
});

test('a catalog wider than the window scrolls as one, every column inside the head and the rows', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/apm/services');
  await expect(page.locator('[data-apm-service-row="checkout"]')).toBeVisible();
  await page.locator('[data-apm-columns]').click();
  await page.getByRole('menuitemcheckbox', { name: 'Lifecycle' }).click();
  await page.keyboard.press('Escape');

  const table = page.locator('[data-apm-catalog-table]');
  await expect(table.locator('[role="columnheader"]', { hasText: 'Lifecycle' })).toBeAttached();
  const widths = await table.evaluate((element) => {
    const head = element.querySelector('[role="row"]')!;
    const row = element.querySelector('[data-apm-service-row="checkout"]')!;
    return { table: [element.scrollWidth, element.clientWidth], head: [head.scrollWidth, head.clientWidth], row: [row.scrollWidth, row.clientWidth] };
  });
  // The table scrolls sideways; nothing spills past the head's band or a row's edge.
  expect(widths.table[0]).toBeGreaterThan(widths.table[1]!);
  expect(widths.head[0]).toBe(widths.head[1]);
  expect(widths.row[0]).toBe(widths.row[1]);
  expect(widths.head[1]).toBe(widths.row[1]);

  const lifecycle = page.locator('[data-apm-service-row="checkout"] [role="cell"]').last();
  await lifecycle.scrollIntoViewIfNeeded();
  await expect(lifecycle).toHaveText('Production');
  await expect(lifecycle).toBeVisible();
});

test('a service opens on its overview, with its team after its name and its documents in its navigation card', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto(SERVICE_PATH);

  await expect(page.getByRole('heading', { name: 'checkout', exact: true })).toBeVisible();
  // The team is said after the name; its owner and who is on call when pointed at.
  const team = page.locator('[data-apm-service-facts] [data-apm-service-fact="Team"]');
  await expect(team).toContainText('Payments');
  await expect(team).toHaveAttribute('title', /Owner: Ada Lovelace · On call: Ada Lovelace/);
  const scope = page.locator('[data-apm-navigation="service"] [data-apm-scope]');
  await expect(scope.locator('[data-apm-docs]')).toContainText('1');
  await expect(page.locator('[data-apm-red-charts]')).toBeVisible();
  await expect(page.locator('[data-apm-latency-distribution]')).toBeVisible();
  await expect(page.locator('[data-apm-card="downstream"]')).toContainText('postgres/orders');
  await expect(page.locator('[data-apm-card="top-errors"]')).toContainText('PaymentDeclined');
});

test('an endpoint opens in a drawer with its slowest requests found again in the trace store', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto('/apm/services/checkout/endpoints?namespace=shop&environment=prod');

  await page.locator('[data-apm-endpoint-row="POST /orders"]').click();
  const drawer = page.locator('[data-apm-drawer="endpoint"]');
  await expect(drawer).toBeVisible();
  await expect(drawer.locator('[data-apm-slow-request]')).toHaveCount(1);

  const filters = JSON.parse(captured.spanQueries[0]?.searchParams.get('filters') ?? '[]') as Array<{ field: string; value: string }>;
  expect(filters).toEqual([
    { field: 'service.name', op: '=', value: 'checkout' },
    { field: 'http.request.method', op: '=', value: 'POST' },
    { field: 'http.route', op: '=', value: '/orders' },
  ]);
  expect(captured.spanQueries[0]?.searchParams.get('sort')).toBe('duration_desc');

  // A request opens its trace in the trace drawer, over the endpoint, which it names as the way back.
  await drawer.locator('[data-apm-slow-request]').click();
  const traceDrawer = page.locator('[data-trace-drawer]');
  await expect(traceDrawer).toBeVisible();
  const back = traceDrawer.locator('[data-apm-drawer-trail="trace"] [data-apm-drawer-crumb="endpoint"]');
  await expect(back).toContainText('POST /orders');
  await back.click();
  await expect(traceDrawer).toBeHidden();
  await expect(drawer).toBeVisible();

  // An error of the endpoint opens over it, and leads back the same way, closing only itself.
  await drawer.locator('[data-apm-card="endpoint-errors"] button').first().click();
  const errorDrawer = page.locator('[data-apm-drawer="error"]');
  await expect(errorDrawer.locator('[data-apm-drawer-trail="error"]')).toContainText('POST /orders');
  await errorDrawer.locator('[data-apm-drawer-crumb="endpoint"]').click();
  await expect(errorDrawer).toBeHidden();
  await expect(drawer).toBeVisible();
  await expect(page).not.toHaveURL(/[?&]error=/);
});

test('an endpoint opened from an error lies over it and leads back to it, after a reload too', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto(SERVICE_PATH);

  await page.locator('[data-apm-card="top-errors"] button').first().click();
  const errorDrawer = page.locator('[data-apm-drawer="error"]');
  await expect(errorDrawer).toContainText('PaymentDeclined');
  await errorDrawer.locator('[data-apm-card="error-endpoints"] a').first().click();

  const endpointDrawer = page.locator('[data-apm-drawer="endpoint"]');
  const back = endpointDrawer.locator('[data-apm-drawer-trail="endpoint"] [data-apm-drawer-crumb="error"]');
  const onTop = () =>
    endpointDrawer.evaluate((drawer) => {
      const box = drawer.getBoundingClientRect();
      const hit = document.elementFromPoint(box.left + box.width / 2, box.top + box.height / 2);
      return hit?.closest('[data-apm-drawer]')?.getAttribute('data-apm-drawer');
    });
  await expect(back).toHaveText('PaymentDeclined');
  await expect.poll(onTop).toBe('endpoint');

  // The address keeps the order the drawers were opened in.
  await page.reload();
  await expect(back).toHaveText('PaymentDeclined');
  await expect.poll(onTop).toBe('endpoint');

  await back.click();
  await expect(endpointDrawer).toBeHidden();
  await expect(errorDrawer).toBeVisible();
  await expect(page).not.toHaveURL(/[?&](endpoint|kind)=/);
});

test('an endpoint is listed by its type and its name in columns of their own', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/apm/services/checkout/endpoints?namespace=shop&environment=prod');

  // After the star of key transactions: the type, then the name.
  const head = page.locator('[data-apm-card="endpoints"] [role="columnheader"]');
  await expect(head.nth(1)).toHaveText('Type');
  await expect(head.nth(2)).toHaveText('Endpoint');
  const row = page.locator('[data-apm-endpoint-row="POST /orders"]');
  await expect(row.locator(':scope > span').nth(1)).toHaveText('HTTP');
  // The method leads the name in its own colour, as the trace table writes it.
  await expect(row.locator(':scope > span').nth(2).getByText('POST', { exact: true })).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
});

test('dependencies list the callers of a service and its calls out by target', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/apm/services/checkout/dependencies?namespace=shop&environment=prod');

  await expect(page.locator('[data-apm-card="upstream"]')).toContainText('gateway');
  const target = page.locator('[data-apm-dependency="postgres/orders"]');
  await expect(target).toContainText('2 operations');
  await target.click();
  await expect(page.locator('[data-apm-card="downstream-table"]')).toContainText('SELECT');
});

test('error groups are triaged where they are read', async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto('/apm/errors');

  const row = page.locator('[data-apm-error-row="fp-checkout"]');
  await expect(row).toContainText('PaymentDeclined');
  await expect(row.locator('[data-apm-issue-state]')).toHaveText('Open');
  await row.click();

  const drawer = page.locator('[data-apm-drawer="error"]');
  await expect(drawer).toContainText('checkout::payment::authorize');
  await drawer.locator('[data-apm-issue-status]').click();
  await page.getByRole('option', { name: 'Resolved' }).click();
  await expect.poll(() => captured.issues.length).toBe(1);
  expect(captured.issues[0]).toMatchObject({ status: 'resolved', assigneeId: null });
});

test('deployments list each version and compare two of them', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/apm/services/checkout/deployments?namespace=shop&environment=prod');

  await expect(page.locator('[data-apm-deployment="2.0.0"]')).toContainText('Current');
  await expect(page.locator('[data-apm-card="compare"]')).toContainText('Regressed');
  await expect(page.locator('[data-apm-card="compare"]')).toContainText('POST /orders');
});

test('runtime charts come from the runtime metrics a service sends', async ({ page, mockServer }) => {
  await boot(page, mockServer.port, { runtimeMetrics: true });
  await page.goto('/apm/services/checkout/runtime?namespace=shop&environment=prod');

  await expect(page.locator('[data-apm-instance="checkout-7d9f-abcde"]')).toContainText('2.0.0');
  await expect(page.locator('[data-apm-card="runtime-process-cpu"]')).toContainText('process_cpu_utilization_ratio');
});

test('runtime says how to get its charts while a service sends no runtime metrics', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/apm/services/checkout/runtime?namespace=shop&environment=prod');

  const empty = page.locator('[data-apm-card="runtime-empty"]');
  await expect(empty).toContainText('No runtime metrics from this service');

  // Its way forward is the OpenTelemetry intake guide, on metrics.
  await empty.getByRole('link').click();
  await expect(page).toHaveURL(/\/datasource\/applications\/opentelemetry\?signal=metrics$/);
});

test('the service map draws every service, and APM opens on its overview', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/apm/map');
  await expect(page.locator('[data-apm-service-map]')).toContainText('payments');

  await page.goto('/apm');
  await expect(page).toHaveURL(/\/apm\/overview$/);
});

test('APM opens on an overview of every service: what they did, which weigh most and what goes on', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/apm');
  await expect(page).toHaveURL(/\/apm\/overview$/);

  const pulse = page.locator('[data-apm-overview-pulse]');
  await expect(pulse).toContainText('Active services');
  await expect(pulse).toContainText('2 services, 1 without traffic');
  await expect(pulse).toContainText('0.91');
  await expect(pulse).toContainText('P90 380 ms');
  await expect(page.locator('[data-apm-health-count="warning"]')).toHaveText('Warning 1');

  await expect(page.locator('[data-apm-insight="anomaly"]')).toContainText('checkout');
  await expect(page.locator('[data-apm-alert="inc-1"]')).toContainText('Checkout error rate above 2%');
  await expect(page.locator('[data-apm-alert="inc-1"]')).toContainText('Critical');
  await expect(page.locator('[data-apm-deployment="2.0.0"]')).toContainText('checkout');
  await expect(page.locator('[data-apm-instrumentation-gaps]')).toContainText('1 service report no service.version');

  // Environments can be chosen whichever one is in view.
  await page.getByRole('combobox', { name: 'Environment' }).click();
  await expect(page.getByRole('option', { name: 'staging' })).toBeVisible();
  await page.keyboard.press('Escape');

  await page.locator('[data-apm-ranking="slowest"] a').first().click();
  await expect(page).toHaveURL(/\/apm\/services\/checkout\?namespace=shop&environment=prod/);
});

test('the APM user-experience address opens RUM', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/apm/user-experience/overview');
  await expect(page).toHaveURL(/\/rum\/explorer$/);
  await expect(page.getByRole('link', { name: 'Applications' })).toBeVisible();
});

test('legacy service URLs open the service in APM', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/services/checkout?environment=prod');
  await expect(page).toHaveURL(/\/apm\/services\/checkout\?environment=prod$/);
  await expect(page.getByRole('heading', { name: 'checkout', exact: true })).toBeVisible();
});

test('APM keyboard navigation opens the service catalog', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto('/investigate');
  await page.keyboard.press('Meta+Alt+S');
  await expect(page).toHaveURL(/\/apm\/services$/);
  await expect(page.getByRole('heading', { name: 'Application Performance Monitoring' })).toBeVisible();
});

for (const theme of ['light', 'dark'] as const) {
  test(`the APM catalog is accessible in ${theme} theme`, async ({ page, mockServer }) => {
    await boot(page, mockServer.port, { theme });
    await page.goto('/apm/services');
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
    await expect(page.locator('[data-apm-service-row="checkout"]')).toBeVisible();
    const results = await new AxeBuilder({ page }).exclude('[role="status"]').exclude('[aria-live]').analyze();
    const critical = results.violations.filter((violation) => violation.impact === 'critical');
    expect(critical, JSON.stringify(critical, null, 2)).toEqual([]);
  });

  test(`the APM overview is accessible in ${theme} theme`, async ({ page, mockServer }) => {
    await boot(page, mockServer.port, { theme });
    // Contrast is read from the figures as they rest, not halfway through rising in.
    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.goto('/apm/overview');
    await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
    await expect(page.locator('[data-apm-alert="inc-1"]')).toBeVisible();
    const results = await new AxeBuilder({ page }).exclude('[role="status"]').exclude('[aria-live]').analyze();
    const serious = results.violations.filter((violation) => violation.impact === 'critical' || violation.impact === 'serious');
    expect(serious, JSON.stringify(serious, null, 2)).toEqual([]);
  });
}
