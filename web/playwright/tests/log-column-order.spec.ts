import { readFile } from 'node:fs/promises';

import { expect, test } from '@playwright/test';

import { installMockShellSession } from '../fixtures/mockSession';

const LOG_STREAM = {
  id: 'application-logs',
  name: 'application_logs',
  stream_type: 'logs',
  schema: {
    fields: [
      { name: '_timestamp', data_type: 'timestamp', nullable: false, indexed: true },
      { name: 'level', data_type: 'utf8', nullable: true, indexed: true },
      { name: 'service.name', data_type: 'utf8', nullable: true, indexed: true },
      { name: 'message', data_type: 'utf8', nullable: true, indexed: false },
      { name: 'trace_id', data_type: 'utf8', nullable: true, indexed: true },
    ],
  },
  retention: { days: 7 },
  effective_retention: { days: 7 },
  settings: {
    description: null,
    index_rules: [],
    retention_filter: null,
    keep_conditions: [],
    max_query_range_hours: null,
    flatten_level: null,
    use_stream_stats_for_partitioning: false,
    store_original_data: false,
    enable_distinct_values: true,
    queryable: true,
  },
  created_at_micros: 1,
  updated_at_micros: 1,
};

const LOG_MESSAGE = 'message follows the configured order and keeps going long enough to verify capped column widths';

test.beforeEach(async ({ page }) => {
  await installMockShellSession(page);
  await page.route('**/api/v1/streams**', (route) =>
    route.fulfill({ json: [LOG_STREAM] }),
  );
  await page.route('**/api/v1/web/logs', (route) =>
    route.fulfill({
      json: {
        items: [
          {
            _timestamp: 1_785_628_332_000_000,
            level: 'INFO',
            'service.name': 'checkout-api',
            message: LOG_MESSAGE,
            trace_id: 'trace-1',
          },
        ],
        next_cursor: null,
        previous_cursor: null,
        has_more: false,
      },
    }),
  );
});

test('dragged field order drives result columns', async ({ page }) => {
  await page.goto('/logs');
  await expect(page.getByText(LOG_MESSAGE)).toBeVisible();

  await page.getByRole('button', { name: 'Columns' }).click();
  const messageField = page.getByRole('menuitemcheckbox', { name: /^Reorder message\./ });
  const levelField = page.getByRole('menuitemcheckbox', { name: /^Reorder level\./ });
  await messageField.dragTo(levelField, { targetPosition: { x: 24, y: 2 } });

  const logsHeader = page.locator('[data-log-result-columns="logs"]');
  await expect(logsHeader.locator(':scope > *')).toHaveText([
    '_timestamp (UTC)',
    'message',
    'level',
    'service.name',
    'trace_id',
  ]);
  const logsRow = page.locator('[data-log-result-row="logs"]').first();
  await expect(logsRow.locator(':scope > *').nth(1)).toHaveText(LOG_MESSAGE);
  await expect(logsRow.locator(':scope > *').nth(2)).toHaveText('INFO');
});

test('timestamp header and values follow the page timezone override', async ({ page }) => {
  await page.goto('/logs');
  await expect(page.getByText(LOG_MESSAGE)).toBeVisible();

  const header = page.locator('[data-log-result-columns="logs"] [data-log-field="_timestamp"]');
  const value = page.locator('[data-log-result-row="logs"] [data-log-field="_timestamp"]').first();
  await expect(header).toHaveText('_timestamp (UTC)');
  const utcValue = await value.textContent();

  await page.getByRole('combobox', { name: 'Timezone' }).click();
  await page.getByText(/Asia\/Shanghai · UTC\+8/).click();

  await expect(header).toHaveText('_timestamp (Asia/Shanghai)');
  await expect(value).not.toHaveText(utcValue ?? '');
});

test('result columns adapt to content, cap width, and ellipsize overflow', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto('/logs');
  await expect(page.getByText(LOG_MESSAGE)).toBeVisible();

  const header = page.locator('[data-log-result-columns="logs"]');
  const timeWidth = await header.locator('[data-log-field="_timestamp"]').evaluate((element) => element.getBoundingClientRect().width);
  const levelWidth = await header.locator('[data-log-field="level"]').evaluate((element) => element.getBoundingClientRect().width);
  const serviceWidth = await header.locator('[data-log-field="service.name"]').evaluate((element) => element.getBoundingClientRect().width);
  const messageWidth = await header.locator('[data-log-field="message"]').evaluate((element) => element.getBoundingClientRect().width);

  expect(timeWidth).toBeLessThanOrEqual(161);
  expect(levelWidth).toBeLessThanOrEqual(73);
  expect(serviceWidth).toBeLessThanOrEqual(133);
  expect(messageWidth).toBeLessThanOrEqual(301);
  expect(timeWidth + levelWidth + serviceWidth).toBeLessThan(365);

  const value = page.locator('[data-log-result-row="logs"]')
    .first()
    .locator('[data-log-field="message"] > span');
  const overflow = await value.evaluate((element) => ({
    clientWidth: element.clientWidth,
    scrollWidth: element.scrollWidth,
    textOverflow: getComputedStyle(element).textOverflow,
  }));
  expect(overflow.scrollWidth).toBeGreaterThan(overflow.clientWidth);
  expect(overflow.textOverflow).toBe('ellipsis');
});

test('result actions place Fields after density and download CSV or LOG text', async ({ page }) => {
  await page.goto('/logs');
  await expect(page.getByText(LOG_MESSAGE)).toBeVisible();

  await expect(page.locator('[data-log-result-summary]').getByRole('button')).toHaveCount(0);
  const mode = page.locator('[data-log-result-mode]');
  await expect(mode).toHaveText('Content');
  await expect(mode.getByRole('button')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Table', exact: true })).toHaveCount(0);
  const actions = page.locator('[data-log-result-actions]');
  const actionButtons = actions.getByRole('button');
  await expect(actionButtons).toHaveText(['Columns', 'Compact', 'Fields', '']);
  await expect(actionButtons.nth(2)).toHaveAccessibleName('Collapse fields');
  await expect(actionButtons.nth(3)).toHaveAccessibleName('Download logs');

  await actionButtons.nth(3).click();
  const csvDownloadPromise = page.waitForEvent('download');
  await page.getByRole('menuitem', { name: 'CSV', exact: true }).click();
  const csvDownload = await csvDownloadPromise;
  expect(csvDownload.suggestedFilename()).toMatch(/^molesignal-logs-.*\.csv$/);
  const csvPath = await csvDownload.path();
  expect(csvPath).not.toBeNull();
  await expect(readFile(csvPath!, 'utf8')).resolves.toContain(
    '"_timestamp","level","service.name","message","trace_id"',
  );

  await actionButtons.nth(3).click();
  const logDownloadPromise = page.waitForEvent('download');
  await page.getByRole('menuitem', { name: 'LOG (text)', exact: true }).click();
  const logDownload = await logDownloadPromise;
  expect(logDownload.suggestedFilename()).toMatch(/^molesignal-logs-.*\.log$/);
  const logPath = await logDownload.path();
  expect(logPath).not.toBeNull();
  await expect(readFile(logPath!, 'utf8')).resolves.toBe(
    '1785628332000000 level=INFO service.name=checkout-api '
      + `message=${JSON.stringify(LOG_MESSAGE)} trace_id=trace-1`,
  );
});
