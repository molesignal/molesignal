import type { Locator, Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

// The agent module renders from its own, mocked, endpoints and every tab of it
// is a link, which is all these checks need.
async function openAgent(page: Page, width = 1440) {
  await page.setViewportSize({ width, height: 800 });
  await page.goto('/agent/chat');
  const nav = page.getByRole('navigation', { name: /agent/i }).first();
  await expect(nav).toBeVisible();
  return nav;
}

/** How much of each end of a strip is faded out, the hint that tabs go on beyond it. */
function fades(scroller: Locator) {
  return scroller.evaluate((el) => ({
    start: getComputedStyle(el).getPropertyValue('--tabs-fade-start').trim(),
    end: getComputedStyle(el).getPropertyValue('--tabs-fade-end').trim(),
  }));
}

/** How far `tab` reaches past either edge of what `scroller` shows; nothing, when all of it is in view. */
async function cutOff(tab: Locator, scroller: Locator) {
  const [box, shown] = await Promise.all([tab.boundingBox(), scroller.boundingBox()]);
  return Math.max(0, shown!.x - box!.x, box!.x + box!.width - (shown!.x + shown!.width));
}

/**
 * Opens `path` in a browser that last showed the app in Chinese, signed in to
 * an account that has English saved. The shell applies the saved language over
 * the remembered one once it has read it, a moment after the page is up: every
 * label changes where it stands, and a strip whose tabs had room in Chinese
 * has tabs wider than itself in English. Nothing about the strip changes.
 *
 * The saved preferences are held back until the function that comes back is
 * called.
 */
async function openBeforeItsLanguage(page: Page, path: string, width: number) {
  await page.addInitScript(() => {
    localStorage.setItem(
      'molesignal-ui-prefs',
      JSON.stringify({ state: { language: 'zh-cn' }, version: 0 }),
    );
  });
  let arrive = () => {};
  const held = new Promise<void>((resolve) => {
    arrive = resolve;
  });
  await page.route(/\/api\/v1\/me\/preferences$/, async (route) => {
    await held;
    await route.fallback();
  });

  await page.setViewportSize({ width, height: 800 });
  await page.goto(path);
  await expect(page.locator('html')).toHaveAttribute('lang', 'zh-cn');

  return async () => {
    arrive();
    await expect(page.locator('html')).toHaveAttribute('lang', 'en-us');
  };
}

test.describe('module tabs', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('glides one indicator to the tab that was opened', async ({ page }) => {
    const nav = await openAgent(page);
    const indicator = nav.locator('[data-tab-indicator]');
    const before = await indicator.boundingBox();

    const settings = nav.getByRole('link', { name: /settings|设置/i });
    const target = await settings.boundingBox();
    await settings.click();
    await expect(settings).toHaveAttribute('aria-current', 'page');

    // It travels there rather than jumping, and comes to rest under the tab.
    await expect
      .poll(
        async () => {
          const box = await indicator.boundingBox();
          return (
            !!box &&
            box.x >= target!.x - 0.5 &&
            box.x + box.width <= target!.x + target!.width + 0.5
          );
        },
        { timeout: 3_000 },
      )
      .toBe(true);
    expect((await indicator.boundingBox())!.x).toBeGreaterThan((before?.x ?? 0) + 50);
  });

  test('does not move the other tabs when one becomes active', async ({ page }) => {
    const nav = await openAgent(page);
    const links = nav.getByRole('link');
    const widths = async () =>
      (await links.evaluateAll((els) => els.map((el) => Math.round(el.getBoundingClientRect().width))));
    const before = await widths();

    await links.nth(2).click();
    await expect(links.nth(2)).toHaveAttribute('aria-current', 'page');

    expect(await widths()).toEqual(before);
  });

  test('keeps every tab reachable when the strip is narrower than its tabs', async ({ page }) => {
    const nav = await openAgent(page, 420);
    const scroller = nav.locator('.tabs-scroll');
    const overflow = await scroller.evaluate((el) => el.scrollWidth - el.clientWidth);
    expect(overflow).toBeGreaterThan(0);

    // The edge with more tabs beyond it fades; the one already at the start does not.
    await expect.poll(() => fades(scroller)).toEqual({ start: '0px', end: '28px' });

    await scroller.evaluate((el) => el.scrollTo({ left: el.scrollWidth }));
    await expect.poll(() => fades(scroller)).toEqual({ start: '28px', end: '0px' });

    // Opening the last tab brings it fully into view.
    const last = nav.getByRole('link').last();
    await last.scrollIntoViewIfNeeded();
    await last.click();
    const [tab, strip] = await Promise.all([last.boundingBox(), scroller.boundingBox()]);
    expect(tab!.x + tab!.width).toBeLessThanOrEqual(strip!.x + strip!.width + 0.5);
  });

  test('moves focus with the arrow keys without opening the tab', async ({ page }) => {
    const nav = await openAgent(page);
    const links = nav.getByRole('link');
    await links.first().focus();

    await page.keyboard.press('ArrowRight');
    await expect(links.nth(1)).toBeFocused();
    await page.keyboard.press('End');
    await expect(links.last()).toBeFocused();
    await expect(page).toHaveURL(/\/agent\/chat/);
  });

  // The strip fills its row: it is exactly as wide before its tabs outgrow it
  // as after, and nobody has scrolled it.
  test('fades the end its tabs have grown past where they stand', async ({ page }) => {
    const languageArrives = await openBeforeItsLanguage(page, '/agent/chat', 600);
    // The module's own strip: on the settings a row of sections stands under it, a strip too.
    const scroller = page.getByTestId('agent-module-header').locator('.tabs-scroll').first();
    const hidden = () => scroller.evaluate((el) => el.scrollWidth - el.clientWidth);
    // In Chinese all six tabs fit.
    await expect(scroller.getByRole('link')).toHaveCount(6);
    expect(await hidden()).toBe(0);
    expect(await fades(scroller)).toEqual({ start: '0px', end: '0px' });

    await languageArrives();

    await expect.poll(hidden).toBeGreaterThan(0);
    await expect.poll(() => fades(scroller)).toEqual({ start: '0px', end: '28px' });
  });

  test('keeps the open tab in view when the tabs before it grow', async ({ page }) => {
    // Settings is the last tab: in English the five before it leave it no room.
    const languageArrives = await openBeforeItsLanguage(page, '/agent/settings', 600);
    // The module's own strip: on the settings a row of sections stands under it, a strip too.
    const scroller = page.getByTestId('agent-module-header').locator('.tabs-scroll').first();
    const open = scroller.locator('[aria-current="page"]');
    await expect(open).toHaveCount(1);
    expect(await cutOff(open, scroller)).toBeLessThan(1);

    await languageArrives();

    await expect.poll(() => scroller.evaluate((el) => el.scrollLeft)).toBeGreaterThan(0);
    await expect.poll(() => cutOff(open, scroller)).toBeLessThan(1);
    await expect.poll(() => fades(scroller)).toEqual({ start: '28px', end: '0px' });
  });
});

test.describe('view tabs', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  // The five views of RUM's settings do not fit a narrow window in either
  // language, and the last one is open: the strip is scrolled to its end, and
  // has to go further when the views grow.
  test('keeps the open view in view when the views grow', async ({ page }) => {
    const languageArrives = await openBeforeItsLanguage(page, '/rum/settings/session-replay', 420);
    const scroller = page.locator('[data-rum-subnavigation="settings"] .tabs-scroll');
    const open = scroller.locator('[aria-current="page"]');
    await expect(open).toHaveCount(1);
    await expect.poll(() => fades(scroller)).toEqual({ start: '28px', end: '0px' });
    expect(await cutOff(open, scroller)).toBeLessThan(1);
    const before = await scroller.evaluate((el) => el.scrollLeft);

    await languageArrives();

    await expect.poll(() => scroller.evaluate((el) => el.scrollLeft)).toBeGreaterThan(before);
    await expect.poll(() => cutOff(open, scroller)).toBeLessThan(1);
    await expect.poll(() => fades(scroller)).toEqual({ start: '28px', end: '0px' });
  });
});
