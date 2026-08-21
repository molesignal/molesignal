import type { Locator } from '@playwright/test';
import { expect, test } from '@playwright/test';

import { installMockShellSession } from '../fixtures/mockSession';

const SETTINGS = {
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
};

const field = (name: string, dataType: 'utf8' | 'int64' = 'utf8') => ({
  name,
  data_type: dataType,
  nullable: true,
  indexed: false,
});

const TRACE_STREAM = {
  id: 'trace-fields',
  name: 'default',
  stream_type: 'traces',
  schema: {
    fields: [
      field('trace_id'),
      field('span_id'),
      field('parent_span_id'),
      field('name'),
      field('service.name'),
      field('start_time_unix_nano', 'int64'),
      field('end_time_unix_nano', 'int64'),
      field('duration_ns', 'int64'),
      field('status_code'),
      field('scope'),
      field('molesignal.compaction.level'),
      field('molesignal.db.pool.size', 'int64'),
    ],
  },
  retention: { days: 7 },
  effective_retention: { days: 7 },
  settings: SETTINGS,
  created_at_micros: 1,
  updated_at_micros: 1,
};

async function boundingBox(locator: Locator) {
  const box = await locator.boundingBox();
  expect(box).not.toBeNull();
  return box!;
}

test.beforeEach(async ({ page }) => {
  await installMockShellSession(page);
  await page.route('**/api/v1/streams**', (route) =>
    route.fulfill({ json: [TRACE_STREAM] }),
  );
  await page.route('**/api/v1/web/topology**', (route) =>
    route.fulfill({ json: { nodes: [], edges: [] } }),
  );
  await page.route('**/api/v1/web/traces**', (route) =>
    route.fulfill({
      json: {
        items: [
          {
            trace_id: 'trace-1',
            service: 'checkout-api',
            operation: 'POST /api/orders',
            start_ns: 1_786_704_600_000_000_000,
            duration_ms: 442,
            span_count: 11,
            error_count: 0,
          },
          {
            trace_id: 'trace-2',
            service: 'checkout-api',
            operation: 'POST /api/orders',
            start_ns: 1_786_704_599_000_000_000,
            duration_ms: 2450,
            span_count: 12,
            error_count: 6,
          },
        ],
        next_cursor: null,
        previous_cursor: null,
        has_more: false,
      },
    }),
  );
});

test('matches log field rows and expands values without a redundant header', async ({ page }) => {
  await page.goto('/traces');

  const panel = page.locator('aside[data-variant="utility"]');
  await expect(panel.getByText('Core fields', { exact: true })).toBeVisible();
  await expect(panel.getByText('Attributes', { exact: true })).toBeVisible();
  await expect(panel.getByText('Resource attributes', { exact: true })).toHaveCount(0);
  await expect(panel.locator('[data-trace-field="service.name"]')).toBeVisible();

  const fieldRow = panel.locator('[data-trace-field="trace_id"]');
  const label = fieldRow.getByText('trace_id', { exact: true });
  const add = fieldRow.getByRole('button', { name: 'Add trace_id to query' });
  await fieldRow.hover();
  const [labelBox, addBox] = await Promise.all([boundingBox(label), boundingBox(add)]);
  expect(addBox.x).toBeGreaterThan(labelBox.x + labelBox.width);

  await fieldRow.locator('button').first().click();
  const values = panel.locator('[data-trace-field-values="trace_id"]');
  await expect(values.getByText('trace-1', { exact: true })).toBeVisible();
  await expect(values.getByText('trace-2', { exact: true })).toBeVisible();
  await expect(values.getByText('Top values', { exact: true })).toHaveCount(0);
  await expect(values.getByText('Count', { exact: true })).toHaveCount(0);
});

test('drags the field panel in both directions and clamps its maximum width', async ({ page }) => {
  await page.goto('/traces');

  const panel = page.locator('aside[data-variant="utility"]');
  const separator = panel.getByRole('separator', {
    name: 'Drag to resize fields; double-click to reset',
  });
  await expect(separator).toBeVisible();

  const dragTo = async (clientX: number) => {
    const handleBox = await boundingBox(separator);
    await page.mouse.move(
      handleBox.x + handleBox.width / 2,
      handleBox.y + handleBox.height / 2,
    );
    await page.mouse.down();
    await page.mouse.move(clientX, handleBox.y + handleBox.height / 2);
    await page.mouse.up();
  };

  await dragTo(1_200);
  expect((await boundingBox(panel)).width).toBe(480);
  await expect(separator).toHaveAttribute('aria-valuenow', '480');

  await dragTo(0);
  expect((await boundingBox(panel)).width).toBe(260);
  await expect(separator).toHaveAttribute('aria-valuenow', '260');

  await separator.dblclick();
  expect((await boundingBox(panel)).width).toBe(260);
});

test('uses canvas gutters and borderless surfaces for the trace workbench hierarchy', async ({ page }) => {
  await page.goto('/traces');

  const shell = page.locator('[data-shell-layout="surface-workbench"]');
  const topbar = page.getByRole('banner');
  const sidebar = page.getByTestId('primary-sidebar');
  const canvas = page.locator('[data-workspace="traces"]');
  const pageHeader = page.getByTestId('page-header');
  const querySurface = page.locator('[data-query-workbench-appearance="surface"]');
  const workspace = page.locator('[data-trace-workspace-layout="surface-grid"]');
  const fieldPanel = page.locator('aside[data-variant="utility"]');
  const results = page.locator('[data-workspace-pane="trace-results"]');
  const detail = page.locator('[data-workspace-pane="trace-detail"]');
  const pagination = results.getByRole('navigation');
  const primaryTabs = querySurface.locator('[data-query-tab-selection="underline"]');
  const activeTab = primaryTabs.getByRole('tab', { name: /Spans/ });
  const syntaxHelp = querySurface.getByRole('button', { name: /query syntax reference/i });
  const timeRange = querySurface.getByRole('button', { name: /^Time range:/ });

  await expect(shell).toBeVisible();
  await expect(querySurface).toBeVisible();
  await expect(pageHeader.getByTestId('page-header-module-icon')).toBeVisible();
  await expect(topbar).toHaveCSS('border-bottom-width', '0px');
  await expect(sidebar).toHaveCSS('border-right-width', '0px');
  await expect(pageHeader).toHaveCSS('border-bottom-width', '0px');
  await expect(pagination).toHaveCSS('border-top-width', '0px');
  await expect(syntaxHelp).toHaveCSS('border-width', '0px');
  await expect(timeRange).toHaveCSS('border-width', '0px');
  await expect(activeTab).toHaveAttribute('aria-selected', 'true');
  expect(
    await activeTab.evaluate(
      (element) => getComputedStyle(element, '::after').height,
    ),
  ).toBe('2px');
  expect(
    await activeTab.evaluate(
      (element) => getComputedStyle(element, '::after').backgroundColor,
    ),
  ).not.toBe('rgba(0, 0, 0, 0)');
  expect(await workspace.evaluate((element) => getComputedStyle(element).gap)).toBe('8px');
  expect((await boundingBox(fieldPanel)).width).toBe(260);
  expect((await boundingBox(results)).width).toBe(380);

  const [canvasBox, headerBox, queryBox] = await Promise.all([
    boundingBox(canvas),
    boundingBox(pageHeader),
    boundingBox(querySurface),
  ]);
  const titleTopGap = headerBox.y - canvasBox.y;
  const titleBottomGap = queryBox.y - headerBox.y - headerBox.height;
  expect(titleTopGap).toBe(12);
  expect(titleBottomGap).toBe(titleTopGap);

  const surfaceBackgrounds = await Promise.all(
    [querySurface, fieldPanel, results, detail].map((locator) =>
      locator.evaluate((element) => getComputedStyle(element).backgroundColor),
    ),
  );
  expect(new Set(surfaceBackgrounds).size).toBe(1);
  expect(
    await canvas.evaluate((element) => getComputedStyle(element).backgroundColor),
  ).not.toBe(surfaceBackgrounds[0]);
  await expect(querySurface).not.toHaveCSS('box-shadow', 'none');

  const traceHeaderHeight = headerBox.height;
  await page.goto('/logs');
  const logHeader = page.getByTestId('page-header');
  await expect(logHeader).toBeVisible();
  expect(
    Math.abs((await boundingBox(logHeader)).height - traceHeaderHeight),
  ).toBeLessThanOrEqual(1);
});
