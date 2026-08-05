/**
 * Mobile (375px) shell visual baselines (P2-T2).
 *
 * The visual config only ever exercised 1440×900; there were no mobile
 * screenshots. This captures the shell at a 375px phone width (login +
 * authenticated home) in dark and light.
 *
 * Unlike 05-visual, the API mocks here return the *array* shapes the clients
 * actually expect (streams / dashboards / audit / alerts), so the home shell
 * renders real empty-state content instead of an error-boundary page.
 *
 * Generate baselines locally with:
 *   pnpm -C web playwright test --update-snapshots playwright/tests/11-visual-mobile.spec.ts
 */
import type { Route } from '@playwright/test';
import { expect, test } from '@playwright/test';

import { installMockSession } from '../fixtures/mockSession';

// Baselines are darwin-only (committed `*-chromium-darwin.png`); skip on the
// linux CI runner until linux baselines are regenerated (shared P2-T6
// follow-up with 05-visual / a11y-focus-ring).
test.beforeEach(() => {
  test.skip(
    process.platform === 'linux',
    'Visual baselines are darwin-only; linux baselines are a follow-up (P2-T6).',
  );
});

// 375×812 ≈ iPhone X/12 logical viewport.
test.use({ viewport: { width: 375, height: 812 } });

const THEME_KEY = 'molesignal-theme';
const DENSITY_KEY = 'molesignal-density';
const EXPLICIT_THEME_KEY = 'molesignal-theme-explicit';

type Theme = 'dark' | 'light';

const emptyArray = async (route: Route) => route.fulfill({ json: [] });

for (const theme of ['dark', 'light'] as Theme[]) {
  test.describe(`mobile @ theme=${theme}`, () => {
    test.beforeEach(async ({ page }) => {
      await page.clock.install({ time: new Date('2026-05-23T10:00:00.000Z') });
      await installMockSession(page);
      await page.addInitScript(
        ({ theme, themeKey, densityKey, explicitKey }) => {
          localStorage.setItem(themeKey, theme);
          localStorage.setItem(densityKey, 'comfortable');
          localStorage.setItem(explicitKey, '1');
        },
        {
          theme,
          themeKey: THEME_KEY,
          densityKey: DENSITY_KEY,
          explicitKey: EXPLICIT_THEME_KEY,
        },
      );
      // Array-shaped responses so list clients (`.slice`/`.map`) don't crash —
      // the home shell renders real empty states, not an error page.
      for (const path of [
        '**/api/v1/streams',
        '**/api/v1/dashboards',
        '**/api/v1/alerts/rules',
        '**/api/v1/alerts/incidents',
        '**/api/v1/pipelines',
        '**/api/v1/functions',
        '**/api/v1/audit**',
      ]) {
        await page.route(path, emptyArray);
      }
      await page.route('**/api/v1/web/topology**', (route) =>
        route.fulfill({ json: { nodes: [], edges: [] } }),
      );
      // Catch-all for anything else so nothing hangs the shell.
      await page.route('**/api/v1/**', (route) => route.fulfill({ json: {} }));
    });

    test('login page', async ({ page }) => {
      await page.goto('/login');
      await expect(page).toHaveScreenshot(`login-mobile-${theme}.png`, {
        fullPage: true,
        animations: 'disabled',
      });
    });

    test('home shell', async ({ page }) => {
      await page.goto('/home');
      await page.waitForLoadState('networkidle').catch(() => undefined);
      await expect(page).toHaveScreenshot(`home-mobile-${theme}.png`, {
        fullPage: true,
        animations: 'disabled',
        maxDiffPixelRatio: 0.005,
      });
    });
  });
}
