/**
 * 04 — keyboard-only navigation + axe a11y baseline
 *
 * Two checks:
 *   1. Palette opens on ⌘K and help overlay opens on ⌘/ — without ever
 *      touching the mouse. The ⌘K dialog is waited out (detached) before
 *      ⌘/ is pressed so the two dialogs never race.
 *   2. axe-core finds zero `critical` violations on the post-login shell.
 */
import AxeBuilder from '@axe-core/playwright';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('a11y / keyboard', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('keyboard-only palette + help; axe scan has no critical violations', async ({ page }) => {
    await page.goto('/investigate');

    // Palette open + close (Esc). Wait for the input to fully detach so the
    // next help keystroke does not get swallowed by an in-flight unmount.
    await page.keyboard.press('Meta+K');
    const paletteInput = page.getByPlaceholder(/search commands/i);
    await expect(paletteInput).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(paletteInput).toBeHidden({ timeout: 5_000 });

    // Help overlay open via modified shortcut.
    await page.keyboard.press('Meta+/');
    await expect(page.getByText(/keyboard shortcuts/i)).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByText(/keyboard shortcuts/i)).toBeHidden({ timeout: 5_000 });

    // axe scan on the resting shell. Exclude live-region status strip so
    // mid-route announce events don't get sampled.
    const results = await new AxeBuilder({ page })
      .exclude('[role="status"]')
      .exclude('[aria-live]')
      .analyze();
    const critical = results.violations.filter((v) => v.impact === 'critical');
    expect(critical, JSON.stringify(critical, null, 2)).toEqual([]);
  });

  test('core routes keep tab focus visible and out of hidden subtrees', async ({
    page,
  }) => {
    for (const route of ['/home', '/logs', '/metrics', '/traces', '/alerts']) {
      await page.goto(route);
      await page.waitForLoadState('networkidle').catch(() => undefined);
      await page.locator('body').click({ position: { x: 2, y: 2 } });

      for (let index = 0; index < 24; index += 1) {
        await page.keyboard.press('Tab');
        const focus = await page.evaluate(() => {
          const active = document.activeElement as HTMLElement | null;
          if (!active || active === document.body) return null;
          const rect = active.getBoundingClientRect();
          const style = getComputedStyle(active);
          return {
            tag: active.tagName,
            hiddenAncestor: Boolean(active.closest('[aria-hidden="true"], [inert]')),
            visible:
              rect.width > 0 &&
              rect.height > 0 &&
              rect.bottom > 0 &&
              rect.right > 0 &&
              rect.top < window.innerHeight &&
              rect.left < window.innerWidth,
            filter: style.filter,
            background: style.backgroundColor,
          };
        });
        expect(focus, `${route} tab ${index + 1} did not reach a control`).not.toBeNull();
        expect(focus?.hiddenAncestor, `${route} focused a hidden control`).toBe(false);
        expect(focus?.visible, `${route} focus moved off screen`).toBe(true);
        expect(
          focus?.filter !== 'none' || focus?.background !== 'rgba(0, 0, 0, 0)',
          `${route} focus has no visible fill/foreground treatment`,
        ).toBe(true);
      }
    }
  });
});
