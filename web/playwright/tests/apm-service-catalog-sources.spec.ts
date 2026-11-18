import type { Page } from '@playwright/test';

import { type Captured, installApmRoutes, SERVICE_PATH } from '../fixtures/mockApm';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

async function boot(page: Page, port: number): Promise<Captured> {
  await mountMockRoutes(page, port);
  return installApmRoutes(page);
}

const section = (name: string) => SERVICE_PATH.replace('?', `/${name}?`);

test("a service's other names and data sources are kept, checked and opened from its settings", async ({ page, mockServer }) => {
  const captured = await boot(page, mockServer.port);
  await page.goto(section('settings'));

  const form = page.locator('[data-apm-service-settings] [data-apm-definition-form]');
  const aliases = form.locator('[data-apm-definition-aliases]');
  await expect(aliases).toHaveValue('order_service, 订单中心');

  // Each source opens where its data is read.
  const logs = form.locator('[data-apm-source-row="logs-0"]');
  await expect(logs.locator('[data-apm-source-stream]')).toHaveValue('logs');
  await expect(logs.locator('[data-apm-source-filter]')).toHaveValue("service_name = 'order_service'");
  await expect(logs.locator('[data-apm-source-open]')).toHaveAttribute('href', /^\/logs\?stream=logs&sql=SELECT/);

  const metric = form.locator('[data-apm-source-row="metrics-0"]');
  await expect(metric.locator('[data-apm-source-selector]')).toHaveValue('{service="checkout"}');
  await expect(metric.locator('[data-apm-source-open]')).toHaveAttribute('href', '/metrics?promql=%7Bservice%3D%22checkout%22%7D');

  await expect(form.locator('[data-apm-source-row="dashboards-0"] [data-apm-source-dashboard]')).toHaveText('API latency');
  await expect(form.locator('[data-apm-source-row="links-0"] [data-apm-source-open]')).toHaveAttribute('target', '_blank');

  // The service's own name is not another name of it: said at once, and the form waits.
  const save = form.locator('[data-apm-definition-save]');
  await aliases.fill('order_service, Checkout');
  await expect(form).toContainText("Checkout is the service's own name");
  await expect(save).toBeDisabled();
  await aliases.fill('order_service, orders-legacy');

  // A selector the backend would refuse is said too.
  await form.locator('[data-apm-source-add="metrics"]').click();
  const added = form.locator('[data-apm-source-row="metrics-1"] [data-apm-source-selector]');
  await added.fill('{service="checkout"');
  await expect(form.locator('[data-apm-source-problem="selector"]')).toBeVisible();
  await expect(save).toBeDisabled();
  await added.fill('http_server_requests_total{app="checkout"}');
  await expect(form.locator('[data-apm-source-problem]')).toHaveCount(0);

  await form.locator('[data-apm-source-row="dashboards-0"] [data-apm-source-remove]').click();
  await save.click();

  await expect.poll(() => captured.definitions.length).toBe(1);
  expect(captured.definitions[0]).toMatchObject({
    aliases: ['order_service', 'orders-legacy'],
    dataSources: {
      logs: [{ stream: 'logs', filter: "service_name = 'order_service'" }],
      metrics: [{ selector: '{service="checkout"}' }, { selector: 'http_server_requests_total{app="checkout"}' }],
      dashboards: [],
      links: [{ name: 'Runbook', url: 'https://wiki.example.com/checkout' }],
    },
  });
});

test('the line after a service name names the other names it goes by', async ({ page, mockServer }) => {
  await boot(page, mockServer.port);
  await page.goto(SERVICE_PATH);

  const aliases = page.locator('[data-apm-service-facts] [data-apm-service-fact="aliases"]');
  await expect(aliases).toContainText('order_service +1');
  await expect(aliases).toHaveAttribute('title', /订单中心/);
});
