import { explorerRead, explorerReadBody } from '../fixtures/explorerReads';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

interface QueryRange {
  start: number;
  end: number;
}

test('lays the same query over the stretch before on the graph, dashed, and over another baseline when chosen', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.setViewportSize({ width: 1_280, height: 900 });
  const ranges: QueryRange[] = [];

  await page.route(explorerRead('query'), async (route) => {
    const body = explorerReadBody(route.request()) as { language?: string; timeRange?: QueryRange };
    if (body.language !== 'promql' || !body.timeRange) {
      await route.fallback();
      return;
    }

    ranges.push(body.timeRange);
    const { start, end } = body.timeRange;
    await route.fulfill({
      json: {
        columns: ['_timestamp', 'value', 'service.name'],
        rows: [
          [start, 1, 'checkout'],
          [Math.round((start + end) / 2), 3, 'checkout'],
          [end, 2, 'checkout'],
        ],
        scannedRows: 3,
        tookMs: 1,
      },
    });
  });

  await page.goto(`/metrics?promql=${encodeURIComponent('sum by (service.name) (rate(http_requests_total[5m]))')}`);
  await expect(page.getByTestId('time-series-chart')).toBeVisible({ timeout: 10_000 });
  const legend = page.getByTestId('time-series-legend-table');
  await expect(legend).not.toContainText('(baseline)');

  await page.locator('[data-metrics-baseline-on]').click();
  await expect(page.locator('[data-metrics-baseline="previous"]')).toBeVisible();

  // The stretch of the same length just before the range looked at. The graph's range is resolved
  // when it is drawn, the query's when it runs: they agree to within moments.
  const SKEW = 5_000_000;
  await expect.poll(() => ranges.length).toBeGreaterThanOrEqual(2);
  const [looked, before] = [ranges[0]!, ranges.at(-1)!];
  expect(Math.abs(before.end - looked.start)).toBeLessThan(SKEW);
  expect(before.end - before.start).toBe(looked.end - looked.start);
  await expect(legend).toContainText('(baseline)');
  // Stacking a baseline on the series it compares with would say nothing.
  await expect(page.getByRole('combobox', { name: 'Stack' })).toBeDisabled();

  // The same stretch a week earlier.
  await page.locator('[data-compare-baseline]').click();
  await page.locator('[data-compare-baseline-option="week"]').click();
  await expect
    .poll(() => Math.abs(looked.start - ranges.at(-1)!.start - 7 * 24 * 3_600_000_000) < SKEW)
    .toBe(true);

  // Off again: the series alone.
  await page.getByRole('button', { name: 'Stop comparing' }).click();
  await expect(legend).not.toContainText('(baseline)');
});
