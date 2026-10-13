import type { Page } from '@playwright/test';

import {
  numberFieldStats,
  serveLogFieldStats,
  serveTraceFieldStats,
  textFieldStats,
} from '../fixtures/fieldStats';
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { expectExplorerResults, serveExplorerResults } from '../fixtures/mockExplorers';

const STATUS = { total: 12_840, present: 5_120, top: [['200', 2_600], ['201', 1_500], ['504', 1_020]] } as const;
const LEVELS = { total: 12_840, present: 12_840, top: [['INFO', 9_000], ['ERROR', 2_400], ['WARN', 1_440]] } as const;
const held = <T extends { top: ReadonlyArray<readonly [string, number]> }>(value: T) => ({
  ...value,
  top: value.top.map(([name, count]) => [name, count] as [string, number]),
});

/** Opens the analysis of a field from its row in the Logs field list. */
async function analyse(page: Page, field: string) {
  const row = page.locator(`[data-log-field-row="${field}"]`);

  // What can be asked of a field shows with the pointer on its row.
  await row.hover();
  await row.locator('[data-field-analysis-trigger]').click();

  const panel = page.locator(`[data-field-analysis="${field}"]`);
  await expect(panel).toBeVisible();
  await expect(panel).not.toHaveAttribute('data-field-analysis-scope', 'pending');

  return panel;
}

const figure = (panel: ReturnType<Page['locator']>, label: string) =>
  panel.locator('dt', { hasText: label }).locator('xpath=following-sibling::dd[1]');

test.describe('Field analysis in the Logs explorer', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await mountMockRoutes(page, mockServer.port);
    await serveExplorerResults(page);
  });

  test('a field is analysed across every log the query matches', async ({ page }) => {
    const asked = await serveLogFieldStats(page, (ask) =>
      ask.field === 'http.status_code' ? numberFieldStats(ask, held(STATUS)) : textFieldStats(ask, held(LEVELS)),
    );
    await page.goto('/logs');
    await expectExplorerResults(page, 'logs');

    const panel = await analyse(page, 'http.status_code');

    // It is asked with what the list asked: the stream, the range, the conditions, and the field.
    expect(asked).toHaveLength(1);
    expect(asked[0]).toMatchObject({ stream: 'application_logs', filters: [], free_text: [], field: 'http.status_code' });
    expect(asked[0]!.to).toBeGreaterThan(asked[0]!.from);

    // The numbers are of all 12,840 that match, not of the hundred on the page.
    await expect(panel).toHaveAttribute('data-field-analysis-scope', 'all');
    await expect(panel.locator('[data-field-analysis-scope-line]')).toHaveText('Of every log the query matches');
    await expect(figure(panel, 'Logs')).toHaveText('12,840');
    await expect(figure(panel, 'With a value')).toHaveText('5,120');
    await expect(figure(panel, 'Distinct values')).toHaveText('3');
    await expect(figure(panel, 'Missing')).toHaveText(/^7,720 \(60\.1\s?%\)$/);
    await expect(figure(panel, 'Max')).toHaveText('504');
    await expect(figure(panel, 'Min')).toHaveText('200');
    await expect(figure(panel, 'Skewness')).toHaveText('1.52');

    // A number is drawn bar by bar, and the bar under the pointer is read above the plot.
    const plot = panel.locator('[data-field-analysis-plot]');
    await expect(plot.locator('[data-field-analysis-bar]')).toHaveCount(2);
    const box = (await plot.boundingBox())!;
    await page.mouse.move(box.x + 4, box.y + box.height / 2);
    await expect(panel.locator('[data-field-analysis-bar-range]')).toHaveText('200 – 220');
    await expect(panel.locator('[data-field-analysis-bars]')).toContainText('4,100 values');

    // Its commonest values, each with how many logs hold it.
    const values = panel.locator('[data-field-analysis-block="top"] [data-field-value]');
    await expect(values).toHaveText(['2002,600', '2011,500', '5041,020']);
  });

  test('the commonest values are followed over time, and the list under the chart is its legend', async ({ page }) => {
    await serveLogFieldStats(page, (ask) => textFieldStats(ask, held(LEVELS)));
    await page.goto('/logs');
    await expectExplorerResults(page, 'logs');

    const panel = await analyse(page, 'level');
    const trend = panel.locator('[data-field-analysis-block="trend"]');

    await expect(trend.getByRole('heading')).toHaveText('Top 3 values over time');
    await expect(trend.locator('canvas').first()).toBeVisible();
    // Text has no statistics and no bars: it is counted, followed and ranked.
    await expect(panel.locator('[data-field-analysis-block="statistics"]')).toHaveCount(0);
    await expect(panel.locator('[data-field-analysis-block="distribution"]')).toHaveCount(0);

    // Each of the values that are followed has the colour of its line; the three differ.
    const swatches = panel.locator('[data-field-analysis-block="top"] [data-field-value-swatch]');
    await expect(swatches).toHaveCount(3);
    const colours = await swatches.evaluateAll((items) => items.map((item) => getComputedStyle(item).backgroundColor));
    expect(new Set(colours).size).toBe(3);
  });

  test('the panel floats over the results, as tall as what it holds', async ({ page }) => {
    await serveLogFieldStats(page, (ask) => textFieldStats(ask, held(LEVELS)));
    await page.goto('/logs');
    await expectExplorerResults(page, 'logs');
    const results = page.locator('[data-workspace-pane="log-results"]');
    const before = (await results.boundingBox())!;

    const panel = await analyse(page, 'level');
    const [list, floating, after] = await Promise.all([
      page.locator('aside[data-variant="utility"]').boundingBox(),
      panel.boundingBox(),
      results.boundingBox(),
    ]);

    // It takes no room from the results: they are where they were, and it lies over their start.
    expect(after).toEqual(before);
    expect(floating!.x).toBeGreaterThanOrEqual(list!.x + list!.width);
    expect(floating!.x).toBeLessThan(after!.x + 16);
    expect(floating!.x + floating!.width).toBeLessThan(after!.x + after!.width);
    // No taller than the list it was opened from, and no empty stretch under what it holds.
    expect(floating!.y).toBeCloseTo(list!.y, 0);
    expect(floating!.height).toBeLessThanOrEqual(list!.height);
    const ground = (await panel.locator('[data-field-analysis-block]').last().boundingBox())!;
    expect(floating!.y + floating!.height - (ground.y + ground.height)).toBeLessThan(24);
  });

  test('falls back to the loaded logs, and says so, when every match cannot be read', async ({ page }) => {
    await serveLogFieldStats(page, () => ({ kind: 'text', total: 0, unavailable: 'time_budget' }));
    await page.goto('/logs');
    await expectExplorerResults(page, 'logs');

    const panel = await analyse(page, 'level');

    await expect(panel).toHaveAttribute('data-field-analysis-scope', 'loaded');
    await expect(panel.locator('[data-field-analysis-scope-line]')).toContainText('Of the 100 loaded logs');
    await expect(panel.locator('[data-field-analysis-scope-line]')).toContainText('Counting every match timed out');
    await expect(figure(panel, 'Logs')).toHaveText('100');
    await expect(panel.getByRole('button', { name: 'Retry' })).toBeVisible();
  });

  test('a masked field is not analysed, in everything or in what is loaded', async ({ page }) => {
    const asked = await serveLogFieldStats(page, (ask) => textFieldStats(ask, held(LEVELS)));
    await page.route('**/api/v1/field_masking/effective/**', (route) =>
      route.fulfill({ json: { fields: [{ field: 'service.name', masked: true }] } }),
    );
    await page.goto('/logs');
    await expectExplorerResults(page, 'logs');

    const panel = await analyse(page, 'service.name');

    await expect(panel).toContainText('This field is masked or encrypted and cannot be analyzed.');
    await expect(panel.locator('[data-field-analysis-block]')).toHaveCount(0);
    expect(asked).toHaveLength(0);
  });

  test('the panel closes on Escape and gives the keyboard back to the button it was opened from', async ({ page }) => {
    await serveLogFieldStats(page, (ask) => textFieldStats(ask, held(LEVELS)));
    await page.goto('/logs');
    await expectExplorerResults(page, 'logs');

    const panel = await analyse(page, 'level');
    const trigger = page.locator('[data-log-field-row="level"] [data-field-analysis-trigger]');

    await expect(panel).toBeFocused();
    await expect(trigger).toHaveAttribute('aria-pressed', 'true');
    await page.keyboard.press('Escape');
    await expect(panel).toHaveCount(0);
    await expect(trigger).toBeFocused();
  });

  test('offers no analysis while there are no results', async ({ page }) => {
    await page.route('**/api/v1/web/logs', (route) =>
      route.fulfill({ json: { items: [], next_cursor: null, previous_cursor: null, has_more: false } }),
    );
    await page.goto('/logs');

    const row = page.locator('[data-log-field-row="level"]');
    await expect(row).toBeVisible();
    await row.hover();
    await expect(row.locator('[data-field-analysis-trigger]')).toHaveCount(0);
  });
});

test.describe('Field analysis in the Traces explorer', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await mountMockRoutes(page, mockServer.port);
    await serveExplorerResults(page);
  });

  test('a field of a span is read in every span of the traces on the page', async ({ page }) => {
    const asked = await serveTraceFieldStats(page, (ask) => numberFieldStats(ask, { total: 640, present: 212, top: [['200', 170], ['404', 12], ['503', 30]] }));
    await page.goto('/traces');
    await expectExplorerResults(page, 'traces');

    await page.locator('[data-field-group="http"] > button').click();
    const row = page.locator('[data-trace-field="http.response.status_code"]');
    await row.locator('[data-trace-field-row]').hover();
    await row.locator('[data-field-analysis-trigger]').click();
    const panel = page.locator('[data-field-analysis="http.response.status_code"]');

    await expect(panel).toHaveAttribute('data-field-analysis-scope', 'all');
    await expect(panel.locator('[data-field-analysis-scope-line]')).toHaveText('Of every span of the 100 loaded traces');
    await expect(figure(panel, 'Spans')).toHaveText('640');
    await expect(figure(panel, 'Missing')).toHaveText(/^428 \(66\.9\s?%\)$/);
    // The traces are named one by one: the page of the list, and the stream it was listed from.
    expect(asked).toHaveLength(1);
    expect(asked[0]).toMatchObject({ stream: 'default', field: 'http.response.status_code' });
    expect(asked[0]!.trace_ids).toHaveLength(100);
  });

  test('a field a trace has one value for is analysed on the rows of the list', async ({ page }) => {
    const asked = await serveTraceFieldStats(page, () => ({ status: 500 }));
    await page.goto('/traces');
    await expectExplorerResults(page, 'traces');

    const row = page.locator('[data-trace-field="duration_ns"]');
    await row.locator('[data-trace-field-row]').hover();
    await row.locator('[data-field-analysis-trigger]').click();
    const panel = page.locator('[data-field-analysis="duration_ns"]');

    await expect(panel).toHaveAttribute('data-field-analysis-scope', 'loaded');
    await expect(panel.locator('[data-field-analysis-scope-line]')).toHaveText('Of the 100 loaded traces');
    await expect(figure(panel, 'Traces')).toHaveText('100');
    // A duration is read in its natural unit, as in the values under the row.
    await expect(figure(panel, 'Max')).toHaveText('2450 ms');
    await expect(panel.locator('[data-field-analysis-plot]')).toBeVisible();
    // Nothing is asked: the rows are in hand.
    expect(asked).toHaveLength(0);
  });
});
