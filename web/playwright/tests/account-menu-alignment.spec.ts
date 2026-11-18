import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('account settings menu and preference fields', () => {
  // The light palette is where a field's hover fill used to fall onto the page canvas.
  test.use({ colorScheme: 'light' });

  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { theme: 'light' });
  });

  test('carries the collapse control at the end of a search field as wide as the menu', async ({
    page,
  }) => {
    await page.goto('/account/settings/preferences');

    const menu = page.locator('nav[aria-label="Account settings"]');
    const field = menu.locator('label[data-ui="input-control"]');
    const collapse = field.getByRole('button', { name: 'Collapse menu' });
    await expect(collapse).toBeVisible();

    const [menuBox, fieldBox, inputBox, collapseBox] = await Promise.all([
      menu.boundingBox(),
      field.boundingBox(),
      field.getByRole('textbox').boundingBox(),
      collapse.boundingBox(),
    ]);
    expect(menuBox).not.toBeNull();
    expect(fieldBox).not.toBeNull();
    expect(inputBox).not.toBeNull();
    expect(collapseBox).not.toBeNull();

    // The field takes the menu's whole row.
    expect(Math.abs(fieldBox!.width - menuBox!.width)).toBeLessThanOrEqual(1);
    // The control stands inside it, as far from its right edge as from its top and bottom...
    const top = collapseBox!.y - fieldBox!.y;
    const bottom = fieldBox!.y + fieldBox!.height - (collapseBox!.y + collapseBox!.height);
    const right = fieldBox!.x + fieldBox!.width - (collapseBox!.x + collapseBox!.width);
    expect(top).toBeGreaterThan(0);
    expect(Math.abs(top - bottom)).toBeLessThanOrEqual(1);
    expect(Math.abs(right - top)).toBeLessThanOrEqual(1);
    // ...and after the words typed, not over them.
    expect(collapseBox!.x).toBeGreaterThanOrEqual(inputBox!.x + inputBox!.width);
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

  test('keeps every preference field on its fill, its shadow deeper, when pointed at or opened', async ({
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
    // A field's edge is a soft shadow with no ring; the pointer and the focus deepen it, the fill stays.
    const ringOf = (selector: string) =>
      page.locator(selector).first().evaluate((element) => getComputedStyle(element).boxShadow);
    const canvasFill = await painted(
      await page.locator('main#main').evaluate((element) => getComputedStyle(element).backgroundColor),
    );

    for (const selector of [
      '[role="combobox"][aria-label="Default timezone"]',
      'button[aria-label="Date & time format"]',
      '[role="combobox"][aria-label="Open after entering workspace"]',
    ]) {
      const field = page.locator(selector).first();
      await field.scrollIntoViewIfNeeded();
      const rest = await painted(await fillOf(selector));
      const restRing = await ringOf(selector);
      // Never the page canvas: the field would vanish into it.
      expect(rest.join(), `${selector} at rest`).not.toBe(canvasFill.join());
      expect(restRing, `${selector} at rest`).not.toBe('none');
      expect(restRing, `${selector} at rest`).not.toContain('0px 0px 0px 1px');

      await field.hover();
      // The shadow eases to its deeper step; the fill stays where it was.
      await expect.poll(() => ringOf(selector)).not.toBe(restRing);
      expect((await painted(await fillOf(selector))).join(), `${selector} hovered`).toBe(rest.join());

      await field.click();
      await expect(field).toHaveAttribute('aria-expanded', 'true');
      await expect.poll(() => ringOf(selector)).not.toBe(restRing);
      expect((await painted(await fillOf(selector))).join(), `${selector} open`).not.toBe(canvasFill.join());
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
