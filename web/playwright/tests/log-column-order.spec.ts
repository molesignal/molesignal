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
  await page.route('**/api/v1/field_masking/effective/**', (route) =>
    route.fulfill({ json: { fields: [] } }),
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
    '_timestamp',
    'message',
    'level',
    'service.name',
    'trace_id',
  ]);
  const logsRow = page.locator('[data-log-result-row="logs"]').first();
  await expect(logsRow.locator(':scope > *').nth(1)).toHaveText(LOG_MESSAGE);
  await expect(logsRow.locator(':scope > *').nth(2)).toHaveText('INFO');
});

test('restores a hidden log column to its original position', async ({ page }) => {
  await page.goto('/logs');
  await expect(page.getByText(LOG_MESSAGE)).toBeVisible();

  const logsHeader = page.locator('[data-log-result-columns="logs"]');
  await expect(logsHeader.locator(':scope > *')).toHaveText([
    '_timestamp',
    'level',
    'service.name',
    'message',
    'trace_id',
  ]);

  await page.getByRole('button', { name: 'Columns' }).click();
  await page.getByRole('menuitemcheckbox', { name: /^Reorder level\./ }).click();
  await expect(logsHeader.locator(':scope > *')).toHaveText([
    '_timestamp',
    'service.name',
    'message',
    'trace_id',
  ]);

  await page.getByRole('menuitemcheckbox', { name: 'level', exact: true }).click();
  await expect(logsHeader.locator(':scope > *')).toHaveText([
    '_timestamp',
    'level',
    'service.name',
    'message',
    'trace_id',
  ]);
});

test('timestamp header omits the redundant timezone suffix', async ({ page }) => {
  await page.goto('/logs');
  await expect(page.getByText(LOG_MESSAGE)).toBeVisible();

  const header = page.locator('[data-log-result-columns="logs"] [data-log-field="_timestamp"]');
  const value = page.locator('[data-log-result-row="logs"] [data-log-field="_timestamp"]').first();
  await expect(header).toHaveText('_timestamp');
  await expect(value).not.toBeEmpty();
  await expect(page.getByRole('combobox', { name: 'Page timezone' })).toHaveCount(0);
});

test('result columns adapt to content and cap their width; the message takes the room that is left', async ({ page }) => {
  await page.setViewportSize({ width: 1600, height: 900 });
  await page.goto('/logs');
  await expect(page.getByText(LOG_MESSAGE)).toBeVisible();

  const header = page.locator('[data-log-result-columns="logs"]');
  const box = (field: string) =>
    header.locator(`[data-log-field="${field}"]`).evaluate((element) => {
      const rect = element.getBoundingClientRect();
      return { left: rect.left, right: rect.right, width: rect.width };
    });
  const [time, level, service, message, trace, table] = await Promise.all([
    box('_timestamp'),
    box('level'),
    box('service.name'),
    box('message'),
    box('trace_id'),
    header.evaluate((element) => {
      const rect = element.getBoundingClientRect();
      return { left: rect.left, right: rect.right, width: rect.width };
    }),
  ]);

  expect(time.width).toBeLessThanOrEqual(161);
  expect(level.width).toBeLessThanOrEqual(73);
  expect(service.width).toBeLessThanOrEqual(133);
  expect(trace.width).toBeLessThanOrEqual(161);
  expect(time.width + level.width + service.width).toBeLessThan(365);
  // No room stands empty beside the message: the last column ends where the table does.
  expect(message.width).toBeGreaterThan(301);
  expect(table.right - trace.right).toBeLessThanOrEqual(17);

  // Where the table is narrow the message is cut, with an ellipsis, and not below its least width.
  await page.setViewportSize({ width: 860, height: 900 });
  const value = page.locator('[data-log-result-row="logs"]')
    .first()
    .locator('[data-log-field="message"] > span');
  await expect
    .poll(() => value.evaluate((element) => element.scrollWidth > element.clientWidth))
    .toBe(true);
  expect(await value.evaluate((element) => getComputedStyle(element).textOverflow)).toBe('ellipsis');
  expect((await box('message')).width).toBeGreaterThanOrEqual(200);
});

test('result actions share the event summary row and download CSV or LOG text', async ({ page }) => {
  await page.goto('/logs');
  await expect(page.getByText(LOG_MESSAGE)).toBeVisible();

  const summary = page.locator('[data-log-result-summary]');
  await expect(page.locator('[data-log-result-mode]')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Table', exact: true })).toHaveCount(0);
  const actions = summary.locator('[data-log-result-actions]');
  const actionButtons = actions.getByRole('button');
  // Rows are one height and the field list folds from its own head: neither has a control here.
  // All three are signs without a word; each is named by what it does.
  await expect(actionButtons).toHaveText(['', '', '']);
  await expect(actionButtons.nth(0)).toHaveAccessibleName('Columns');
  await expect(actionButtons.nth(1)).toHaveAccessibleName('Hide histogram');
  await expect(actionButtons.nth(2)).toHaveAccessibleName('Download logs');

  await actionButtons.nth(2).click();
  const csvDownloadPromise = page.waitForEvent('download');
  await page.getByRole('menuitem', { name: 'CSV', exact: true }).click();
  const csvDownload = await csvDownloadPromise;
  expect(csvDownload.suggestedFilename()).toMatch(/^molesignal-logs-.*\.csv$/);
  const csvPath = await csvDownload.path();
  expect(csvPath).not.toBeNull();
  await expect(readFile(csvPath!, 'utf8')).resolves.toContain(
    '"_timestamp","level","service.name","message","trace_id"',
  );

  await actionButtons.nth(2).click();
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

test('expanded field values omit the redundant value and count header', async ({ page }) => {
  await page.goto('/logs');
  await expect(page.getByText(LOG_MESSAGE)).toBeVisible();

  const panel = page.locator('aside[data-variant="utility"]');
  const fieldRow = panel.locator('[data-log-field-row="level"]');
  await fieldRow.locator('button').first().click();

  const values = panel.locator('[data-log-field-values="level"]');
  await expect(values.getByText('INFO', { exact: true })).toBeVisible();
  await expect(values.getByText('Top values', { exact: true })).toHaveCount(0);
  await expect(values.getByText('Count', { exact: true })).toHaveCount(0);
});
