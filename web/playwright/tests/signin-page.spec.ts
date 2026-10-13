import type { Page } from '@playwright/test';

import { expect, test } from '../fixtures/mockBackend';

async function openSignin(page: Page, instance: { signup_enabled: boolean }) {
  await page.route('**/api/v1/auth/sso/providers', (route) => route.fulfill({ json: [] }));
  await page.route('**/api/v1/instance', (route) =>
    route.fulfill({
      json: { external_url: '', version: '26.0.0.0', ...instance },
    }),
  );
  await page.goto('/signin');
  await expect(page.getByTestId('signin-card')).toBeVisible();
}

test.describe('Sign-in page', () => {
  test('puts one form card in a centred column, between the brand and the version', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openSignin(page, { signup_enabled: false });

    const card = (await page.getByTestId('signin-card').boundingBox())!;
    const logo = (await page.getByTestId('signin-logo').boundingBox())!;
    const version = (await page.getByTestId('signin-build-info').boundingBox())!;

    // Centred in the viewport, and a form-sized card, not a split panel.
    expect(Math.abs(card.x + card.width / 2 - 720)).toBeLessThanOrEqual(1);
    expect(card.width).toBeLessThanOrEqual(440);
    // Reading order: brand, card, version.
    expect(logo.y + logo.height).toBeLessThanOrEqual(card.y);
    expect(card.y + card.height).toBeLessThanOrEqual(version.y);
  });

  test('shows the product version and nothing about the release channel', async ({ page }) => {
    await openSignin(page, { signup_enabled: false });

    await expect(page.getByTestId('signin-build-info')).toHaveText('v26.0.0.0');
    await expect(page.getByText(/alpha|beta|stable/i)).toHaveCount(0);
  });

  test('offers the sign-up entry only when the instance allows it', async ({ page }) => {
    await openSignin(page, { signup_enabled: false });
    await expect(page.getByRole('button', { name: /Sign up/ })).toHaveCount(0);

    await page.unrouteAll();
    await openSignin(page, { signup_enabled: true });
    await expect(page.getByRole('button', { name: /Sign up/ })).toBeVisible();
  });

  for (const viewport of [
    { width: 390, height: 844 },
    { width: 320, height: 568 },
  ]) {
    test(`fits a ${viewport.width}px phone without scrolling sideways`, async ({ page }) => {
      await page.setViewportSize(viewport);
      await openSignin(page, { signup_enabled: true });

      const overflow = await page.evaluate(() => {
        const stage = document.querySelector<HTMLElement>('[data-testid="auth-stage"]')!;
        return {
          page: document.documentElement.scrollWidth - window.innerWidth,
          stage: stage.scrollWidth - stage.clientWidth,
        };
      });
      expect(overflow.page).toBeLessThanOrEqual(0);
      expect(overflow.stage).toBeLessThanOrEqual(0);

      // A short screen scrolls the stage instead of clipping the form.
      const submit = page.getByRole('button', { name: /Sign in/ }).last();
      await submit.scrollIntoViewIfNeeded();
      const box = (await submit.boundingBox())!;
      expect(box.y).toBeGreaterThanOrEqual(0);
      expect(box.y + box.height).toBeLessThanOrEqual(viewport.height);
    });
  }
});
