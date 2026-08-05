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
  await page.waitForTimeout(500);
  if (!page.url().includes('/home')) {
    console.log(await page.locator('body').innerText());
    throw new Error(`Mock session did not reach /home: ${page.url()}`);
  }
  await page.getByRole('banner').waitFor({ state: 'visible' });
  await page.locator('aside[aria-label="Primary"]').waitFor({ state: 'attached' });
}

async function mockActiveOrg(page: Page) {
  await page.route('**/api/v1/web/search**', (route) =>
    route.fulfill({
      json: {
        items: [
          { kind: 'stream', id: 'logs-prod', label: 'logs-prod', subtitle: 'logs stream' },
          { kind: 'stream', id: 'metrics-prod', label: 'metrics-prod', subtitle: 'metrics stream' },
        ],
      },
    }),
  );
  await page.route('**/api/v1/dashboards', (route) =>
    route.fulfill({ json: [{ id: 'dash-1', title: 'Production overview', panels: [] }] }),
  );
  await page.route('**/api/v1/alerts/rules', (route) =>
    route.fulfill({ json: [{ id: 'rule-1', name: 'High errors', enabled: true }] }),
  );
  await page.route('**/api/v1/alerts/incidents', (route) => route.fulfill({ json: [] }));
  await page.route('**/api/v1/scheduled_pipelines', (route) =>
    route.fulfill({ json: [{ id: 'pipe-1', name: 'Daily rollup', source_stream: 'logs-prod', target_stream: 'logs-rollup' }] }),
  );
}

async function reloadShell(page: Page) {
  await page.goto(`${baseUrl}/home`, { waitUntil: 'domcontentloaded' });
  await page.getByRole('banner').waitFor({ state: 'visible' });
  await page.locator('aside[aria-label="Primary"]').waitFor({ state: 'attached' });
}

async function verifyChrome(page: Page, isMobile: boolean) {
  await page.getByTestId('org-switcher').focus();
  await page.keyboard.press('Enter');
  await page.getByText('Switch organization').waitFor({ state: 'visible' });
  await reloadShell(page);

  await page.getByTestId('command-palette-trigger').focus();
  await page.keyboard.press('Enter');
  await page.getByText(/Search streams|No results|Actions/i).first().waitFor({ state: 'visible' });
  await reloadShell(page);

  await page.getByTestId('settings-trigger').focus();
  await page.keyboard.press('Enter');
  await page.getByText('Theme').waitFor({ state: 'visible' });
  await reloadShell(page);

  await page.getByRole('button', { name: 'User menu' }).focus();
  await page.keyboard.press('Enter');
  await page.getByText('Sign out').waitFor({ state: 'visible' });
  await reloadShell(page);

  if (isMobile) {
    await page.getByRole('button', { name: 'Toggle sidebar' }).focus();
    await page.keyboard.press('Enter');
    await page.getByRole('link', { name: 'Home' }).waitFor({ state: 'visible' });
    await page.waitForFunction(() => {
      const sidebar = document.querySelector('aside[aria-label="Primary"]');
      if (!sidebar) return false;
      return sidebar.getBoundingClientRect().x >= -1;
    });
  }
}

async function main() {
  await mkdir(outDir, { recursive: true });
  const browser = await chromium.launch({ headless: true });
  const results: Array<{ name: string; file: string }> = [];

  for (const cfg of [
    { name: 'desktop', viewport: { width: 1440, height: 900 }, mobile: false },
    { name: 'laptop', viewport: { width: 1024, height: 900 }, mobile: false },
    { name: 'tablet', viewport: { width: 768, height: 900 }, mobile: false },
    { name: 'mobile', viewport: { width: 375, height: 812 }, mobile: true },
  ] as const) {
    const page = await browser.newPage({ viewport: cfg.viewport });
    await preparePage(page);
    await verifyChrome(page, cfg.mobile);
    const file = join(outDir, `shell-${cfg.name}.png`);
    await page.screenshot({ path: file, fullPage: true });
    results.push({ name: cfg.name, file });
    await page.close();
  }

  for (const cfg of [
    { name: 'home-empty', active: false },
    { name: 'home-active', active: true },
  ] as const) {
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
    if (cfg.active) await mockActiveOrg(page);
    await preparePage(page);
    await page.getByText(cfg.active ? '4/5' : '0/5').first().waitFor({ state: 'visible' });
    const file = join(outDir, `${cfg.name}.png`);
    await page.screenshot({ path: file, fullPage: true });
    results.push({ name: cfg.name, file });
    await page.close();
  }

  await browser.close();
  console.log(JSON.stringify(results, null, 2));
}

void main();
