import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('account settings menu and preference fields', () => {
  // The light palette is where a field's hover fill used to fall onto the page canvas.
  test.use({ colorScheme: 'light' });

  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { theme: 'light' });
  });

  test('keeps the collapse control level with the search field and beside it, not over it', async ({
    page,
  }) => {
    await page.goto('/account/settings/preferences');

    const field = page.locator('nav[aria-label="Account settings"] label[data-ui="input-control"]');
    const collapse = page.getByRole('button', { name: 'Collapse menu' });
    await expect(collapse).toBeVisible();

    const [fieldBox, collapseBox] = await Promise.all([field.boundingBox(), collapse.boundingBox()]);
    expect(fieldBox).not.toBeNull();
    expect(collapseBox).not.toBeNull();

    // One row: the same vertical centre...
    const centre = (box: { y: number; height: number }) => box.y + box.height / 2;
    expect(Math.abs(centre(fieldBox!) - centre(collapseBox!))).toBeLessThanOrEqual(1);
    // ...and the control sits to the field's right instead of floating over its text.
    expect(collapseBox!.x).toBeGreaterThanOrEqual(fieldBox!.x + fieldBox!.width - 1);
  });

  test('collapses to the strip and expands again from the menu\'s own control', async ({ page }) => {
    await page.addInitScript(() => window.localStorage.removeItem('molesignal.subnav.v1'));
    await page.goto('/account/settings/preferences');

    const menu = page.locator('nav[aria-label="Account settings"]');
    await expect(menu).toBeVisible();
    await page.getByRole('button', { name: 'Collapse menu' }).click();
    await expect(menu).toBeHidden();

    await page.getByRole('button', { name: 'Expand menu' }).click();
    await expect(menu).toBeVisible();
    await expect(page.getByRole('button', { name: 'Collapse menu' })).toBeVisible();
  });

  test('keeps every preference field filled, one step darker, when pointed at or opened', async ({
    page,
  }) => {
    await page.goto('/account/settings/preferences');

    // Computed colours come back in different colour spaces; paint them to compare.
    const painted = (colour: string) =>
      page.evaluate((value) => {
        const canvas = document.createElement('canvas');
        canvas.width = 1;
        canvas.height = 1;
        const context = canvas.getContext('2d')!;
        context.fillStyle = value;
        context.fillRect(0, 0, 1, 1);
        return Array.from(context.getImageData(0, 0, 1, 1).data).slice(0, 3);
      }, colour);
    const fillOf = (selector: string) =>
      page.locator(selector).first().evaluate((element) => getComputedStyle(element).backgroundColor);
    const canvasFill = await painted(
      await page.locator('main#main').evaluate((element) => getComputedStyle(element).backgroundColor),
    );
    const sum = (rgb: number[]) => rgb[0]! + rgb[1]! + rgb[2]!;

    for (const selector of [
      '[role="combobox"][aria-label="Default timezone"]',
      'button[aria-label="Date & time format"]',
      '[role="combobox"][aria-label="Open after entering workspace"]',
    ]) {
      const field = page.locator(selector).first();
      await field.scrollIntoViewIfNeeded();
      const rest = await painted(await fillOf(selector));

      await field.hover();
      // The fill eases from one palette step to the next; wait for it to settle.
      await expect
        .poll(async () => (await painted(await fillOf(selector))).join())
        .not.toBe(rest.join());
      const hovered = await painted(await fillOf(selector));
      // Never the page canvas (the field would vanish), and darker than at rest.
      expect(hovered.join(), `${selector} hovered`).not.toBe(canvasFill.join());
      expect(sum(hovered), `${selector} hovered`).toBeLessThan(sum(rest));

      await field.click();
      await expect(field).toHaveAttribute('aria-expanded', 'true');
      await expect
        .poll(async () => (await painted(await fillOf(selector))).join())
        .not.toBe(canvasFill.join());
      await page.keyboard.press('Escape');
      await page.mouse.move(2, 2);
    }
  });

  test('draws its selects in one fill, weight and colour', async ({
    page,
  }) => {
    await page.goto('/account/settings/preferences');

    const read = (name: string) =>
      page.getByRole('combobox', { name, exact: true }).evaluate((element) => {
        const style = getComputedStyle(element);
        return [style.backgroundColor, style.fontWeight, style.color].join('|');
      });
    expect(await read('Open after entering workspace')).toBe(await read('Default timezone'));
  });
});
