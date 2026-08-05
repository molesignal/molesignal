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
}

async function mockDetailBackends(page: Page) {
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
          {
            span_id: 'span-db',
            parent_span_id: 'span-root',
            service: 'postgres',
            operation: 'SELECT orders',
            start_ns: 5_000_000,
            end_ns: 22_000_000,
            status: 'OK',
            attributes: {},
            events: [],
          },
        ],
      },
    }),
  );
  await page.route('**/api/v1/query', (route) =>
    route.fulfill({
      json: {
        columns: ['trace_id', 'service', 'operation', 'start_ns', 'duration_ms'],
        rows: [['trace-1', 'api-gateway', 'GET /api/orders', 0, 30.2]],
        scanned_rows: 1,
        took_ms: 3,
      },
    }),
  );
  await page.route('**/api/v1/web/search**', (route) =>
    route.fulfill({
      json: {
        items: [
          { kind: 'stream', id: 'logs-prod', label: 'logs-prod', subtitle: 'logs stream' },
        ],
      },
    }),
  );
  await page.route('**/api/v1/query/jobs/job-1', (route) =>
    route.fulfill({
      json: {
        job_id: 'job-1',
        state: 'completed',
        submitted_at_micros: 1_716_000_000_000_000,
        started_at_micros: 1_716_000_000_100_000,
        finished_at_micros: 1_716_000_000_500_000,
        result_object_key: 'queries/job-1/result.ndjson',
        result_rows: 42,
        error: null,
        expires_at_micros: 1_716_086_400_000_000,
      },
    }),
  );
  await page.route('**/api/v1/dashboards/dash-1', (route) =>
    route.fulfill({
      json: {
        id: 'dash-1',
        org_id: 'acme',
        title: 'Production overview',
        folder_id: 'Service health',
        tags: ['api'],
        created_at: 1_715_000_000,
        updated_at: 1_716_000_000,
        model: { panels: [{ id: 1 }, { id: 2 }] },
      },
    }),
  );
}

async function captureRoute(
  page: Page,
  routePath: string,
  name: string,
  expected: { text?: RegExp; inputValue?: RegExp },
) {
  await page.goto(`${baseUrl}${routePath}`, { waitUntil: 'domcontentloaded' });
  await page.getByRole('banner').waitFor({ state: 'visible' });
  if (expected.text) {
    await page.getByText(expected.text).first().waitFor({ state: 'visible', timeout: 10_000 });
  }
  if (expected.inputValue) {
    await page.waitForFunction(
      ({ source, flags }) => {
        const matcher = new RegExp(source, flags);
        return Array.from(document.querySelectorAll('input, textarea')).some((element) => {
          const field = element as HTMLInputElement | HTMLTextAreaElement;
          return matcher.test(field.value);
        });
      },
      { source: expected.inputValue.source, flags: expected.inputValue.flags },
      { timeout: 10_000 },
    );
  }
  await page.waitForTimeout(500);

  const bodyText = await page.locator('body').innerText();
  if (/\b(detail|session|inspector|editor)\.[a-z0-9_.-]+/i.test(bodyText)) {
    throw new Error(`Raw i18n key rendered on ${routePath}`);
  }

  const file = join(outDir, `detail-${name}.png`);
  await page.screenshot({ path: file, fullPage: true });
  return { name, file };
}

async function main() {
  await mkdir(outDir, { recursive: true });
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await mockDetailBackends(page);
  await preparePage(page);

  const results = [];
  for (const target of [
    { path: '/traces/trace-1', name: 'trace', expected: { text: /Trace detail/i } },
    { path: '/traces/session/session-1', name: 'trace-session', expected: { text: /Session traces/i } },
    { path: '/streams/logs-prod', name: 'stream', expected: { text: /logs-prod/i } },
    { path: '/logs/inspector?id=job-1', name: 'logs-inspector', expected: { text: /job-1/i } },
    { path: '/dashboards/dash-1', name: 'dashboard', expected: { text: /Production overview/i } },
    { path: '/dashboards/dash-1/edit', name: 'dashboard-editor', expected: { inputValue: /Production overview/i } },
  ] as const) {
    results.push(await captureRoute(page, target.path, target.name, target.expected));
  }

  await browser.close();
  console.log(JSON.stringify(results, null, 2));
}

void main();
