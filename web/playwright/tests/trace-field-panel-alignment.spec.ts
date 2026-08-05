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
        items: [],
        next_cursor: null,
        previous_cursor: null,
        has_more: false,
      },
    }),
  );
});

test('aligns trace field controls without an empty leading action slot', async ({ page }) => {
  await page.goto('/traces');

  const panel = page.locator('aside[data-variant="utility"]');
  const rootAdd = panel.getByRole('button', { name: 'Add scope to query' });
  const rootLabel = panel.getByText('scope', { exact: true });
  const namespaceToggle = panel.getByRole('button', { name: 'Expand molesignal' });
  const namespaceLabel = namespaceToggle.getByText('molesignal', { exact: true });

  await rootAdd.scrollIntoViewIfNeeded();
  await expect(rootAdd).toBeVisible();
  await expect(namespaceToggle).toBeVisible();

  const [panelBox, rootAddBox, rootLabelBox, namespaceLabelBox] = await Promise.all([
    boundingBox(panel),
    boundingBox(rootAdd),
    boundingBox(rootLabel),
    boundingBox(namespaceLabel),
  ]);

  expect(rootAddBox.x - panelBox.x).toBeLessThanOrEqual(12);
  expect(Math.abs(rootLabelBox.x - namespaceLabelBox.x)).toBeLessThanOrEqual(1);

  await namespaceToggle.click();
  const nestedAdd = panel.getByRole('button', {
    name: 'Add molesignal.compaction.level to query',
  });
  await expect(nestedAdd).toBeVisible();
  const nestedAddBox = await boundingBox(nestedAdd);

  expect(nestedAddBox.x - rootAddBox.x).toBeGreaterThanOrEqual(11);
  expect(nestedAddBox.x - rootAddBox.x).toBeLessThanOrEqual(15);
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
  expect((await boundingBox(panel)).width).toBe(240);
  await expect(separator).toHaveAttribute('aria-valuenow', '240');

  await separator.dblclick();
  expect((await boundingBox(panel)).width).toBe(240);
});
