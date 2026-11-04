/**
 * Logs asked for from elsewhere: a service's logs opened from a trace or from a
 * metric series, and the filters pinned on another page. Each asks by meaning
 * (`service=api`, a filter keyed `service_name`); the stream these specs query
 * keeps the service as OTLP writes it, `service.name`, and no environment or
 * alert at all. Every request the page sends names only the stream's fields,
 * and what it cannot name is said under the query.
 */
import type { Page } from '@playwright/test';

import { explorerRead, explorerReadBody, isExplorerRead } from '../fixtures/explorerReads';
import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';
import { expectExplorerResults, serveExplorerResults } from '../fixtures/mockExplorers';

interface LogFilter {
  field: string;
  op: string;
  value: string;
  quoted: boolean;
}

/** The filters of every request the page sends for a page of logs, as it sends them. */
function askedFilters(page: Page): LogFilter[][] {
  const asked: LogFilter[][] = [];

  page.on('request', (request) => {
    if (isExplorerRead(request, 'web/logs')) {
      asked.push(explorerReadBody<{ filters: LogFilter[] }>(request).filters);
    }
  });

  return asked;
}

/** The page asked for logs, and asked every time for exactly these filters. */
function expectAskedFor(asked: readonly LogFilter[][], filters: LogFilter[]) {
  expect(asked.length).toBeGreaterThan(0);
  for (const ask of asked) expect(ask).toEqual(filters);
}

function front(page: Page) {
  return page.locator('[data-query-tab-panel][data-active="true"]');
}

const service = (value: string): LogFilter => ({ field: 'service.name', op: '=', value, quoted: true });

test.describe('Logs asked for from elsewhere', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await serveExplorerResults(page);
  });

  test("a service's logs opened from a trace name the service as the stream does", async ({ page }) => {
    const asked = askedFilters(page);
    await page.goto('/traces/demo-trace');
    await expect(page.getByTestId('trace-span-bar')).toHaveCount(3);

    // The span's service is a reference at the head of its detail.
    await page.getByTestId('trace-span-row').filter({ hasText: 'GET /users' }).getByRole('button', { name: 'GET /users', exact: true }).click();
    await page.getByTestId('trace-span-panel').getByRole('button', { name: 'api', exact: true }).click();
    await page.getByRole('link', { name: /View logs for this service/ }).click();

    await expect(page).toHaveURL(/\/logs\?/);
    await expectExplorerResults(page, 'logs');
    await expect(front(page).locator('.monaco-editor .view-lines').first()).toHaveText("service.name = 'api'");
    expectAskedFor(asked, [service('api')]);
    await expect(front(page).locator('[data-log-condition-notice]')).toHaveCount(0);
  });

  test("a service's logs opened from a metric series leave out what the stream keeps nowhere", async ({ page }) => {
    // One series of the metric, labelled as Prometheus labels it.
    await page.route(explorerRead('query'), async (route) => {
      if (explorerReadBody<{ language?: string }>(route.request()).language !== 'promql') return route.fallback();

      const now = Date.parse(FROZEN_NOW_ISO) * 1_000;

      return route.fulfill({
        json: {
          columns: ['_timestamp', 'value', 'service_name', 'deployment_environment', 'instance'],
          rows: Array.from({ length: 10 }, (_, minute) => [
            now - (10 - minute) * 60_000_000,
            5 + minute,
            'checkout-api',
            'prod',
            'node-3:9100',
          ]),
          scannedRows: 10,
          tookMs: 3,
        },
      });
    });
    const asked = askedFilters(page);
    await page.goto(`/metrics?promql=${encodeURIComponent('rate(http_requests_total[5m])')}`);

    await page.getByRole('button', { name: 'Show labels for http_requests_total' }).click();
    await page.getByTestId('series-identifier-labels').locator('[data-signal-type="service"]').click();
    await page.getByRole('link', { name: /View logs for this service/ }).click();

    await expect(page).toHaveURL(/\/logs\?/);
    await expectExplorerResults(page, 'logs');
    await expect(front(page).locator('.monaco-editor .view-lines').first()).toHaveText("service.name = 'checkout-api'");
    // The series is of the production environment; the stream keeps no environment to ask by.
    expectAskedFor(asked, [service('checkout-api')]);
    await expect(front(page).locator('[data-log-condition-notice]')).toHaveText(
      'Not added to the query: environment = prod. Stream application_logs has no field for it.',
    );
  });

  test('pinned filters apply on the fields of the stream, and one it has no field for is said', async ({ page }) => {
    const asked = askedFilters(page);
    const pinned = [
      ['service_name', '=', 'checkout-api'],
      ['alert', '=', 'A-1'],
    ];
    await page.goto(`/logs?filters=${encodeURIComponent(JSON.stringify(pinned))}`);

    await expectExplorerResults(page, 'logs');
    expectAskedFor(asked, [service('checkout-api')]);
    await expect(front(page).locator('[data-log-condition-notice]')).toHaveText(
      'Not added to the query: pinned filter alert = A-1. Stream application_logs has no field for it.',
    );
  });
});
