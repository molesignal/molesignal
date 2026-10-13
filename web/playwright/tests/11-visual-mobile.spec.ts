/**
 * Mobile (375px) shell visual baselines.
 *
 * The visual config only ever exercised 1440×900; there were no mobile
 * screenshots. This captures the shell at a 375px phone width (sign-in +
 * authenticated home) in dark and light.
 *
 * The shared mock backend answers every /api/v1/* call, so the home shell
 * renders its real content instead of an error page.
 *
 * Generate baselines locally with:
 *   pnpm -C web playwright test --update-snapshots playwright/tests/11-visual-mobile.spec.ts
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

// Baselines are darwin-only (committed `*-chromium-darwin.png`); skip on the
// linux CI runner until linux baselines are regenerated for all visual suites.
test.beforeEach(() => {
  test.skip(
    process.platform === 'linux',
    'Visual baselines are darwin-only; linux baselines have not been generated.',
  );
});

// 375×812 ≈ iPhone X/12 logical viewport.
test.use({ viewport: { width: 375, height: 812 } });

type Theme = 'dark' | 'light';

for (const theme of ['dark', 'light'] as Theme[]) {
  test.describe(`mobile @ theme=${theme}`, () => {
    test('sign-in page', async ({ page, mockServer }) => {
      await mountMockRoutes(page, mockServer.port, {
        theme,
        density: 'comfortable',
        signedOut: true,
      });
      await page.goto('/signin');
      await expect(page.getByTestId('signin-card')).toBeVisible();
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
      await expect(page).toHaveScreenshot(`login-mobile-${theme}.png`, {
        fullPage: true,
        animations: 'disabled',
      });
    });

    test('home shell', async ({ page, mockServer }) => {
      // The theme is the one the mock user has saved: the shell applies the
      // saved preferences over what the browser remembered.
      await mountMockRoutes(page, mockServer.port, {
        theme,
        density: 'comfortable',
        preferences: { theme, density: 'comfortable' },
      });
      await page.goto('/home');
      await page.waitForLoadState('networkidle').catch(() => undefined);
      await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
      // The page is the home page, not a shell that could not load its access.
      await expect(page.locator('[data-home-surface="hero"]')).toBeVisible();
      await expect(page).toHaveScreenshot(`home-mobile-${theme}.png`, {
        fullPage: true,
        animations: 'disabled',
        maxDiffPixelRatio: 0.005,
      });
    });
  });
}
