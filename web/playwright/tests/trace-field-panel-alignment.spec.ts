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

/** The fields the stream keeps an index on. */
const INDEXED = new Set(['trace_id', 'service.name']);

const field = (name: string, dataType: 'utf8' | 'int64' = 'utf8') => ({
  name,
  data_type: dataType,
  nullable: true,
  indexed: INDEXED.has(name),
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
  // The list is divided by what a field is to the reader: kept in an index, or not. The Spans view
  // has no place to put a field on display, so it has no such part.
  const sections = panel.locator('[data-side-panel-section]');
  await expect(sections.getByRole('button', { name: /^(Displayed|Indexed|Other) fields/ })).toHaveText([
    'Indexed fields2',
    'Other fields12',
  ]);
  await expect(sections.nth(0).locator('[data-trace-field]')).toHaveCount(2);
  await expect(sections.nth(0).locator('[data-trace-field="service.name"]')).toBeVisible();
  await expect(sections.nth(1).locator('[data-trace-field="span_id"]')).toBeVisible();

  // A part folds away under its heading, and comes back.
  const indexedHeading = sections.nth(0).getByRole('button', { name: /^Indexed fields/ });
  await indexedHeading.click();
  await expect(indexedHeading).toHaveAttribute('aria-expanded', 'false');
  await expect(panel.locator('[data-trace-field="service.name"]')).toHaveCount(0);
  await indexedHeading.click();
  await expect(panel.locator('[data-trace-field="service.name"]')).toBeVisible();

  // A field is a small card lifted off the ground of the list.
  const card = panel.locator('[data-trace-field="service.name"]');
  await expect(card).not.toHaveCSS('box-shadow', 'none');
  expect(await card.evaluate((element) => getComputedStyle(element).backgroundColor)).not.toBe(
    await card.evaluate((element) => getComputedStyle(element.closest('[data-side-panel-section]')!.parentElement!).backgroundColor),
  );

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

  // Open, the values are inside the field's card, under its head, set in from the card's edges
  // and on a ground of their own: what belongs to a field is seen inside it. The click left the
  // button focused; it has no fill of its own, which would light only the part of the row it covers.
  const row = fieldRow.locator('[data-trace-field-row]');
  const toggle = fieldRow.locator('button').first();
  await page.mouse.move(0, 0);
  const [cardBox, rowBox, valuesBox] = await Promise.all([boundingBox(fieldRow), boundingBox(row), boundingBox(values)]);
  expect(valuesBox.x).toBeGreaterThan(cardBox.x);
  expect(valuesBox.x + valuesBox.width).toBeLessThan(cardBox.x + cardBox.width);
  expect(valuesBox.y).toBeGreaterThanOrEqual(rowBox.y + rowBox.height);
  expect(valuesBox.y + valuesBox.height).toBeLessThan(cardBox.y + cardBox.height);
  const background = (locator: Locator) =>
    locator.evaluate((element) => getComputedStyle(element).backgroundColor);
  expect(await background(values)).not.toBe('rgba(0, 0, 0, 0)');
  expect(await background(row)).not.toBe(await background(values));
  expect(await background(toggle)).toBe('rgba(0, 0, 0, 0)');

  // Under the keyboard the whole row is lit, not a piece of it.
  await page.keyboard.press('Tab');
  await page.keyboard.press('Shift+Tab');
  await expect(toggle).toBeFocused();
  expect(await background(toggle)).toBe('rgba(0, 0, 0, 0)');
  expect(await background(row)).not.toBe('rgba(0, 0, 0, 0)');
});

test('lists the attributes that share the first part of their names under it', async ({ page }) => {
  await page.goto('/traces');

  const panel = page.locator('aside[data-variant="utility"]');
  // Two attributes of `molesignal` are one entry that says how many they are; the fields a
  // trace is told by keep their own rows, `service.name` among them, which shares its first
  // part with no other field.
  const group = panel.locator('[data-field-group="molesignal"]');
  const toggle = group.getByRole('button', { name: 'molesignal: 2 fields' });
  await expect(toggle).toHaveAttribute('aria-expanded', 'false');
  await expect(group.locator('[data-field-group-count]')).toHaveText('2');
  await expect(panel.locator('[data-trace-field="molesignal.compaction.level"]')).toHaveCount(0);
  await expect(panel.locator('[data-trace-field="service.name"] [data-trace-field-row]')).toHaveText('Tservice.name');
  await expect(panel.locator('[data-field-group="service"]')).toHaveCount(0);

  // Open, its fields are inside its card by the rest of their names, each a row on a ground of its own.
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  const fields = group.locator('[data-field-group-fields="molesignal"]');
  const level = fields.locator('[data-trace-field="molesignal.compaction.level"]');
  await expect(fields.locator('[data-trace-field-row]')).toHaveText(['Tcompaction.level', '#db.pool.size']);
  const [groupBox, levelBox] = await Promise.all([boundingBox(group), boundingBox(level)]);
  expect(levelBox.x).toBeGreaterThan(groupBox.x);
  expect(levelBox.x + levelBox.width).toBeLessThan(groupBox.x + groupBox.width);
  expect(levelBox.y + levelBox.height).toBeLessThan(groupBox.y + groupBox.height);
  const ground = (locator: Locator) => locator.evaluate((element) => getComputedStyle(element).backgroundColor);
  expect(await ground(level)).not.toBe('rgba(0, 0, 0, 0)');
  expect(await ground(level)).not.toBe(await ground(group));
  // The group is one card; the fields in it are not cards of their own.
  await expect(group).not.toHaveCSS('box-shadow', 'none');
  await expect(level).toHaveCSS('box-shadow', 'none');
  // The whole name is a pointer away.
  await expect(level.getByText('compaction.level', { exact: true })).toHaveAttribute('title', 'molesignal.compaction.level');

  // A search cuts the list to what matches and opens it: one match is a field of its own again.
  await toggle.click();
  await panel.getByRole('textbox', { name: 'Filter trace fields' }).fill('pool');
  await expect(panel.locator('[data-field-group]')).toHaveCount(0);
  await expect(panel.locator('[data-trace-field="molesignal.db.pool.size"] [data-trace-field-row]')).toHaveText(
    '#molesignal.db.pool.size',
  );
});

test('reads a span field from the loaded traces and switches a value filter on and off', async ({ page }) => {
  const statements: string[] = [];
  await page.route('**/api/v1/query', async (route) => {
    const statement = String(route.request().postDataJSON().statement);
    statements.push(statement);
    await route.fulfill({
      json: statement.includes('distinct_values')
        ? { columns: ['distinct_values', 'traces', 'samples'], rows: [[2, 2, 23]], scanned_rows: 23, took_ms: 1 }
        : {
            columns: ['value', 'traces', 'spans'],
            rows: [['checkout-api', 2, 20], ['payments', 1, 3]],
            scanned_rows: 23,
            took_ms: 1,
          },
    });
  });
  const listed: URL[] = [];
  page.on('request', (request) => {
    if (request.url().includes('/api/v1/web/traces')) listed.push(new URL(request.url()));
  });
  const filters = () => listed.at(-1)?.searchParams.get('filters') ?? null;

  await page.goto('/traces');

  const panel = page.locator('aside[data-variant="utility"]');
  const fieldRow = panel.locator('[data-trace-field="service.name"]');
  // A row says nothing it does not know: the count of loaded traces used to stand on every one.
  await expect(fieldRow.locator('[data-trace-field-row]')).toHaveText('Tservice.name');
  await fieldRow.locator('button').first().click();

  // The values are of the spans of the traces on the page, counted in those traces.
  const values = panel.locator('[data-trace-field-values="service.name"]');
  await expect(values.locator('[data-trace-field-summary]')).toHaveText('2 values · in 2 of 2 traces');
  await expect(values.locator('[data-field-value]')).toHaveCount(2);
  expect(statements.some((statement) => statement.includes("WHERE trace_id IN ('trace-1', 'trace-2')"))).toBe(true);

  // The bar under a value is its share of the loaded traces: half of its track for one trace of two.
  const payments = values.locator('[data-field-value="payments"]');
  const [trackBox, shareBox] = await Promise.all([
    boundingBox(payments.locator('[data-field-value-track]')),
    boundingBox(payments.locator('[data-field-value-share]')),
  ]);
  expect(Math.abs(shareBox.width - trackBox.width / 2)).toBeLessThanOrEqual(1);
  // A bar of its own under the words, not a tint behind them.
  expect(trackBox.height).toBe(3);
  const wordsBox = await boundingBox(payments.locator('[title="payments"]'));
  expect(trackBox.y).toBeGreaterThanOrEqual(wordsBox.y + wordsBox.height);

  // A field query matches a trace when any span does, so it cannot leave a span's value out:
  // only the filter that means what it says is offered.
  await payments.hover();
  await expect(payments.getByRole('button')).toHaveCount(1);
  const filter = payments.getByRole('button', { name: 'Filter for service.name = payments' });
  await expect(filter).toHaveAttribute('aria-pressed', 'false');
  await filter.click();

  // The filter is whole, so it is written and run at once, and the view stays where it was.
  await expect.poll(filters).toBe(JSON.stringify([{ field: 'service.name', op: '=', value: 'payments' }]));
  await expect(filter).toHaveAttribute('aria-pressed', 'true');
  await expect(page.locator('[data-query-workbench-appearance="surface"]')).toContainText("service.name = 'payments'");
  await expect(page.getByRole('tab', { name: /Spans/ })).toHaveAttribute('aria-selected', 'true');

  // On, the button stays in view without the pointer, and takes the filter out again.
  await page.mouse.move(0, 0);
  await expect(filter).toHaveCSS('opacity', '1');
  await filter.click();
  await expect(filter).toHaveAttribute('aria-pressed', 'false');
  await expect(page.locator('[data-query-workbench-appearance="surface"]')).not.toContainText('payments');
});

test('reads what a trace has one of from the loaded rows, and a measure as a range', async ({ page }) => {
  let queried = 0;
  await page.route('**/api/v1/query', (route) => {
    queried += 1;
    return route.fulfill({ json: { columns: [], rows: [], scanned_rows: 0, took_ms: 0 } });
  });
  await page.goto('/traces?tab=traces');

  const panel = page.locator('aside[data-variant="utility"]');
  const spanCount = panel.locator('[data-trace-field="span_count"]');
  await spanCount.locator('button').first().click();
  const values = panel.locator('[data-trace-field-values="span_count"]');
  await expect(values.locator('[data-trace-field-summary]')).toHaveText('2 values · in 2 of 2 traces');

  // A trace has one count, so leaving a value out means what it says here.
  const eleven = values.locator('[data-field-value="11"]');
  await eleven.hover();
  await expect(eleven.getByRole('button', { name: 'Filter for span_count = 11' })).toBeVisible();
  await expect(eleven.getByRole('button', { name: 'Filter out span_count = 11' })).toBeVisible();
  expect(queried).toBe(0);

  // The eye picks what stands beside a picked trace, which this view has.
  await spanCount.locator('[data-trace-field-row]').hover();
  await expect(spanCount.getByRole('button', { name: 'Hide span_count' })).toBeVisible();
  await expect(panel.locator('[data-trace-field="span_id"]').getByRole('button', { name: /^(Show|Hide) / })).toHaveCount(0);
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

test('uses canvas gutters and surface depth for the trace workbench hierarchy', async ({ page }) => {
  await page.goto('/traces');

  const shell = page.locator('[data-shell-layout="surface-workbench"]');
  const topbar = page.getByRole('banner');
  const sidebar = page.getByTestId('primary-sidebar');
  const canvas = page.locator('[data-workspace="traces"]');
  const querySurface = page.locator('[data-query-workbench-appearance="surface"]');
  // The explorer has no title row: its name sits at the start of the workbench bar.
  const barTitle = querySurface.locator('[data-workbench-title]');
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
  await expect(page.getByTestId('page-header')).toHaveCount(0);
  await expect(barTitle.getByTestId('page-header-module-icon')).toBeVisible();
  await expect(barTitle.getByRole('heading', { level: 1, name: 'Traces' })).toBeVisible();
  await expect(topbar).toHaveCSS('border-bottom-width', '0px');
  await expect(sidebar).toHaveCSS('border-right-width', '0px');
  await expect(pagination).toHaveCSS('border-top-width', '0px');
  await expect(syntaxHelp).toHaveCSS('border-width', '0px');
  await expect(timeRange).toHaveCSS('border-width', '0px');
  await expect(activeTab).toHaveAttribute('aria-selected', 'true');
  // The query-toolbar tabs keep their own 3px underline; the module tab strips
  // (`ModuleTabs`) are the ones with the 2px gliding indicator.
  expect(
    await activeTab.evaluate(
      (element) => getComputedStyle(element, '::after').height,
    ),
  ).toBe('3px');
  expect(
    await activeTab.evaluate(
      (element) => getComputedStyle(element, '::after').backgroundColor,
    ),
  ).not.toBe('rgba(0, 0, 0, 0)');
  expect(await workspace.evaluate((element) => getComputedStyle(element).gap)).toBe('8px');
  expect((await boundingBox(fieldPanel)).width).toBe(260);
  expect((await boundingBox(results)).width).toBe(380);

  const [canvasBox, queryBox, workspaceBox] = await Promise.all([
    boundingBox(canvas),
    boundingBox(querySurface),
    boundingBox(workspace),
  ]);
  // The bar sits close under the query tabs that name it (6px), and a 12px gutter
  // sets it off from the panes below.
  const barTopGap = queryBox.y - canvasBox.y;
  const barBottomGap = workspaceBox.y - queryBox.y - queryBox.height;
  expect(barTopGap).toBe(6);
  expect(barBottomGap).toBe(12);

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

  // Logs carries its name in the same place, so moving between the two explorers
  // does not move the title.
  const traceTitleBox = await boundingBox(barTitle);
  await page.goto('/logs');
  const logTitle = page.locator('[data-query-workbench-appearance="surface"] [data-workbench-title]');
  await expect(logTitle.getByRole('heading', { level: 1, name: 'Logs' })).toBeVisible();
  const logTitleBox = await boundingBox(logTitle);
  expect(Math.abs(logTitleBox.x - traceTitleBox.x)).toBeLessThanOrEqual(1);
  expect(Math.abs(logTitleBox.y - traceTitleBox.y)).toBeLessThanOrEqual(1);
  expect(Math.abs(logTitleBox.height - traceTitleBox.height)).toBeLessThanOrEqual(1);
});
