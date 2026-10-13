import type { Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;

/** A workspace whose logs are arriving. */
const RECEIVING_LOGS = {
  generated_at_micros: NOW,
  window: { start_micros: NOW - 3_600_000_000, end_micros: NOW, window_secs: 3_600 },
  intake_status: 'healthy',
  probe_reason: null,
  intake_bytes: 9_400_000,
  stored_bytes: 2_150_000,
  rows: 48_210,
  compression_savings_ratio: 0.77,
  active_streams: 1,
  total_streams: 1,
  attention_streams: 0,
  last_received_at_micros: NOW - 12_000_000,
  stats_probe: { succeeded: 1, total: 1 },
  buckets: [],
  signals: [
    {
      stream_type: 'logs',
      status: 'healthy',
      total_streams: 1,
      active_streams: 1,
      rows: 48_210,
      stored_bytes: 2_150_000,
      last_received_at_micros: NOW - 12_000_000,
    },
  ],
  streams: [
    {
      id: 'logs/default',
      name: 'default',
      stream_type: 'logs',
      status: 'healthy',
      rows: 48_210,
      stored_bytes: 2_150_000,
      first_received_at_micros: NOW - 86_400_000_000,
      last_received_at_micros: NOW - 12_000_000,
    },
  ],
};

const guide = (page: Page) => page.locator('[data-datasource-guide]');
const rows = (page: Page) => page.locator('[data-datasource-source]');
const signals = (page: Page) => page.getByRole('group', { name: 'Signal' });
const search = (page: Page) => page.getByPlaceholder('Search all sources…');

async function useLanguage(page: Page, language: 'en-us' | 'zh-cn') {
  await page.route('**/api/v1/me/preferences', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({
          json: {
            theme: 'system',
            density: 'normal',
            language,
            default_home_route: '/home',
            time_format: 'iso_24h',
            date_format: 'yyyy_mm_dd_dash',
            timezone: '',
            keyboard_shortcuts_enabled: true,
          },
        })
      : route.fallback(),
  );
}

test.describe('Datasource page', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('says each thing once: one navigation card, one list, one guide, one primary action', async ({
    page,
  }) => {
    await page.goto('/datasource/recommended/kubernetes');
    await expect(guide(page)).toBeVisible();

    // The title row is a title: no trail above it and nothing to press beside it.
    const header = page.getByTestId('page-header');
    await expect(header.getByRole('button')).toHaveCount(0);
    await expect(header.getByRole('navigation')).toHaveCount(0);

    // Categories and the signal in view share the navigation card.
    const navigation = page.locator('[data-datasource-navigation="surface"]');
    await expect(navigation.getByRole('link', { name: 'Recommended' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    await expect(navigation.locator('[data-module-tabs-end]').getByRole('group')).toHaveAccessibleName(
      'Signal',
    );
    await expect(signals(page).getByRole('button', { name: 'All' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );

    // A row names a source and its signals, and claims nothing about receipt.
    await expect(rows(page).first()).toHaveText(/^Kubernetes\s*Logs$/);
    await expect(page.locator('[data-datasource-catalog-surface]')).not.toContainText(
      /Unverified|Signal active|Verified/,
    );

    // Three steps, numbered once; nothing inside a step is numbered again.
    await expect(guide(page).locator('[data-guide-section] h3')).toHaveText([
      'Endpoint and token',
      'Set up the source',
      'Verify data is arriving',
    ]);
    await expect(guide(page)).not.toContainText(/\b0[1-9]\b/);
    await expect(page.getByRole('button', { name: 'Verify connection' })).toHaveCount(1);
    await expect(page.getByRole('checkbox')).toHaveCount(0);
  });

  test('sets every title of the guide on one edge and ends its fields and code together', async ({
    page,
  }) => {
    await page.goto('/datasource/recommended/kubernetes');
    await expect(guide(page).locator('pre').first()).toBeVisible();

    const lefts = await guide(page)
      .locator('h2, [data-guide-section] h3, [data-guide-step] h4')
      .evaluateAll((titles) => titles.map((title) => Math.round(title.getBoundingClientRect().left)));
    expect(new Set(lefts).size).toBe(1);

    const rights = await guide(page)
      .locator('[data-guide-connection] [data-ui="read-only-control"], figure')
      .evaluateAll((blocks) => blocks.map((block) => Math.round(block.getBoundingClientRect().right)));
    expect(rights.length).toBeGreaterThanOrEqual(4);
    expect(new Set(rights).size).toBe(1);

    // The list and the guide stand side by side on the page's gutter.
    const [rail, panel] = await Promise.all([
      page.locator('[data-datasource-catalog-surface]').boundingBox(),
      page.locator('[data-datasource-guide-surface]').boundingBox(),
    ]);
    expect(panel!.y).toBe(rail!.y);
    expect(Math.round(panel!.x - (rail!.x + rail!.width))).toBe(12);
  });

  test('finds a source in any category and goes to it', async ({ page }) => {
    await page.goto('/datasource/recommended/kubernetes');
    await expect(guide(page)).toBeVisible();

    await search(page).fill('kafka');

    await expect(rows(page)).toHaveCount(1);
    await expect(rows(page).first()).toContainText('Apache Kafka');
    await expect(rows(page).first()).toContainText('Messaging');
    // The guide in view stays until a result is picked.
    await expect(guide(page).getByRole('heading', { level: 2 })).toHaveText('Kubernetes');

    await search(page).press('Enter');

    await expect(page).toHaveURL(/\/datasource\/queues\/kafka$/);
    await expect(search(page)).toHaveValue('');
    await expect(guide(page).getByRole('heading', { level: 2 })).toHaveText('Apache Kafka');
    await expect(page.getByRole('link', { name: 'Messaging' })).toHaveAttribute(
      'aria-current',
      'page',
    );
  });

  test('narrows to a signal in the address, so it survives a reload and another category', async ({
    page,
  }) => {
    await page.goto('/datasource/recommended/kubernetes');
    await expect(guide(page)).toBeVisible();

    await signals(page).getByRole('button', { name: 'Metrics' }).click();

    // Kubernetes sends logs only: the first source that sends metrics takes its place.
    await expect(page).toHaveURL(/\/datasource\/recommended\/linux\?signal=metrics$/);
    await expect(rows(page)).toHaveCount(2);

    await page.reload();
    await expect(signals(page).getByRole('button', { name: 'Metrics' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );

    await page.getByRole('link', { name: 'Databases' }).click();
    await expect(page).toHaveURL(/\/datasource\/databases\/postgres\?signal=metrics$/);

    await page.getByRole('link', { name: 'Security' }).click();
    await expect(page.getByText('No Metrics sources in Security.')).toBeVisible();
    await page.getByRole('button', { name: 'Show all signals' }).click();
    await expect(page).toHaveURL(/\/datasource\/security\/falco$/);
  });

  test('checks receipt in one place and points onward once data has arrived', async ({ page }) => {
    await page.route(/\/api\/v1\/home\/overview(?:\?.*)?$/, (route) =>
      route.fulfill({ json: RECEIVING_LOGS }),
    );
    await page.goto('/datasource/recommended/kubernetes');

    const verification = page.locator('[data-verification]');
    // What the workspace already receives is shown before anyone asks.
    await expect(verification).toHaveAttribute('data-verification', 'idle');
    await expect(verification.locator('[data-verification-facts]')).toContainText('48.2K rows');

    await page.getByRole('button', { name: 'Verify connection' }).click();

    await expect(verification).toHaveAttribute('data-verification', 'success');
    await expect(verification).toContainText('Data detected');
    await expect(page.locator('[data-guide-section="2"]')).toHaveAttribute('data-state', 'done');
    await expect(page.locator('[data-guide-section="3"]')).toHaveAttribute('data-state', 'done');

    await verification.getByRole('button', { name: 'View received data' }).click();
    await expect(page).toHaveURL(/\/logs(?:[?#]|$)/);
  });

  test('explains an empty check instead of leaving a row of dashes', async ({ page }) => {
    await page.goto('/datasource/recommended/kubernetes');

    const verification = page.locator('[data-verification]');
    await expect(verification.locator('[data-verification-facts]')).toHaveCount(0);

    await page.getByRole('button', { name: 'Verify connection' }).click();

    await expect(verification).toHaveAttribute('data-verification', 'warning');
    await expect(page.locator('[data-guide-section="3"]')).toHaveAttribute(
      'data-state',
      'attention',
    );
    await expect(page.locator('[data-verification-causes]').getByRole('listitem')).toHaveCount(3);
    await expect(page.getByRole('button', { name: 'Send test event' })).toBeVisible();
    await expect(verification).not.toContainText('—');
  });

  test('reads in the language of the interface, catalogue included', async ({ page }) => {
    await useLanguage(page, 'en-us');
    await page.goto('/datasource/recommended/linux');
    await expect(guide(page).getByRole('heading', { level: 2 })).toHaveText('Linux hosts');
    await expect(guide(page).locator('[data-guide-step] h4').first()).toHaveText('Install Vector');
    // No Chinese left in an English page.
    expect(await page.locator('#main').innerText()).not.toMatch(/[一-鿿]/);

    await page.unroute('**/api/v1/me/preferences');
    await useLanguage(page, 'zh-cn');
    await page.reload();
    await expect(guide(page).getByRole('heading', { level: 2 })).toHaveText('Linux 主机');
    await expect(guide(page).locator('[data-guide-step] h4').first()).toHaveText('安装 Vector');
    await expect(page.locator('[data-datasource-navigation="surface"]')).toContainText('基础设施');
  });
});
