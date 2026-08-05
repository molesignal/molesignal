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

async function mockAdminBackends(page: Page) {
  await page.route('**/api/v1/users', (route) =>
    route.fulfill({
      json: [
        { id: 'dev', email: 'dev@example.com', display_name: 'Dev User', disabled: false },
        { id: 'sre', email: 'sre@example.com', display_name: 'SRE Admin', disabled: false },
      ],
    }),
  );
  await page.route('**/api/v1/users/dev', (route) =>
    route.fulfill({
      json: { id: 'dev', email: 'dev@example.com', display_name: 'Dev User', disabled: false },
    }),
  );
  await page.route('**/api/v1/orgs', (route) =>
    route.fulfill({
      json: [
        { id: 'acme-prod', name: 'Acme Production', slug: 'acme-prod', role: 'Owner' },
        { id: 'acme-dev', name: 'Acme Development', slug: 'acme-dev', role: 'Admin' },
      ],
    }),
  );
}

async function captureRoute(page: Page, routePath: string, name: string, expectedText: RegExp) {
  await page.goto(`${baseUrl}${routePath}`, { waitUntil: 'domcontentloaded' });
  await page.getByRole('banner').waitFor({ state: 'visible' });
  await page.getByText(expectedText).first().waitFor({ state: 'visible', timeout: 10_000 });
  await page.waitForTimeout(500);

  const bodyText = await page.locator('body').innerText();
  if (/\b(nav|users|groups|general|settings)\.[a-z0-9_.-]+/i.test(bodyText)) {
    throw new Error(`Raw i18n key rendered on ${routePath}`);
  }

  const file = join(outDir, `admin-${name}.png`);
  await page.screenshot({ path: file, fullPage: true });
  return { name, file };
}

async function main() {
  await mkdir(outDir, { recursive: true });
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await mockAdminBackends(page);
  await preparePage(page);

  const results = [];
  for (const target of [
    { path: '/iam/users', name: 'iam-users', expected: /Dev User/i },
    { path: '/iam/roles', name: 'iam-roles', expected: /Built-in role matrix/i },
    { path: '/settings/general', name: 'settings-general', expected: /Default home route/i },
  ] as const) {
    results.push(await captureRoute(page, target.path, target.name, target.expected));
  }

  await browser.close();
  console.log(JSON.stringify(results, null, 2));
}

void main();
