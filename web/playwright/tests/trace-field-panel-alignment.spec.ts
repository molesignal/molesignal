import type { Locator } from '@playwright/test';
import { expect, test } from '@playwright/test';

import { explorerRead, explorerReadBody } from '../fixtures/explorerReads';
import { installMockShellSession } from '../fixtures/mockSession';

const SETTINGS = {
  description: null,
  indexRules: [],
  retentionFilter: null,
  keepConditions: [],
  maxQueryRangeHours: null,
  flattenLevel: null,
  useStreamStatsForPartitioning: false,
  storeOriginalData: false,
  enableDistinctValues: true,
  queryable: true,
};

/** The fields the stream keeps an index on. */
const INDEXED = new Set(['trace_id', 'service.name']);

const field = (name: string, dataType: 'utf8' | 'int64' = 'utf8') => ({
  name,
  dataType: dataType,
  nullable: true,
  indexed: INDEXED.has(name),
});

const TRACE_STREAM = {
  id: 'trace-fields',
  name: 'default',
  streamType: 'traces',
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
  effectiveRetention: { days: 7 },
  settings: SETTINGS,
  createdAt: 1,
  updatedAt: 1,
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
            traceId: 'trace-1',
            service: 'checkout-api',
            operation: 'POST /api/orders',
            startNs: 1_786_704_600_000_000_000,
            durationMs: 442,
            spanCount: 11,
            errorCount: 0,
          },
          {
            traceId: 'trace-2',
            service: 'checkout-api',
            operation: 'POST /api/orders',
            startNs: 1_786_704_599_000_000_000,
            durationMs: 2450,
            spanCount: 12,
            errorCount: 6,
          },
        ],
        nextCursor: null,
        previousCursor: null,
        hasMore: false,
      },
    }),
  );
});

test('lists the fields as rows under headings like the table head, and opens values under a row', async ({ page }) => {
  await page.goto('/traces');

  const panel = page.locator('aside[data-variant="utility"]');
  // The fields a trace is first narrowed by come first, then those kept in an index, then the rest.
  const sections = panel.locator('[data-side-panel-section]');
  await expect(sections.getByRole('button', { name: /^(Common|Displayed|Indexed|Other) fields/ })).toHaveText([
    'Common fields4',
    'Indexed fields1',
    'Other fields9',
  ]);
  await expect(sections.nth(0).locator('[data-trace-field]')).toHaveCount(4);
  await expect(sections.nth(0).locator('[data-trace-field="service.name"]')).toBeVisible();
  await expect(sections.nth(1).locator('[data-trace-field="trace_id"]')).toBeVisible();

  // A part folds away under its heading, and comes back.
  const indexedHeading = sections.nth(1).getByRole('button', { name: /^Indexed fields/ });
  await indexedHeading.click();
  await expect(indexedHeading).toHaveAttribute('aria-expanded', 'false');
  await expect(panel.locator('[data-trace-field="trace_id"]')).toHaveCount(0);
  await indexedHeading.click();
  await expect(panel.locator('[data-trace-field="trace_id"]')).toBeVisible();

  // A heading of the list stands on the panel, without a band of its own; the table's head is the
  // band every table head is.
  const background = (locator: Locator) => locator.evaluate((element) => getComputedStyle(element).backgroundColor);
  expect(await background(indexedHeading)).toBe('rgba(0, 0, 0, 0)');
  const band = await page.evaluate(() => {
    const probe = document.createElement('span');
    probe.style.backgroundColor = 'var(--table-head-surface)';
    document.body.append(probe);
    const fill = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return fill;
  });
  expect(await background(page.locator('[data-trace-table-head]'))).toBe(band);

  // A field is a row of the list from edge to edge with a line under it, not a card.
  const fieldRow = panel.locator('[data-trace-field="trace_id"]');
  await expect(fieldRow).toHaveCSS('box-shadow', 'none');
  await expect(fieldRow).toHaveCSS('border-bottom-width', '1px');
  const [rowBox, sectionBox] = await Promise.all([boundingBox(fieldRow), boundingBox(sections.nth(1))]);
  expect(Math.abs(rowBox.width - sectionBox.width)).toBeLessThanOrEqual(1);

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

  // Open, the values are under the field's line, as wide as the list, on a ground of their own.
  // The click left the button focused; it has no fill of its own, which would light only the part
  // of the line it covers.
  const row = fieldRow.locator('[data-trace-field-row]');
  const toggle = fieldRow.locator('button').first();
  await page.mouse.move(0, 0);
  const [lineBox, valuesBox] = await Promise.all([boundingBox(row), boundingBox(values)]);
  expect(valuesBox.y).toBeGreaterThanOrEqual(lineBox.y + lineBox.height);
  expect(Math.abs(valuesBox.width - lineBox.width)).toBeLessThanOrEqual(1);
  expect(await background(values)).not.toBe('rgba(0, 0, 0, 0)');
  expect(await background(row)).not.toBe(await background(values));
  expect(await background(toggle)).toBe('rgba(0, 0, 0, 0)');

  // Under the keyboard the whole line is lit, not a piece of it.
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

  // Open, its fields are rows of the list under its own, by the rest of their names, set in further.
  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-expanded', 'true');
  const fields = group.locator('[data-field-group-fields="molesignal"]');
  const level = fields.locator('[data-trace-field="molesignal.compaction.level"]');
  await expect(fields.locator('[data-trace-field-row]')).toHaveText(['Tcompaction.level', '#db.pool.size']);
  const [toggleBox, levelBox] = await Promise.all([boundingBox(toggle), boundingBox(level)]);
  expect(Math.abs(levelBox.width - toggleBox.width)).toBeLessThanOrEqual(1);
  expect(levelBox.y).toBeGreaterThanOrEqual(toggleBox.y + toggleBox.height - 1);
  const [groupName, levelName] = await Promise.all([
    boundingBox(toggle.getByText('molesignal', { exact: true })),
    boundingBox(level.getByText('compaction.level', { exact: true })),
  ]);
  expect(levelName.x).toBeGreaterThan(groupName.x);
  // Neither the group nor the fields in it are cards.
  await expect(group).toHaveCSS('box-shadow', 'none');
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
  await page.route(explorerRead('query'), async (route) => {
    const statement = String(explorerReadBody(route.request()).statement);
    statements.push(statement);
    await route.fulfill({
      json: statement.includes('distinct_values')
        ? { columns: ['distinct_values', 'traces', 'samples'], rows: [[2, 2, 23]], scannedRows: 23, tookMs: 1 }
        : {
            columns: ['value', 'traces', 'spans'],
            rows: [['checkout-api', 2, 20], ['payments', 1, 3]],
            scannedRows: 23,
            tookMs: 1,
          },
    });
  });
  const listed: URL[] = [];
  page.on('request', (request) => {
    if (request.url().includes('/api/v1/web/traces')) listed.push(new URL(request.url()));
  });
  const filters = () => listed.at(-1)?.searchParams.get('filters') ?? null;

  await page.goto('/traces?tab=spans');

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

  // A field query keeps a trace when one of its spans has the value, and with `!=` when none has
  // it: a value of a span is offered both ways.
  await payments.hover();
  await expect(payments.getByRole('button')).toHaveCount(2);
  const filter = payments.getByRole('button', { name: 'Filter for service.name = payments' });
  const filterOut = payments.getByRole('button', { name: 'Filter out service.name = payments' });
  await expect(filterOut).toHaveAttribute('aria-pressed', 'false');
  await filterOut.click();

  // The filter is whole, so it is written and run at once, and the view stays where it was.
  const workbench = page.locator('[data-query-workbench-appearance="surface"]');
  await expect.poll(filters).toBe(JSON.stringify([{ field: 'service.name', op: '!=', value: 'payments' }]));
  await expect(filterOut).toHaveAttribute('aria-pressed', 'true');
  await expect(workbench).toContainText("service.name != 'payments'");
  await expect(page.getByRole('tab', { name: /Spans/ })).toHaveAttribute('aria-selected', 'true');

  // Asked for instead, the value takes the place of the filter that left it out.
  await expect(filter).toHaveAttribute('aria-pressed', 'false');
  await filter.click();
  await expect.poll(filters).toBe(JSON.stringify([{ field: 'service.name', op: '=', value: 'payments' }]));
  await expect(filter).toHaveAttribute('aria-pressed', 'true');
  await expect(filterOut).toHaveAttribute('aria-pressed', 'false');
  await expect(workbench).toContainText("service.name = 'payments'");
  await expect(workbench).not.toContainText('!=');

  // On, the button stays in view without the pointer, and takes the filter out again.
  await page.mouse.move(0, 0);
  await expect(filter).toHaveCSS('opacity', '1');
  await filter.click();
  await expect(filter).toHaveAttribute('aria-pressed', 'false');
  await expect(workbench).not.toContainText('payments');
});

test('reads what a trace has one of from the loaded rows, and a measure as a range', async ({ page }) => {
  let queried = 0;
  await page.route(explorerRead('query'), (route) => {
    queried += 1;
    return route.fulfill({ json: { columns: [], rows: [], scannedRows: 0, tookMs: 0 } });
  });
  await page.goto('/traces');

  const panel = page.locator('aside[data-variant="utility"]');
  const spanCount = panel.locator('[data-trace-field="span_count"]');
  await spanCount.locator('button').first().click();
  const values = panel.locator('[data-trace-field-values="span_count"]');
  await expect(values.locator('[data-trace-field-summary]')).toHaveText('2 values · in 2 of 2 traces');

  // A trace has one count, which either filter compares on the trace itself.
  const eleven = values.locator('[data-field-value="11"]');
  await eleven.hover();
  await expect(eleven.getByRole('button', { name: 'Filter for span_count = 11' })).toBeVisible();
  await expect(eleven.getByRole('button', { name: 'Filter out span_count = 11' })).toBeVisible();
  expect(queried).toBe(0);

  // The eye puts a field of each trace's root span beside the traces as a column; a count the
  // trace carries is a column of its own, and a span's id is no field of the root to show.
  const eye = /^(Show .* as a column|Remove the .* column)$/;
  await spanCount.locator('[data-trace-field-row]').hover();
  await expect(spanCount.getByRole('button', { name: eye })).toHaveCount(0);
  await expect(panel.locator('[data-trace-field="span_id"]').getByRole('button', { name: eye })).toHaveCount(0);
  const start = panel.locator('[data-trace-field="start_time_unix_nano"]');
  await start.locator('[data-trace-field-row]').hover();
  await expect(start.getByRole('button', { name: "Show start_time_unix_nano of each trace's root span as a column" })).toBeVisible();
});

test('counts on each tab what it has loaded: the traces, and the spans once they are listed', async ({ page }) => {
  await page.route('**/api/v1/web/traces/spans**', (route) =>
    route.fulfill({
      json: {
        items: ['a', 'b', 'c'].map((spanId, index) => ({
          traceId: 'trace-1',
          spanId: spanId,
          service: 'checkout-api',
          operation: 'POST /api/orders',
          startNs: 1_786_704_600_000_000_000 + index * 1_000_000,
          durationNs: 1_000_000,
          status: 'OK',
        })),
        nextCursor: null,
        previousCursor: null,
        hasMore: false,
      },
    }),
  );
  await page.goto('/traces');

  const tabs = page.locator('[data-query-workbench-appearance="surface"] [data-query-tab-selection="underline"]');
  await expect(tabs.getByRole('tab', { name: /^Traces/ })).toHaveText(/^Traces\s*2$/);
  // The spans are listed when their tab is opened, and counted then.
  await expect(tabs.getByRole('tab', { name: /^Spans/ })).toHaveText(/^Spans$/);
  await tabs.getByRole('tab', { name: /^Spans/ }).click();
  await expect(tabs.getByRole('tab', { name: /^Spans/ })).toHaveText(/^Spans\s*3$/);
  await expect(page.locator('[data-span-list-row]')).toHaveCount(3);
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

test('stands the field list and the results on one surface, under the workbench bar', async ({ page }) => {
  await page.goto('/traces');

  const shell = page.locator('[data-shell-layout="surface-workbench"]');
  const topbar = page.getByRole('banner');
  const sidebar = page.getByTestId('primary-sidebar');
  const canvas = page.locator('[data-workspace="traces"]');
  const querySurface = page.locator('[data-query-workbench-appearance="surface"]');
  // The explorer has no title row: its name sits at the start of the workbench bar.
  const barTitle = querySurface.locator('[data-workbench-title]');
  const workspace = page.locator('[data-trace-workspace-layout="joined"]');
  const fieldPanel = page.locator('aside[data-variant="utility"]');
  const results = page.locator('[data-workspace-pane="trace-results"]');
  const pagination = results.getByRole('navigation');
  const primaryTabs = querySurface.locator('[data-query-tab-selection="underline"]');
  const activeTab = primaryTabs.getByRole('tab', { name: /^Traces/ });
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
  // One surface: the list set off from the results by its own line, with no gutter between.
  const [workspaceSize, panelSize, resultsSize] = await Promise.all([boundingBox(workspace), boundingBox(fieldPanel), boundingBox(results)]);
  expect(panelSize.width).toBe(260);
  expect(Math.abs(panelSize.x + panelSize.width - resultsSize.x)).toBeLessThanOrEqual(0.5);
  expect(Math.abs(resultsSize.x + resultsSize.width - (workspaceSize.x + workspaceSize.width))).toBeLessThanOrEqual(0.5);
  await expect(fieldPanel).toHaveCSS('border-right-width', '1px');

  const [canvasBox, queryBox, workspaceBox] = await Promise.all([
    boundingBox(canvas),
    boundingBox(querySurface),
    boundingBox(workspace),
  ]);
  // The query tabs are in the query box: the bar is the first card under the top bar,
  // 12px off it, as a 12px gutter sets it off from the panes below.
  const barTopGap = queryBox.y - canvasBox.y;
  const barBottomGap = workspaceBox.y - queryBox.y - queryBox.height;
  expect(barTopGap).toBe(12);
  expect(barBottomGap).toBe(12);

  const surfaceBackgrounds = await Promise.all(
    [querySurface, fieldPanel, workspace].map((locator) =>
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
