import { chromium, type Page } from '@playwright/test';
import { mkdir } from 'node:fs/promises';
import { join } from 'node:path';

import { installMockSession } from '../playwright/fixtures/mockSession';

const outDir = '/private/tmp/molesignal-web-ui-overhaul';
const baseUrl = 'http://127.0.0.1:5174';

const prefsState = { state: { palette: 'default', language: 'en-us' }, version: 0 };

async function preparePage(page: Page) {
  await installMockSession(page);
  await page.addInitScript(
    ({ prefs }) => {
      window.localStorage.setItem('molesignal-ui-prefs', JSON.stringify(prefs));
    },
    { prefs: prefsState },
  );
  await page.goto(`${baseUrl}/home`, { waitUntil: 'domcontentloaded' });
  await page.getByRole('banner').waitFor({ state: 'visible' });
  await page.locator('aside[aria-label="Primary"]').waitFor({ state: 'attached' });
}

async function mockListBackends(page: Page) {
  await page.route('**/api/v1/system/license', (route) =>
    route.fulfill({
      json: {
        edition: 'pro',
        verified: true,
        expired: false,
        issued_to: 'visual-qa',
        features: [],
        max_ingest_bytes_per_day: null,
        expires_at_micros: null,
        active_version_id: 'license-visual-qa',
      },
    }),
  );
  await page.route('**/api/v1/web/search**', (route) =>
    route.fulfill({
      json: {
        items: [
          { kind: 'stream', id: 'logs-prod', label: 'logs-prod', subtitle: 'logs stream' },
          { kind: 'stream', id: 'metrics-prod', label: 'metrics-prod', subtitle: 'metrics stream' },
          { kind: 'stream', id: 'traces-prod', label: 'traces-prod', subtitle: 'traces stream' },
        ],
      },
    }),
  );
  await page.route('**/api/v1/dashboards', (route) =>
    route.fulfill({
      json: [
        {
          id: 'dash-1',
          org_id: 'acme',
          title: 'Production overview',
          folder_id: 'Service health',
          tags: ['api', 'latency'],
          created_at: 1_715_000_000,
          updated_at: 1_716_000_000,
          model: { panels: [{ id: 1 }, { id: 2 }] },
        },
      ],
    }),
  );
  await page.route('**/api/v1/alerts/rules', (route) =>
    route.fulfill({
      json: [
        {
          id: 'rule-1',
          name: 'High error rate',
          enabled: true,
          labels: { severity: 'critical', service: 'api-gateway' },
          trigger: { operator: 'gt', threshold: 0.05, for_periods: 3 },
          query: { period_secs: 60 },
          annotations: { channels: 'pagerduty,#ops-alerts', updated_at: '2026-05-26 14:00' },
        },
      ],
    }),
  );
  await page.route('**/api/v1/alerts/incidents', (route) =>
    route.fulfill({ json: [{ id: 'inc-1', rule_id: 'rule-1', status: 'open' }] }),
  );
  await page.route('**/api/v1/scheduled_pipelines', (route) =>
    route.fulfill({
      json: [
        {
          id: 'pipe-1',
          name: 'Daily log rollup',
          description: 'logs extend table and archive',
          cron: '0 * * * *',
          enabled: true,
        },
      ],
    }),
  );
  await page.route('**/api/v1/scheduled_reports/*/deliveries', (route) =>
    route.fulfill({ json: [{ id: 'delivery-1', delivered_at: 1_716_000_000_000_000, status: 'success' }] }),
  );
  await page.route('**/api/v1/scheduled_reports', (route) =>
    route.fulfill({
      json: [
        {
          id: 'report-1',
          name: 'Weekly platform health',
          description: 'Service health and reliability summary',
          cron: '0 9 * * 1',
          enabled: true,
          recipients: ['#ops-weekly', 'sre@example.com'],
        },
      ],
    }),
  );
}

async function captureRoute(page: Page, routePath: string, name: string, expectedText: RegExp) {
  await page.goto(`${baseUrl}${routePath}`, { waitUntil: 'domcontentloaded' });
  await page.getByRole('banner').waitFor({ state: 'visible' });
  await page.getByText(expectedText).first().waitFor({ state: 'visible' });
  await page.waitForTimeout(300);

  const bodyText = await page.locator('body').innerText();
  if (/\b(list|actions|states|tabs|kpis)\.[a-z0-9_.-]+/i.test(bodyText)) {
    throw new Error(`Raw i18n key rendered on ${routePath}`);
  }

  const file = join(outDir, `list-${name}.png`);
  await page.screenshot({ path: file, fullPage: true });
  return { name, file };
}

async function main() {
  await mkdir(outDir, { recursive: true });
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await mockListBackends(page);
  await preparePage(page);

  const results = [];
  for (const target of [
    { path: '/dashboards', name: 'dashboards', expected: /Production overview/i },
    { path: '/alerts', name: 'alerts', expected: /High error rate/i },
    { path: '/streams', name: 'streams', expected: /logs-prod/i },
    { path: '/functions', name: 'functions', expected: /No functions defined|New function/i },
    { path: '/pipelines', name: 'pipelines', expected: /Daily log rollup/i },
    { path: '/reports', name: 'reports', expected: /Weekly platform health/i },
  ] as const) {
    results.push(await captureRoute(page, target.path, target.name, target.expected));
  }

  await browser.close();
  console.log(JSON.stringify(results, null, 2));
}

void main();
