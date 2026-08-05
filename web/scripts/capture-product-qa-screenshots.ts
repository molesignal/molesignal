import { chromium, type Page } from '@playwright/test';
import { mkdir } from 'node:fs/promises';
import { join } from 'node:path';

import { installMockSession } from '../playwright/fixtures/mockSession';

const outDir = '/private/tmp/molesignal-web-ui-overhaul';
const baseUrl = 'http://127.0.0.1:5174';

const prefsState = { state: { palette: 'default', language: 'en-us' }, version: 0 };

async function installMocks(page: Page) {
  await page.route('**/api/v1/system/license', (route) =>
    route.fulfill({
      json: {
        edition: 'community',
        verified: false,
        expired: false,
        issued_to: 'visual-qa',
        features: [],
        max_ingest_bytes_per_day: null,
        expires_at_micros: null,
        active_version_id: null,
      },
    }),
  );
  await page.route('**/api/v1/users/dev', (route) =>
    route.fulfill({ json: { id: 'dev', email: 'dev@example.com', display_name: 'Dev User', disabled: false } }),
  );
  await page.route('**/api/v1/users', (route) =>
    route.fulfill({ json: [{ id: 'dev', email: 'dev@example.com', display_name: 'Dev User', disabled: false }] }),
  );
  await page.route('**/api/v1/orgs', (route) =>
    route.fulfill({ json: [{ id: 'acme-prod', name: 'Acme Production', slug: 'acme-prod', role: 'Owner' }] }),
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
  await page.route('**/api/v1/web/trace/trace-1', (route) =>
    route.fulfill({
      json: {
        trace_id: 'trace-1',
        root_span_id: 'span-root',
        spans: [
          {
            span_id: 'span-root',
            service: 'api-gateway',
            operation: 'GET /api/orders',
            start_ns: 0,
            end_ns: 30_000_000,
            status: 'OK',
            attributes: {},
            events: [],
          },
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
          tags: ['api'],
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
      json: [{ id: 'pipe-1', name: 'Daily log rollup', description: 'logs archive', cron: '0 * * * *', enabled: true }],
    }),
  );
}

async function preparePage(page: Page) {
  await installMockSession(page);
  await page.addInitScript(
    ({ prefs }) => {
      window.localStorage.setItem('molesignal-ui-prefs', JSON.stringify(prefs));
    },
    { prefs: prefsState },
  );
  await installMocks(page);
  await page.goto(`${baseUrl}/home`, { waitUntil: 'domcontentloaded' });
  await page.getByRole('banner').waitFor({ state: 'visible' });
  await page.locator('aside[aria-label="Primary"]').waitFor({ state: 'attached' });
}

async function verifyPage(page: Page, routePath: string) {
  const bodyText = await page.locator('body').innerText();
  if (/\b(?:actions|activation|detail|drawer|editor|general|home|datasource|kpis|list|nav|settings|states|tabs|users)\.[a-z0-9_.-]+/i.test(bodyText)) {
    throw new Error(`Raw i18n key rendered on ${routePath}`);
  }
  if (/\{[^}]*("?status"?|"?error"?|"?message"?)[^}]*403/i.test(bodyText) || /Request failed with status code 403/i.test(bodyText)) {
    throw new Error(`Raw 403 payload rendered on ${routePath}`);
  }

  const blankTables = await page.evaluate(() =>
    Array.from(document.querySelectorAll('table'))
      .filter((table) => table.querySelectorAll('tbody tr').length === 0)
      .map((table) => table.textContent?.trim() ?? '')
      .filter(Boolean),
  );
  if (blankTables.length > 0 && !/(No|Awaiting|required|pending|Loading|empty)/i.test(bodyText)) {
    throw new Error(`Blank table without product state on ${routePath}: ${blankTables.join(' | ')}`);
  }

  const clippedButtons = await page.evaluate(() =>
    Array.from(document.querySelectorAll('button, a[role="button"], [role="button"]'))
      .map((element) => {
        const text = (element.textContent ?? '').replace(/\s+/g, ' ').trim();
        const htmlElement = element as HTMLElement;
        const style = window.getComputedStyle(htmlElement);
        const clipped =
          text.length > 0 &&
          style.overflow !== 'visible' &&
          (htmlElement.scrollWidth > htmlElement.clientWidth + 2 || htmlElement.scrollHeight > htmlElement.clientHeight + 2);
        return clipped ? text : '';
      })
      .filter(Boolean),
  );
  if (clippedButtons.length > 0) {
    throw new Error(`Clipped button text on ${routePath}: ${clippedButtons.join(' | ')}`);
  }
}

async function captureRoute(
  page: Page,
  routePath: string,
  name: string,
  expectedText: RegExp,
) {
  await page.goto(`${baseUrl}${routePath}`, { waitUntil: 'domcontentloaded' });
  await page.getByRole('banner').waitFor({ state: 'visible' });
  await page.getByText(expectedText).first().waitFor({ state: 'visible', timeout: 10_000 });
  await page.waitForTimeout(300);
  await verifyPage(page, routePath);
  const file = join(outDir, `qa-${name}.png`);
  await page.screenshot({ path: file, fullPage: true });
  return { name, file };
}

async function main() {
  await mkdir(outDir, { recursive: true });
  const browser = await chromium.launch({ headless: true });
  const results: Array<{ name: string; file: string }> = [];

  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await preparePage(page);
  for (const target of [
    { path: '/home', name: 'home', expected: /Activation/i },
    { path: '/datasource/recommended/kubernetes', name: 'datasource', expected: /Kubernetes/i },
    { path: '/traces/trace-1', name: 'observe-trace', expected: /Trace detail/i },
    { path: '/streams', name: 'data-streams', expected: /logs-prod/i },
    { path: '/settings/general', name: 'admin-settings', expected: /Default home route/i },
  ] as const) {
    results.push(await captureRoute(page, target.path, target.name, target.expected));
  }
  await page.close();

  const mobile = await browser.newPage({ viewport: { width: 375, height: 812 } });
  await preparePage(mobile);
  results.push(await captureRoute(mobile, '/home', 'mobile-shell', /Activation/i));
  await mobile.close();

  await browser.close();
  console.log(JSON.stringify(results, null, 2));
}

void main();
