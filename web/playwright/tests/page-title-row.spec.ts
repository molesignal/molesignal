import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('Page title row', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  for (const width of [1440, 1280, 1024]) {
    test(`keeps the title whole at ${width}px`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto('/apm/transactions');

      const heading = page.getByTestId('page-header').getByRole('heading', { level: 1 });
      await expect(heading).toBeVisible();
      // No ellipsis: the title is never the part that gives way.
      expect(await heading.evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(false);
    });
  }

  test('keeps what the page is scoped to out of the title row', async ({ page }) => {
    await page.setViewportSize({ width: 1024, height: 900 });
    await page.goto('/apm/transactions');

    const header = page.getByTestId('page-header');
    const navigation = page.locator('[data-apm-navigation="surface"]');
    // The filters and the time range belong to the navigation card under the title…
    await expect(navigation.getByLabel('Namespace')).toBeVisible();
    await expect(navigation.getByRole('button', { name: /^Time range:/ })).toBeVisible();
    // …so the title row stays one line, with nothing squeezed in beside the title.
    await expect(header.getByLabel('Namespace')).toHaveCount(0);
    const row = (await header.locator('[data-page-title-row]').boundingBox())!;
    expect(row.height).toBeLessThanOrEqual(40);
  });

  test('keeps the description where the title has room for it', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto('/home');
    await expect(page.getByTestId('page-header').getByText('·')).toBeVisible();
  });

  test('drops the description rather than show a fragment of it', async ({ page }) => {
    // 360px leaves the title block narrower than 24rem: the title stands alone.
    await page.setViewportSize({ width: 360, height: 800 });
    await page.goto('/home');

    const header = page.getByTestId('page-header');
    await expect(header.getByRole('heading', { level: 1 })).toBeVisible();
    await expect(header.getByText('·')).toBeHidden();
    await expect(header.getByText('System status, anomalies, and next actions')).toBeHidden();
  });
});
