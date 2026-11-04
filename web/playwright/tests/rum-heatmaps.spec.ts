import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_RUM } from '../fixtures/mockRumData';

const ADDRESS = `/rum/analytics/heatmaps?app=${MOCK_RUM.applications.web}&time=now-7d..now`;

test.describe('RUM heatmaps', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('is a view of the analytics tab', async ({ page }) => {
    await page.goto(ADDRESS);

    await expect(page.locator('[data-rum-subnavigation="analytics"]')).toContainText('Heatmap');
    await expect(page.locator('[data-rum-analytics="heatmap"]')).toBeVisible();
  });

  test('draws the clicks of a page over a recording of it, and names what the recording does not have', async ({ page }) => {
    await page.goto(`${ADDRESS}&page=/checkout`);

    await expect(page.locator('[data-rum-heatmap-stage="clicks"]')).toBeVisible();
    await expect(page.locator('[data-rum-heatmap-canvas]')).toHaveAttribute('data-heat-spots', /^[1-9]/);

    const canvasHasHeat = () =>
      page.locator('[data-rum-heatmap-canvas]').evaluate((canvas: HTMLCanvasElement) => {
        const context = canvas.getContext('2d');

        return context ? context.getImageData(0, 0, canvas.width, canvas.height).data.some((value, index) => index % 4 === 3 && value > 0) : false;
      });

    await expect.poll(canvasHasHeat).toBe(true);

    const pay = page.locator('[data-heat-element="body>button"]');

    await expect(pay).toContainText('Pay now');
    await expect(pay.locator('[data-heat-off]')).toHaveCount(0);

    const elsewhere = page.locator('[data-heat-element^="body>main"]').first();

    await expect(elsewhere.locator('[data-heat-off]')).toBeVisible();
    await expect(page.locator('[data-rum-heatmap-session]')).toHaveAttribute('href', /^\/rum\/sessions\/view\/[0-9a-f-]+\?at=\d+$/);
  });

  test('shades a page by how far down people got, and says the depths in numbers', async ({ page }) => {
    await page.goto(`${ADDRESS}&page=/checkout&map=scroll`);

    await expect(page.locator('[data-rum-heatmap-stage="scroll"]')).toBeVisible();
    await expect(page.locator('[data-rum-median-depth]')).toContainText('px');
    await expect(page.locator('[data-rum-scroll-line="0.5"]')).toBeVisible();
    await expect(page.locator('[data-rum-reached="0"]')).toHaveText('100%');
  });

  test('a page without a recording keeps its numbers', async ({ page }) => {
    await page.goto(`${ADDRESS}&page=/pricing`);

    await expect(page.locator('[data-rum-heatmap-no-recording]')).toBeVisible();
    await expect(page.locator('[data-rum-heatmap-canvas]')).toHaveCount(0);
    await expect(page.locator('[data-rum-overview-card="heatmap-numbers"]')).toBeVisible();
  });

  test('another size of window is another map, kept in the address', async ({ page }) => {
    await page.goto(`${ADDRESS}&page=/checkout`);
    await page.locator('[data-rum-heatmap-device]').click();
    await page.getByRole('option', { name: 'Phone' }).click();

    await expect(page).toHaveURL(/device=mobile/);
    await expect(page.locator('[data-rum-heatmap-no-recording]')).toBeVisible();
  });

  test('an application of an app says why it has none', async ({ page }) => {
    await page.goto(`/rum/analytics/heatmaps?app=${MOCK_RUM.applications.app}&time=now-7d..now`);

    await expect(page.getByText('Heatmaps are for browser applications')).toBeVisible();
  });
});

test.describe('RUM heatmaps in Chinese', () => {
  test('speak it', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });
    await page.goto(`${ADDRESS}&page=/checkout`);

    await expect(page.locator('[data-rum-overview-card="heatmap-numbers"]')).toContainText('被点击的元素');
  });
});
