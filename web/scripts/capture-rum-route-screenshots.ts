import { chromium, type Page } from '@playwright/test';
import { mkdir } from 'node:fs/promises';
import { join } from 'node:path';

import { installMockSession } from '../playwright/fixtures/mockSession';

const outDir = '/private/tmp/molesignal-web-ui-overhaul';
const baseUrl = 'http://127.0.0.1:5174';

const prefsState = { state: { palette: 'default', language: 'en-us' }, version: 0 };
const now = 1_716_000_000_000_000;

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

async function mockRumBackends(page: Page) {
  await page.route('**/api/v1/debug-artifacts**', (route) =>
    route.fulfill({
      json: [
        {
          id: 'map-1',
          application_id: 'storefront',
          service: 'web-console',
          release: 'v1.4.0',
          kind: 'javascript_sourcemap',
          platform: 'web',
          architecture: '',
          debug_id: '',
          filename: 'app.js.map',
          size_bytes: 2048,
          checksum_sha256: 'demo',
          uploaded_at_micros: now,
        },
      ],
    }),
  );
  await page.route('**/api/v1/query', async (route) => {
    const request = route.request().postDataJSON() as { statement?: string };
    const statement = request.statement ?? '';

    if (statement.includes('FROM rum_sessions WHERE')) {
      return route.fulfill({
        json: {
          columns: ['session_id', 'user_id', 'country', 'browser', 'duration_ms', 'error_count', 'started_at_micros'],
          rows: [['sess-1', 'user-1', 'US', 'Chrome', 187_000, 1, now]],
          scanned_rows: 1,
          took_ms: 3,
        },
      });
    }
    if (statement.includes('FROM rum_sessions ORDER')) {
      return route.fulfill({
        json: {
          columns: ['session_id', 'user_id', 'country', 'browser', 'duration_ms', 'error_count', 'started_at_micros'],
          rows: [
            ['sess-1', 'user-1', 'US', 'Chrome', 187_000, 1, now],
            ['sess-2', 'user-2', 'DE', 'Firefox', 91_000, 0, now - 60_000_000],
          ],
          scanned_rows: 2,
          took_ms: 4,
        },
      });
    }
    if (statement.includes('FROM rum_actions WHERE session_id')) {
      return route.fulfill({
        json: {
          columns: ['ts_micros', 'type', 'name', 'payload'],
          rows: [[now, 'click', 'Checkout', { target: '#checkout' }]],
          scanned_rows: 1,
          took_ms: 2,
        },
      });
    }
    if (statement.includes('FROM rum_errors WHERE')) {
      return route.fulfill({
        json: {
          columns: ['fingerprint', 'message', 'stack', 'session_id'],
          rows: [
            [
              'err-1',
              'TypeError: cannot read properties',
              JSON.stringify([{ function: 'renderCheckout', file: 'app.js', line: 42, column: 12 }]),
              'sess-1',
            ],
          ],
          scanned_rows: 1,
          took_ms: 2,
        },
      });
    }
    if (statement.includes('FROM rum_errors GROUP BY fingerprint')) {
      return route.fulfill({
        json: {
          columns: ['fingerprint', 'message', 'count', 'users', 'last_seen_micros'],
          rows: [['err-1', 'TypeError: cannot read properties', 7, 3, now]],
          scanned_rows: 7,
          took_ms: 5,
        },
      });
    }
    if (statement.includes("FROM rum_actions WHERE type = 'view'")) {
      return route.fulfill({
        json: {
          columns: ['ts_micros', 'lcp_ms', 'fid_ms', 'cls', 'ttfb_ms'],
          rows: [
            [now - 180_000_000, 1900, 42, 0.04, 210],
            [now - 120_000_000, 2300, 61, 0.07, 260],
            [now - 60_000_000, 2450, 71, 0.09, 310],
          ],
          scanned_rows: 3,
          took_ms: 3,
        },
      });
    }
    if (statement.includes("FROM rum_actions WHERE type = 'resource'")) {
      return route.fulfill({
        json: {
          columns: ['url', 'count', 'p50_ms', 'p95_ms', 'err_rate'],
          rows: [['/api/orders', 430, 78, 240, 0.012]],
          scanned_rows: 430,
          took_ms: 6,
        },
      });
    }
    if (statement.includes('FROM rum_errors GROUP BY ts_micros')) {
      return route.fulfill({
        json: {
          columns: ['ts_micros', 'count'],
          rows: [[now - 120_000_000, 2], [now - 60_000_000, 5]],
          scanned_rows: 7,
          took_ms: 3,
        },
      });
    }

    return route.fulfill({
      json: { columns: [], rows: [], scanned_rows: 0, took_ms: 1 },
    });
  });
}

async function captureRoute(page: Page, routePath: string, name: string, expectedText: RegExp) {
  await page.goto(`${baseUrl}${routePath}`, { waitUntil: 'domcontentloaded' });
  await page.getByRole('banner').waitFor({ state: 'visible' });
  await page.getByText(expectedText).first().waitFor({ state: 'visible', timeout: 10_000 });
  await page.waitForTimeout(500);

  const bodyText = await page.locator('body').innerText();
  if (/\b(performance|sessions|errors|source_maps|error_detail|session_detail)\.[a-z0-9_.-]+/i.test(bodyText)) {
    throw new Error(`Raw i18n key rendered on ${routePath}`);
  }

  const file = join(outDir, `rum-${name}.png`);
  await page.screenshot({ path: file, fullPage: true });
  return { name, file };
}

async function main() {
  await mkdir(outDir, { recursive: true });
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await mockRumBackends(page);
  await preparePage(page);

  const results = [];
  for (const target of [
    { path: '/rum/sessions', name: 'sessions', expected: /sess-1/i },
    { path: '/rum/sessions/view/sess-1', name: 'session-detail', expected: /Checkout/i },
    { path: '/rum/errors', name: 'errors', expected: /TypeError/i },
    { path: '/rum/errors/view/err-1', name: 'error-detail', expected: /renderCheckout/i },
    { path: '/rum/performance/overview', name: 'performance-overview', expected: /Largest Contentful Paint/i },
    { path: '/rum/performance/apis', name: 'performance-apis', expected: /api\/orders/i },
    { path: '/rum/settings/source-maps', name: 'source-maps', expected: /app\.js\.map/i },
    { path: '/rum/settings/source-maps/upload', name: 'upload-source-maps', expected: /Upload source maps/i },
  ] as const) {
    results.push(await captureRoute(page, target.path, target.name, target.expected));
  }

  await browser.close();
  console.log(JSON.stringify(results, null, 2));
}

void main();
