import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

interface QueryRange {
  start: number;
  end: number;
}

async function selectMetric(page: Page, name = 'http_requests_total') {
  await page.getByTestId('metric-search-trigger').click();
  const browser = page.getByTestId('metrics-browser-dialog');
  await expect(browser).toBeVisible();
  await browser.getByText(name, { exact: true }).click();
  await expect(browser).toBeHidden();
}

test.describe('Metrics time-range zoom', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('query editor is expanded by default and can be collapsed', async ({ page }) => {
    await page.goto('/metrics');

    await expect(page.locator('[data-query-editor-state="expanded"]')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Collapse query' })).toBeVisible();

    await page.getByRole('button', { name: 'Collapse query' }).click();
    const collapsedEditor = page.locator('[data-query-editor-state="collapsed"]');
    await expect(collapsedEditor).toBeVisible();
    await expect(collapsedEditor).toContainText('Choose a metric or enter PromQL');

    await collapsedEditor.click();
    await expect(page.locator('[data-query-editor-state="expanded"]')).toBeVisible();
  });

  test('expands the graph to use space released by the collapsed query editor', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1_280, height: 1_000 });
    await page.goto('/metrics');
    await selectMetric(page);

    const chart = page.getByTestId('time-series-chart');
    await expect(chart).toBeVisible({ timeout: 10_000 });
    const expandedBox = await chart.boundingBox();
    expect(expandedBox).not.toBeNull();
    if (!expandedBox) return;

    await page.getByRole('button', { name: 'Collapse query' }).click();
    await expect(page.locator('[data-query-editor-state="collapsed"]')).toBeVisible();
    await expect
      .poll(async () => (await chart.boundingBox())?.height ?? 0)
      .toBeGreaterThan(expandedBox.height + 80);
  });

  test('opens the metric browser from one search trigger and keeps results full width', async ({ page }) => {
    await page.goto('/metrics');

    const workspace = page.getByTestId('metrics-workspace');
    const queryCard = page.getByTestId('metrics-query-card');
    const searchTrigger = page.getByTestId('metric-search-trigger');
    const emptyQueryState = page.getByRole('status').filter({ hasText: 'PromQL' });

    await expect(workspace).toBeVisible();
    await expect(queryCard).toBeVisible();
    await expect(searchTrigger).toBeVisible();
    await expect(page.getByTestId('metrics-browser-dialog')).toHaveCount(0);
    await expect(emptyQueryState).toBeVisible();

    const [workspaceBox, queryCardBox, searchTriggerBox, emptyQueryBox] = await Promise.all([
        workspace.boundingBox(),
        queryCard.boundingBox(),
        searchTrigger.boundingBox(),
        emptyQueryState.boundingBox(),
    ]);
    expect(workspaceBox).not.toBeNull();
    expect(queryCardBox).not.toBeNull();
    expect(searchTriggerBox).not.toBeNull();
    expect(emptyQueryBox).not.toBeNull();
    if (!workspaceBox || !queryCardBox || !searchTriggerBox || !emptyQueryBox) return;
    expect(searchTriggerBox.y).toBeGreaterThan(queryCardBox.y);
    expect(searchTriggerBox.x).toBeGreaterThanOrEqual(queryCardBox.x);
    expect(searchTriggerBox.width).toBeGreaterThan(queryCardBox.width * 0.9);
    expect(workspaceBox.y).toBeGreaterThanOrEqual(
      queryCardBox.y + queryCardBox.height,
    );
    expect(emptyQueryBox.height).toBeGreaterThanOrEqual(300);
    expect(emptyQueryBox.width).toBeGreaterThan(workspaceBox.width * 0.8);

    await selectMetric(page);
    await expect(page.getByRole('img', { name: /Metrics time series/i })).toBeVisible({
      timeout: 10_000,
    });
  });

  test('switches Code and Builder inline and composes PromQL from dropdowns', async ({
    page,
  }) => {
    await page.goto('/metrics');

    const queryCard = page.getByTestId('metrics-query-card');
    const codeButton = queryCard.getByRole('button', { name: 'Code' });
    const builderButton = queryCard.getByRole('button', { name: 'Builder' });
    await expect(
      queryCard.getByText('Prometheus', { exact: true }),
    ).toHaveCount(0);
    await expect(codeButton).toHaveAttribute('aria-pressed', 'true');

    await builderButton.click();
    await expect(page).toHaveURL(/\/metrics(?:\?|$)/);
    await expect(builderButton).toHaveAttribute('aria-pressed', 'true');
    await expect(page.getByTestId('promql-builder-panel')).toBeVisible();
    await expect(page.getByTestId('metric-search-trigger')).toHaveCount(0);

    const metricSelect = page.getByRole('combobox', { name: 'Metric name' });
    await expect(metricSelect).toBeEnabled();
    await metricSelect.click();
    await page
      .getByRole('option', { name: 'http_requests_total', exact: true })
      .click();
    await expect(page.getByTestId('promql-builder-preview')).toHaveText(
      'rate(http_requests_total[5m])',
    );

    const functionSelect = page.getByRole('combobox', {
      name: 'PromQL function',
    });
    await functionSelect.click();
    await expect(
      page.getByRole('option', { name: /avg_over_time/i }),
    ).toBeEnabled();
    await expect(
      page.getByRole('option', { name: /histogram_quantile/i }),
    ).toBeDisabled();
    await page.getByRole('option', { name: /avg_over_time/i }).click();
    await expect(page.getByTestId('promql-builder-preview')).toHaveText(
      'avg_over_time(http_requests_total[5m])',
    );

    await functionSelect.click();
    await page.getByRole('option', { name: /^Rate/ }).click();

    await page.getByRole('combobox', { name: 'PromQL aggregation' }).click();
    await page.getByRole('option', { name: 'Sum', exact: true }).click();
    await expect(page.getByTestId('promql-builder-preview')).toHaveText(
      'sum(rate(http_requests_total[5m]))',
    );

    await codeButton.click();
    await expect(page.getByTestId('promql-builder-panel')).toHaveCount(0);
    await expect(page.getByTestId('metric-search-trigger')).toBeVisible();
    await expect(queryCard).toContainText('sum(rate(http_requests_total[5m]))');
  });

  test('edits query options and applies them to results and query execution', async ({
    page,
  }) => {
    const requests: Array<{
      limit?: number;
      time_range?: QueryRange;
    }> = [];
    page.on('request', (request) => {
      if (
        request.method() === 'POST' &&
        new URL(request.url()).pathname === '/api/v1/query'
      ) {
        requests.push(request.postDataJSON());
      }
    });

    await page.goto('/metrics');
    await selectMetric(page);
    await expect(page.getByRole('img', { name: /Metrics time series/i })).toBeVisible({
      timeout: 10_000,
    });

    await page.getByTestId('metrics-options-toggle').click();
    const options = page.getByTestId('metrics-query-options');
    await expect(options).toBeVisible();
    const legendOption = page.getByTestId('metrics-option-legend');
    await legendOption.getByRole('combobox', { name: 'Legend mode' }).click();
    await page.getByRole('option', { name: /Custom/ }).click();
    await legendOption
      .getByRole('textbox', { name: 'Custom legend' })
      .fill('{{status}}');
    await expect(
      page
        .getByTestId('time-series-legend')
        .getByRole('button', { name: '500', exact: true }),
    ).toBeVisible();

    await page.getByTestId('metrics-option-format').click();
    await page.getByRole('option', { name: 'Table', exact: true }).click();
    await expect(page.getByTestId('metrics-result-table')).toBeVisible();

    await page.getByTestId('metrics-option-step').fill('5m');
    await page.getByTestId('metrics-option-type').click();
    await page.getByRole('option', { name: 'Instant', exact: true }).click();
    await page.getByTestId('metrics-option-exemplars').click();
    await page.getByRole('option', { name: 'Off', exact: true }).click();

    const toolbar = page.getByTestId('metrics-explore-toolbar');
    const runButton = toolbar.getByRole('button', { name: 'Run', exact: true });
    const beforeInstant = requests.length;
    await runButton.click();
    await expect.poll(() => requests.length).toBeGreaterThan(beforeInstant);
    expect(requests.at(-1)?.limit).toBe(1);

    await page.getByTestId('metrics-option-type').click();
    await page.getByRole('option', { name: 'Range', exact: true }).click();
    const beforeRange = requests.length;
    await runButton.click();
    await expect.poll(() => requests.length).toBeGreaterThan(beforeRange);
    const latest = requests.at(-1)!;
    const spanMilliseconds =
      (latest.time_range!.end - latest.time_range!.start) / 1_000;
    expect(latest.limit).toBe(
      Math.min(1_000, Math.max(2, Math.ceil(spanMilliseconds / 300_000))),
    );
  });

  test('switches one query result between graph, table, and inspector views', async ({
    page,
  }) => {
    await page.goto('/metrics');
    await selectMetric(page);

    await expect(page.getByRole('img', { name: /Metrics time series/i })).toBeVisible({
      timeout: 10_000,
    });

    const legend = page.getByTestId('time-series-legend');
    await expect(legend).toHaveAttribute('data-legend-mode', 'table');
    await expect(legend).toHaveAttribute('data-legend-placement', 'bottom');
    const legendTable = page.getByTestId('time-series-legend-table');
    for (const header of ['Name', 'Last', 'Min', 'Max', 'Mean']) {
      await expect(
        legendTable.getByRole('columnheader', { name: header }),
      ).toBeVisible();
    }
    await expect(
      legendTable.getByRole('button', {
        name: 'http_requests_total{status="500"}',
        exact: true,
      }),
    ).toBeVisible();
    await expect(page.getByTestId('metrics-legend-name')).toHaveCount(0);

    const stackMode = page.getByRole('combobox', { name: 'Stack' });
    await expect(stackMode).toBeEnabled();
    await stackMode.click();
    await page.getByRole('option', { name: 'Percent', exact: true }).click();
    await expect(stackMode).toContainText('Percent');
    await expect(page.getByTestId('time-series-chart')).toHaveAttribute(
      'data-stack-mode',
      'percent',
    );

    await page.getByRole('tab', { name: 'Table' }).click();
    const table = page.getByTestId('metrics-result-table');
    await expect(table).toBeVisible();
    await expect(table.locator('tbody tr')).not.toHaveCount(0);

    await page.getByRole('tab', { name: 'Query inspector' }).click();
    await expect(
      page
        .getByTestId('metrics-workspace')
        .getByText('rate(http_requests_total[5m])', { exact: true }),
    ).toBeVisible();
    await expect(page.getByText('Rows scanned', { exact: true })).toBeVisible();
  });

  test('wraps long legend names and keeps the series marker vertically centered', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1_280, height: 900 });
    await page.route('**/api/v1/query', async (route) => {
      const body = route.request().postDataJSON() as {
        language?: string;
        time_range?: QueryRange;
      };
      if (body.language !== 'promql') {
        await route.fallback();
        return;
      }
      const start = body.time_range?.start ?? 1;
      const end = body.time_range?.end ?? 2;
      const instanceId = `instance-${'0123456789abcdef'.repeat(12)}`;
      await route.fulfill({
        json: {
          columns: ['_timestamp', 'value', 'service.instance.id'],
          rows: [
            [start, 1, instanceId],
            [end, 2, instanceId],
          ],
          scanned_rows: 2,
          took_ms: 1,
        },
      });
    });

    await page.goto('/metrics');
    await selectMetric(page);

    const legendLabel = page
      .getByTestId('time-series-legend-table')
      .getByRole('button', { name: /^http_requests_total\{/ });
    await expect(legendLabel).toBeVisible({ timeout: 10_000 });
    await expect(legendLabel).toHaveCSS('white-space', 'normal');
    await expect(page.getByRole('combobox', { name: 'Stack' })).toBeDisabled();
    await expect(page.getByTestId('metrics-stack-mode')).toContainText('Off');

    const row = legendLabel.locator('xpath=ancestor::tr');
    const marker = row.locator('td').first().locator('span[aria-hidden="true"]');
    const [labelBox, rowBox, markerBox] = await Promise.all([
      legendLabel.boundingBox(),
      row.boundingBox(),
      marker.boundingBox(),
    ]);
    expect(labelBox).not.toBeNull();
    expect(rowBox).not.toBeNull();
    expect(markerBox).not.toBeNull();
    if (!labelBox || !rowBox || !markerBox) return;

    expect(labelBox.height).toBeGreaterThan(30);
    const rowCenter = rowBox.y + rowBox.height / 2;
    const markerCenter = markerBox.y + markerBox.height / 2;
    expect(Math.abs(rowCenter - markerCenter)).toBeLessThanOrEqual(1);
  });

  test('plots an all-zero rate on the bottom zero baseline', async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 1_200 });
    await page.route('**/api/v1/query', async (route) => {
      const body = route.request().postDataJSON() as {
        language?: string;
        time_range?: QueryRange;
      };
      if (body.language !== 'promql') {
        await route.fallback();
        return;
      }
      const start = body.time_range?.start ?? 1;
      const end = body.time_range?.end ?? 3;
      await route.fulfill({
        json: {
          columns: ['_timestamp', 'value', 'status'],
          rows: [
            [start, 0, '200'],
            [start + (end - start) / 2, 0, '200'],
            [end, 0, '200'],
          ],
          scanned_rows: 3,
          took_ms: 2,
        },
      });
    });

    await page.goto('/metrics');
    await selectMetric(page);

    const chart = page.getByRole('img', { name: /Metrics time series/i });
    await expect(chart).toBeVisible({ timeout: 10_000 });
    const overlay = chart.locator('.u-over');
    await overlay.scrollIntoViewIfNeeded();
    const overlayBox = await overlay.boundingBox();
    expect(overlayBox).not.toBeNull();
    if (!overlayBox) return;

    await page.mouse.move(
      overlayBox.x + overlayBox.width / 2,
      overlayBox.y + overlayBox.height - 4,
    );
    const cursorPoint = chart.locator('.u-cursor-pt').first();
    await expect(cursorPoint).toBeVisible();
    const pointBox = await cursorPoint.boundingBox();
    expect(pointBox).not.toBeNull();
    if (!pointBox) return;

    const pointCenter = pointBox.y + pointBox.height / 2;
    expect(pointCenter).toBeGreaterThan(
      overlayBox.y + overlayBox.height * 0.85,
    );
    await expect(page.getByRole('tooltip')).toContainText('0 req/s');
  });

  test('dragging the chart narrows the query range and reset restores it', async ({ page }) => {
    const requestedRanges: QueryRange[] = [];
    page.on('request', (request) => {
      if (
        request.method() !== 'POST' ||
        new URL(request.url()).pathname !== '/api/v1/query'
      ) {
        return;
      }
      const body = request.postDataJSON() as {
        language?: string;
        time_range?: QueryRange;
      };
      if (body.language === 'promql' && body.time_range) {
        requestedRanges.push(body.time_range);
      }
    });

    await page.goto('/metrics');
    await selectMetric(page);

    const chart = page.getByRole('img', { name: /Metrics time series/i });
    await expect(chart).toBeVisible({ timeout: 10_000 });
    await expect.poll(() => requestedRanges.length).toBeGreaterThanOrEqual(1);

    const plotOverlay = chart.locator('.u-over');
    await plotOverlay.scrollIntoViewIfNeeded();
    const bounds = await plotOverlay.boundingBox();
    expect(bounds).not.toBeNull();
    if (!bounds) return;

    const y = bounds.y + bounds.height * 0.5;
    await page.mouse.move(bounds.x + bounds.width * 0.72, y);
    await page.mouse.down();
    await page.mouse.move(bounds.x + bounds.width * 0.28, y, { steps: 8 });
    await expect(chart.getByTestId('chart-range-selection')).toBeVisible();
    await page.mouse.up();

    await expect(page.getByTestId('metrics-reset-zoom')).toBeVisible();
    await expect.poll(() => requestedRanges.length).toBeGreaterThanOrEqual(2);

    const initialRange = requestedRanges[0]!;
    const zoomedRange = requestedRanges.at(-1)!;
    expect(zoomedRange.end - zoomedRange.start).toBeLessThan(
      (initialRange.end - initialRange.start) * 0.6,
    );

    await page.getByTestId('metrics-reset-zoom').click();
    await expect(page.getByTestId('metrics-reset-zoom')).toHaveCount(0);
    await expect.poll(() => requestedRanges.length).toBeGreaterThanOrEqual(3);

    const restoredRange = requestedRanges.at(-1)!;
    expect(restoredRange.end - restoredRange.start).toBe(initialRange.end - initialRange.start);
  });

  test('double-click doubles the current range instead of resetting to the origin', async ({ page }) => {
    const requestedRanges: QueryRange[] = [];
    page.on('request', (request) => {
      if (
        request.method() !== 'POST' ||
        new URL(request.url()).pathname !== '/api/v1/query'
      ) {
        return;
      }
      const body = request.postDataJSON() as {
        language?: string;
        time_range?: QueryRange;
      };
      if (body.language === 'promql' && body.time_range) {
        requestedRanges.push(body.time_range);
      }
    });

    await page.goto('/metrics');
    await selectMetric(page);

    const chart = page.getByRole('img', { name: /Metrics time series/i });
    await expect(chart).toBeVisible({ timeout: 10_000 });
    await expect.poll(() => requestedRanges.length).toBeGreaterThanOrEqual(1);
    const plotOverlay = chart.locator('.u-over');
    await plotOverlay.scrollIntoViewIfNeeded();
    const bounds = await plotOverlay.boundingBox();
    expect(bounds).not.toBeNull();
    if (!bounds) return;

    const initialRange = requestedRanges.at(-1)!;
    const y = bounds.y + bounds.height * 0.45;
    await page.mouse.move(bounds.x + bounds.width * 0.7, y);
    await page.mouse.down();
    await page.mouse.move(bounds.x + bounds.width * 0.4, y, { steps: 6 });
    await page.mouse.up();
    await expect
      .poll(() => requestedRanges.length)
      .toBeGreaterThan(1);

    const zoomedRange = requestedRanges.at(-1)!;
    expect(zoomedRange.end - zoomedRange.start).toBeLessThan(
      (initialRange.end - initialRange.start) * 0.5,
    );

    const requestsBeforeDoubleClick = requestedRanges.length;
    await plotOverlay.dblclick({
      position: {
        x: bounds.width * 0.5,
        y: bounds.height * 0.45,
      },
    });
    await expect
      .poll(() => requestedRanges.length)
      .toBeGreaterThan(requestsBeforeDoubleClick);

    const expandedRange = requestedRanges.at(-1)!;
    expect(
      Math.abs(
        (expandedRange.end - expandedRange.start) -
          (zoomedRange.end - zoomedRange.start) * 2,
      ),
    ).toBeLessThanOrEqual(
      2_000,
    );
    expect(expandedRange.end - expandedRange.start).toBeLessThan(
      initialRange.end - initialRange.start,
    );
  });

  test('x-axis drag previews the pan before committing the query range', async ({ page }) => {
    const requestedRanges: QueryRange[] = [];
    page.on('request', (request) => {
      if (
        request.method() !== 'POST' ||
        new URL(request.url()).pathname !== '/api/v1/query'
      ) {
        return;
      }
      const body = request.postDataJSON() as {
        language?: string;
        time_range?: QueryRange;
      };
      if (body.language === 'promql' && body.time_range) {
        requestedRanges.push(body.time_range);
      }
    });

    await page.goto('/metrics');
    await selectMetric(page);

    const chart = page.getByRole('img', { name: /Metrics time series/i });
    await expect(chart).toBeVisible({ timeout: 10_000 });
    await expect.poll(() => requestedRanges.length).toBeGreaterThanOrEqual(1);

    const xAxis = chart.locator('.u-axis[data-range-pan="horizontal-drag"]');
    await expect(xAxis).toBeVisible();
    await xAxis.scrollIntoViewIfNeeded();
    const axisBounds = await xAxis.boundingBox();
    expect(axisBounds).not.toBeNull();
    if (!axisBounds) return;

    const plotOverlay = chart.locator('.u-over');
    const visibleFromBefore = Number(
      await plotOverlay.getAttribute('data-visible-from'),
    );
    const requestsBeforePan = requestedRanges.length;
    const initialRange = requestedRanges.at(-1)!;
    const startX = axisBounds.x + axisBounds.width * 0.6;
    const y = axisBounds.y + axisBounds.height * 0.5;

    await page.mouse.move(startX, y);
    await page.mouse.down();
    await page.mouse.move(startX - axisBounds.width * 0.15, y, { steps: 8 });

    await expect
      .poll(async () =>
        Number(await plotOverlay.getAttribute('data-visible-from')),
      )
      .toBeGreaterThan(visibleFromBefore);
    const previewFrom = Number(
      await plotOverlay.getAttribute('data-visible-from'),
    );
    expect(requestedRanges).toHaveLength(requestsBeforePan);

    await page.mouse.up();
    await expect
      .poll(() => requestedRanges.length)
      .toBeGreaterThan(requestsBeforePan);

    const pannedRange = requestedRanges.at(-1)!;
    expect(pannedRange.start).toBeGreaterThan(initialRange.start);
    await expect
      .poll(async () =>
        Number(await plotOverlay.getAttribute('data-visible-from')),
      )
      .toBeCloseTo(previewFrom, 2);
    expect(
      Math.abs(
        (pannedRange.end - pannedRange.start) -
          (initialRange.end - initialRange.start),
      ),
    ).toBeLessThanOrEqual(2_000);
  });
});
