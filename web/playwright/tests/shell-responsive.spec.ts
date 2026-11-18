import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.beforeEach(async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
});

test('mobile search and navigation remain keyboard accessible', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/metrics');
  const search = page.getByTestId('command-palette-trigger');
  await expect(search.locator('span')).toBeHidden();
  const searchSize = await search.boundingBox();
  expect(searchSize?.width).toBe(searchSize?.height);
  await search.click();
  await expect(page.getByRole('dialog')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(search).toBeFocused();

  // The same menu as on a wide screen: it takes the phone's whole width under the top bar.
  const trigger = page.getByTestId('main-menu-trigger');
  const menu = page.getByTestId('main-menu');
  await trigger.click();
  await expect(menu).toBeVisible();
  await expect(menu.locator(':focus')).toHaveAttribute('data-menu-entry', 'metrics');
  expect((await menu.boundingBox())!.width).toBe(390);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  await page.keyboard.press('Escape');
  await expect(menu).toHaveCount(0);
  await expect(trigger).toBeFocused();
  await trigger.click();
  await menu.getByRole('link', { name: 'Logs', exact: true }).click();
  await expect(page).toHaveURL(/\/logs/);
  await expect(menu).toHaveCount(0);
});

test('every width gives the page the whole width, with no rail beside it', async ({ page }) => {
  for (const width of [390, 900, 1440]) {
    await page.setViewportSize({ width, height: 800 });
    await page.goto('/metrics');
    await expect(page.getByTestId('main-menu-trigger')).toBeVisible();
    await expect(page.getByTestId('primary-sidebar')).toHaveCount(0);
    await expect(page.locator('#main')).toHaveCSS('padding-left', '0px');
  }
});
