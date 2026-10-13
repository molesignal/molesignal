import type { Page } from '@playwright/test';

import { expect, FROZEN_NOW_ISO, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;
/** A tenth of a second, in the nanoseconds a CPU profile counts in. */
const S = 100_000_000;
const MIB = 1024 * 1024;

// runtime.main → checkout.handle → business.execute → { database.query, cache.lookup }: five rows.
const CPU_FLAME = {
  names: ['total', 'runtime.main', 'checkout.handle', 'business.execute', 'database.query', 'cache.lookup'],
  // offset, total, self, name
  levels: [
    [0, 100 * S, 0, 0],
    [0, 100 * S, 0, 1],
    [0, 100 * S, 10 * S, 2],
    [0, 90 * S, 10 * S, 3],
    [0, 60 * S, 60 * S, 4, 0, 20 * S, 20 * S, 5],
  ],
  numTicks: 100 * S,
  maxSelf: 60 * S,
  units: 'nanoseconds',
};

const ALLOC_FLAME = {
  names: ['total', 'runtime.main', 'json.parse'],
  levels: [
    [0, 8 * MIB, 0, 0],
    [0, 8 * MIB, 2 * MIB, 1],
    [0, 6 * MIB, 6 * MIB, 2],
  ],
  numTicks: 8 * MIB,
  maxSelf: 6 * MIB,
  units: 'bytes',
};

function profile(id: string, type: string, service: string, minutesAgo: number) {
  return {
    id,
    service,
    profile_type: type,
    timestamp: NOW - minutesAgo * 60_000_000,
    total_value: type === 'cpu' ? 25 * S : 4 * MIB,
    sample_count: 100,
    duration_nanos: 60_000_000_000,
    unsymbolized: false,
  };
}

// Memory profiles first in the answer: the page opens on CPU by the order types are offered in.
const PROFILES = [
  profile('alloc-0', 'alloc_space', 'checkout', 2),
  profile('alloc-1', 'alloc_space', 'payments', 3),
  profile('cpu-0', 'cpu', 'checkout', 4),
  profile('cpu-1', 'cpu', 'payments', 5),
  profile('cpu-2', 'cpu', 'checkout', 6),
  profile('cpu-3', 'cpu', 'payments', 7),
];

/** Serves the window's profiles and records which profiles each flame graph was merged from. */
async function serveProfiles(page: Page): Promise<string[][]> {
  const merged: string[][] = [];

  await page.route(/\/api\/v1\/profiles(?:\?.*)?$/, (route) =>
    route.fulfill({ json: { profiles: PROFILES } }),
  );
  await page.route(/\/api\/v1\/profiles\/flamegraph\/selection$/, (route) => {
    const ids = (route.request().postDataJSON() as { profile_ids: string[] }).profile_ids;
    merged.push(ids);

    return route.fulfill({
      json: {
        flamebearer: ids[0]?.startsWith('alloc') ? ALLOC_FLAME : CPU_FLAME,
        truncated: false,
        profile_count: ids.length,
      },
    });
  });

  return merged;
}

test.describe('Profile analysis', () => {
  let merged: string[][];

  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    merged = await serveProfiles(page);
    await page.goto('/profiles');
    await expect(page.locator('[data-profile-view="flame"]')).toBeVisible();
  });

  test('opens on the first type the window holds and merges that type alone', async ({ page }) => {
    const type = page.locator('[data-profile-type-select]');
    const scope = page.locator('[data-profile-summary] [data-profile-scope]');

    // Seconds of CPU and bytes of memory are not added up: the graph is of one type.
    await expect(type).toHaveText('CPU');
    await expect(scope).toContainText('CPU 10 s');
    await expect(page.locator('[data-profile-selection-open]')).toHaveText('4 / 4 profiles');
    expect(merged.at(-1)).toEqual(['cpu-0', 'cpu-1', 'cpu-2', 'cpu-3']);

    await type.click();
    // The types the window has profiles of come first; the rest are offered, set back. No "all".
    await expect(page.getByRole('option')).toHaveText([
      'CPU',
      'Allocated memory',
      'Wall time',
      'Allocated objects',
      'In-use memory',
      'In-use objects',
      'Goroutines',
      'Lock contention',
    ]);
    await expect(page.locator('[role="option"][data-fitted-muted]')).toHaveCount(6);

    await page.getByRole('option', { name: 'Allocated memory' }).click();

    await expect(scope).toContainText('Allocated memory 8.0 MiB');
    await expect(page.locator('[data-profile-selection-open]')).toHaveText('2 / 2 profiles');
    expect(merged.at(-1)).toEqual(['alloc-0', 'alloc-1']);
  });

  test('leads with the hottest function in one line over the graph', async ({ page }) => {
    const summary = page.locator('[data-profile-summary]');
    const hottest = summary.locator('[data-profile-hottest]');

    await expect(hottest).toContainText('database.query');
    await expect(hottest).toContainText('60.0% self');
    // One line, not a band of figures.
    expect((await summary.boundingBox())!.height).toBeLessThanOrEqual(48);

    await hottest.click();
    await expect(
      page.locator('[data-profile-view] button[aria-label="Analyze function database.query"]'),
    ).toHaveClass(/ring-indigo/);
  });

  test('the graph is as tall as its stack, and the tip of a frame does not make it scroll', async ({
    page,
  }) => {
    const view = page.locator('[data-profile-view="flame"]');
    const bars = view.locator('button[aria-label]');
    const box = () =>
      view.evaluate((element) => ({
        height: element.clientHeight,
        width: element.clientWidth,
        scrollHeight: element.scrollHeight,
        scrollWidth: element.scrollWidth,
      }));

    // The root is where a call path starts, not a function called "total".
    await expect(bars.first()).toHaveText('/');
    const before = await box();
    // Five rows of 24px and the room around them: a shallow stack does not get the room of a deep one.
    expect(before.height).toBe(5 * 24 + 8);
    expect(before.scrollHeight).toBe(before.height);

    // The last row: where a tip under the pointer has the least room in the view.
    await bars.last().hover();
    const tip = page.locator('[data-flame-tooltip]');
    await expect(tip).toBeVisible();
    await expect(tip).toContainText('cache.lookup');

    // The tip stands over the page. Laid out in the view it was taller than the view: a scrollbar
    // came with every hover and took its width from the bars.
    expect(await tip.evaluate((element) => Boolean(element.closest('[data-profile-view]')))).toBe(false);
    expect(await box()).toEqual(before);
    const tipBox = (await tip.boundingBox())!;
    expect(tipBox.x + tipBox.width).toBeLessThanOrEqual(1440);
    expect(tipBox.y + tipBox.height).toBeLessThanOrEqual(900);
    // The tip names the frame once: no native title on the bar says it again over the tip.
    expect(await bars.evaluateAll((elements) => elements.filter((element) => element.hasAttribute('title')).length)).toBe(0);
  });

  test('the profiles the graph is merged from are chosen from the summary', async ({ page }) => {
    await page.locator('[data-profile-selection-open]').click();
    const drawer = page.locator('[data-profile-selection]');

    await expect(drawer).toBeVisible();
    await expect(drawer.locator('[data-profile-selection-count]')).toHaveText('4 / 4 selected');
    await expect(drawer.locator('tbody tr')).toHaveCount(4);

    await drawer.getByRole('checkbox', { name: 'Toggle profile cpu-0 in analysis' }).click();

    await expect(drawer.locator('[data-profile-selection-count]')).toHaveText('3 / 4 selected');
    await expect(page.locator('[data-profile-selection-open]')).toHaveText('3 / 4 profiles');
    await expect.poll(() => merged.at(-1)).toEqual(['cpu-1', 'cpu-2', 'cpu-3']);

    await page.keyboard.press('Escape');
    await expect(drawer).toBeHidden();
    // The list is no longer a panel folded at the foot of the page.
    await expect(page.getByRole('button', { name: 'Expand list' })).toHaveCount(0);
  });

  test('comparing is a way on from the analysis, and takes its scope along', async ({ page }) => {
    const compare = page.getByRole('button', { name: 'Compare profiles' });
    const fill = (name: string) =>
      page.getByRole('button', { name, exact: true }).evaluate((element) => getComputedStyle(element).backgroundColor);

    // The page is for reading the graph: comparing is not its filled action.
    expect(await fill('Compare profiles')).toBe(await fill('Refresh'));

    await compare.click();

    await expect(page).toHaveURL(/\/profiles\/compare\?type=cpu$/);
  });
});
