import type { Page } from '@playwright/test';

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
    const fades = () =>
      scroller.evaluate((el) => ({
        start: getComputedStyle(el).getPropertyValue('--tabs-fade-start').trim(),
        end: getComputedStyle(el).getPropertyValue('--tabs-fade-end').trim(),
      }));
    await expect.poll(fades).toEqual({ start: '0px', end: '28px' });

    await scroller.evaluate((el) => el.scrollTo({ left: el.scrollWidth }));
    await expect.poll(fades).toEqual({ start: '28px', end: '0px' });

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
});
