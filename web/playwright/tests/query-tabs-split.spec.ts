import type { Locator, Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { expectExplorerResults, serveExplorerResults } from '../fixtures/mockExplorers';

/** The halves of a split page, left then right. */
const halves = (page: Page) => page.locator('[data-query-pane]');
const half = (page: Page, side: 0 | 1) => page.locator(`[data-query-pane="${side}"]`);
/** The strips of tabs, one over each half. */
const strips = (page: Page) => page.locator('[data-query-tab-strip]');
const splitSwitch = (page: Page) => page.locator('[data-query-split]');
const timeRange = (pane: Locator) => pane.locator('[data-time-range-trigger="true"]');

const boxOf = async (locator: Locator) => {
  const box = await locator.boundingBox();
  if (!box) throw new Error('not laid out');

  return box;
};

async function openSplitLogs(page: Page) {
  await page.goto('/logs');
  await expectExplorerResults(page, 'logs');
  await splitSwitch(page).click();
  await expect(halves(page)).toHaveCount(2);
}

test.describe('Query tabs split in two', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await mountMockRoutes(page, mockServer.port);
    await serveExplorerResults(page);
  });

  test('the page is split in two equal halves, each with its own tabs over it, and a narrow seam', async ({
    page,
  }) => {
    await page.goto('/logs');
    await expectExplorerResults(page, 'logs');
    await expect(halves(page)).toHaveCount(1);
    await expect(splitSwitch(page)).toHaveAttribute('aria-pressed', 'false');
    await expect(splitSwitch(page)).toHaveAccessibleName('Split in two');

    await splitSwitch(page).click();
    await expect(halves(page)).toHaveCount(2);
    await expect(splitSwitch(page)).toHaveAttribute('aria-pressed', 'true');

    // Two halves of the room beside the sidebar, as wide as each other, with nothing between.
    const [left, right] = [await boxOf(half(page, 0)), await boxOf(half(page, 1))];
    expect(Math.abs(left.width - right.width)).toBeLessThanOrEqual(1);
    expect(Math.abs(left.x + left.width - right.x)).toBeLessThanOrEqual(1);
    expect(Math.abs(right.x + right.width - 1440)).toBeLessThanOrEqual(1);

    // Each half has its strip over it, listing its own tab; the copy on the right is in front.
    await expect(strips(page)).toHaveCount(2);
    for (const side of [0, 1] as const) {
      const strip = await boxOf(strips(page).nth(side));
      const pane = side === 0 ? left : right;
      expect(strip.x).toBeGreaterThanOrEqual(pane.x - 1);
      expect(strip.x + strip.width).toBeLessThanOrEqual(pane.x + pane.width + 1);
      expect(strip.y + strip.height).toBeLessThanOrEqual(pane.y + 1);
      await expect(strips(page).nth(side).getByRole('tab')).toHaveCount(1);
      await expect(strips(page).nth(side).getByRole('tab')).toHaveAccessibleName('application_logs');
    }
    await expect(strips(page).nth(0)).toHaveAttribute('data-query-tab-strip-focused', 'false');
    await expect(strips(page).nth(1)).toHaveAttribute('data-query-tab-strip-focused', 'true');

    // The seam is 12px of canvas between the two workbench bars, not two page gutters.
    const bars = page.locator('[data-query-pane] [data-query-workbench-appearance="surface"]');
    const [leftBar, rightBar] = [await boxOf(bars.nth(0)), await boxOf(bars.nth(1))];
    expect(rightBar.x - (leftBar.x + leftBar.width)).toBe(12);
    // The outer gutters are the page's own.
    expect(leftBar.x - left.x).toBe(20);
    expect(right.x + right.width - (rightBar.x + rightBar.width)).toBe(20);

    // A half is too narrow for a list of fields beside the results: it is folded, and can be opened.
    for (const side of [0, 1] as const) {
      expect((await boxOf(half(page, side).locator('aside').first())).width).toBeLessThan(60);
    }
    // Both halves list the logs, each from a query of its own.
    for (const side of [0, 1] as const) {
      await expect(half(page, side).locator('[data-log-result-row="logs"]').first()).toBeVisible();
    }
  });

  test('each half keeps its own query and range, and the address is that of the half that is worked in', async ({
    page,
  }) => {
    await openSplitLogs(page);

    // The right half is in front: its range changes, the left one's does not.
    await timeRange(half(page, 1)).click();
    await page.getByRole('button', { name: 'Last 6 hours' }).click();
    await expect(timeRange(half(page, 1))).toContainText('Last 6 hours');
    await expect(timeRange(half(page, 0))).toContainText('Last 1 hour');
    await expect(page).toHaveURL(/time=now-6h\.\.now/);

    // Touching the left half brings it to the front: the address is its own again.
    await half(page, 0).locator('[data-log-result-row="logs"]').first().click({ position: { x: 4, y: 4 } });
    await expect(half(page, 0)).toHaveAttribute('data-active', 'true');
    await expect(half(page, 1)).toHaveAttribute('data-active', 'false');
    await expect(page).not.toHaveURL(/time=/);
    await expect(strips(page).nth(0)).toHaveAttribute('data-query-tab-strip-focused', 'true');
    // The right half is still on the page, on its own range.
    await expect(timeRange(half(page, 1))).toContainText('Last 6 hours');
  });

  test('a tab opens in the half it is asked for in, moves across, and the last tab of a half takes the half with it', async ({
    page,
  }) => {
    await openSplitLogs(page);

    await strips(page).nth(0).getByRole('button', { name: 'New tab' }).click();
    await expect(strips(page).nth(0).getByRole('tab')).toHaveCount(2);
    await expect(strips(page).nth(1).getByRole('tab')).toHaveCount(1);
    await expect(half(page, 0)).toHaveAttribute('data-active', 'true');

    // A tab is moved to the other half from its menu.
    await strips(page).nth(0).getByRole('tab').first().click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Move to the right half' }).click();
    await expect(strips(page).nth(0).getByRole('tab')).toHaveCount(1);
    await expect(strips(page).nth(1).getByRole('tab')).toHaveCount(2);
    await expect(half(page, 1)).toHaveAttribute('data-active', 'true');

    // The left half's only tab is closed: the page goes back to one, with the tabs in one strip.
    await strips(page).nth(0).getByRole('tab').click();
    await strips(page).nth(0).getByRole('button', { name: /^Close / }).click();
    await expect(halves(page)).toHaveCount(1);
    await expect(strips(page)).toHaveCount(1);
    await expect(strips(page).getByRole('tab')).toHaveCount(2);
    await expect(splitSwitch(page)).toHaveAttribute('aria-pressed', 'false');
  });

  test('the split is there after a reload, and the same switch takes it off', async ({ page }) => {
    await openSplitLogs(page);

    await page.reload();
    await expect(halves(page)).toHaveCount(2);
    for (const side of [0, 1] as const) {
      await expect(half(page, side).locator('[data-log-result-row="logs"]').first()).toBeVisible();
    }

    await splitSwitch(page).click();
    await expect(halves(page)).toHaveCount(1);
    await expect(strips(page)).toHaveCount(1);
    // Both tabs are still open, in one list.
    await expect(strips(page).getByRole('tab')).toHaveCount(2);
    await expect(splitSwitch(page)).toHaveAccessibleName('Split in two');
    // With the page to itself, the list of fields is back beside the results.
    expect((await boxOf(halves(page).locator('aside').first())).width).toBeGreaterThan(200);
  });

  test('Traces and Metrics split the same way', async ({ page }) => {
    for (const explorer of ['traces', 'metrics'] as const) {
      await page.goto(`/${explorer}`);
      await expect(splitSwitch(page)).toBeVisible();
      await splitSwitch(page).click();
      await expect(halves(page)).toHaveCount(2);
      await expect(strips(page)).toHaveCount(2);

      const [left, right] = [await boxOf(half(page, 0)), await boxOf(half(page, 1))];
      expect(Math.abs(left.width - right.width)).toBeLessThanOrEqual(1);
      // Nothing of a half reaches past it.
      for (const side of [0, 1] as const) {
        expect(await half(page, side).evaluate((element) => element.scrollWidth - element.clientWidth)).toBe(0);
      }
    }
  });

  test('a window too narrow for two halves is not split, and has no switch for it', async ({ page }) => {
    await openSplitLogs(page);

    // Narrowed under two halves: the half in front has the page, and the split is still asked for.
    await page.setViewportSize({ width: 700, height: 800 });
    await expect(halves(page)).toHaveCount(1);
    await expect(strips(page)).toHaveCount(1);
    await expect(strips(page).getByRole('tab')).toHaveCount(2);

    // Widened again, it is split as it was left.
    await page.setViewportSize({ width: 1440, height: 900 });
    await expect(halves(page)).toHaveCount(2);

    // A page that is not split cannot be, in a narrow window: the switch is not offered.
    await splitSwitch(page).click();
    await expect(halves(page)).toHaveCount(1);
    await page.setViewportSize({ width: 700, height: 800 });
    await expect(splitSwitch(page)).toHaveCount(0);
  });
});
