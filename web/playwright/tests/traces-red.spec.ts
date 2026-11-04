/**
 * The RED drilldown of the Traces explorer: rate, errors and duration of the
 * requests the query matches, broken down by a field, every step of the drill
 * a step of the query itself.
 */
import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { serveExplorerResults } from '../fixtures/mockExplorers';
import { type AskedRed, serveTraceRed } from '../fixtures/mockTraceRed';

/** Room for the page to load and count: a load from the dev server takes several seconds while a suite runs. */
const LOADED = { timeout: 15_000 };

async function openRed(page: Page, port: number): Promise<AskedRed> {
  await mountMockRoutes(page, port);
  await serveExplorerResults(page);
  const asked = await serveTraceRed(page);
  await page.goto('/traces?tab=red');
  await expect(page.locator('[data-red-signal]')).toHaveCount(3, LOADED);
  await expect(page.locator('[data-red-group]').first()).toBeVisible(LOADED);

  return asked;
}

const filtersOf = (params: URLSearchParams | undefined) =>
  JSON.parse(params?.get('filters') ?? '[]') as Array<{ field: string; op: string; value: string }>;

test.describe('Traces RED drilldown', () => {
  test('opens on the services, the busiest first, with the three figures over the range', async ({
    page,
    mockServer,
  }) => {
    const asked = await openRed(page, mockServer.port);

    await expect(page.locator('[data-red-dimension]')).toHaveText('service.name');
    // Each trace of the day is one request of its root service and one of inventory.
    await expect(page.locator('[data-red-group]').first()).toHaveAttribute('data-red-group', 'inventory');
    await expect(page.locator('[data-red-signal="rate"] [data-red-figure]')).not.toHaveText('—');
    expect(asked.overview.at(-1)?.get('scope')).toBe('entry');
    expect(Number(asked.overview.at(-1)?.get('bucketTime'))).toBeGreaterThan(0);
  });

  test('a value narrows the query to it and goes one field down; the path goes back up', async ({
    page,
    mockServer,
  }) => {
    const asked = await openRed(page, mockServer.port);

    await page.locator('[data-red-group="checkout-api"] button').first().click();

    await expect(page.locator('[data-red-step="0"]')).toContainText('service.name = checkout-api');
    await expect(page.locator('[data-red-dimension]')).toHaveText('name');
    await expect(page.locator('[data-red-group="POST /api/orders"]')).toBeVisible();
    await expect(page.locator('[data-red-group="catalog"]')).toHaveCount(0);
    expect(filtersOf(asked.breakdown.at(-1))).toEqual([
      { field: 'service.name', op: '=', value: 'checkout-api' },
    ]);

    // The root of the path is every request again, broken down by the field that was left.
    await page.locator('[data-red-path] li').first().getByRole('button').click();

    await expect(page.locator('[data-red-step]')).toHaveCount(0);
    await expect(page.locator('[data-red-dimension]')).toHaveText('service.name');
    await expect(page.locator('[data-red-group="catalog"]')).toBeVisible();
  });

  test('every Span can be counted instead of the entry Spans', async ({ page, mockServer }) => {
    const asked = await openRed(page, mockServer.port);

    await page.locator('[data-red-path] [role="group"] button').nth(1).click();

    await expect.poll(() => asked.overview.at(-1)?.get('scope')).toBe('all');
    await expect.poll(() => new URL(page.url()).searchParams.get('scope')).toBe('all');
  });

  test('the failed requests alone, and the requests of one stretch of durations', async ({ page, mockServer }) => {
    const asked = await openRed(page, mockServer.port);

    await page.locator('[data-red-signal="errors"] [data-query-icon-action]').click();
    await expect(page.locator('[data-red-step="0"]')).toContainText('status_code = ERROR');

    await page.locator('[data-red-signal="duration"] [role="group"] button').nth(1).click();
    const plot = page.locator('[data-red-latency-plot]');
    await expect(plot).toBeVisible();
    await plot.focus();
    await page.keyboard.press('ArrowRight');
    await page.keyboard.press('Enter');

    // The shortest bar is closed on both sides: two steps, after the one for the failures.
    await expect(page.locator('[data-red-step]')).toHaveCount(3);
    await expect.poll(() => filtersOf(asked.overview.at(-1)).map((filter) => filter.field)).toContain('duration_ns');
  });

  test("a group's spans open in the Span view with its value in the query", async ({ page, mockServer }) => {
    await openRed(page, mockServer.port);

    await page.locator('[data-red-group="catalog"] [data-query-icon-action]').click();

    await expect(page.locator('[data-workspace-pane="span-results"]')).toBeVisible(LOADED);
    await expect(page.locator('[data-red-view]')).toHaveCount(0);
    // The address bar follows the tab in front a render later; the Span view is not the default.
    await expect.poll(() => new URL(page.url()).searchParams.get('tab')).toBe('spans');
  });
});
