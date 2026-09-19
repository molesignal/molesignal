import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.beforeEach(async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port, { theme: 'light' });
});

test('renders the Trace sorter as a lightweight toolbar menu', async ({
  page,
}) => {
  await page.goto('/traces');

  const trigger = page.getByRole('combobox', { name: 'Sort traces' });
  await expect(trigger).toHaveCSS('height', '28px');
  await expect(trigger).toHaveCSS('border-top-width', '0px');
  await trigger.hover();
  await expect(trigger).not.toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  await trigger.click();

  const menu = page.getByRole('listbox');
  const selected = page.getByRole('option', { name: 'Newest first' });
  await expect(menu).toBeVisible();
  await expect(menu).toHaveCSS('border-top-width', '0px');
  await expect(menu).toHaveCSS('border-radius', '8px');
  await expect(menu).toHaveCSS('box-shadow', /4px 12px/);
  await expect(selected).toHaveAttribute('data-state', 'checked');
  await expect(selected).toHaveCSS('height', '30px');
  await expect(selected).toHaveCSS('border-radius', '6px');
  await expect(selected).toHaveCSS('font-size', '12px');
  await expect(selected).toHaveCSS('font-weight', '600');
  expect(
    await selected.evaluate((element) => getComputedStyle(element).color),
  ).not.toBe(
    await page
      .getByRole('option', { name: 'Oldest first' })
      .evaluate((element) => getComputedStyle(element).color),
  );

  await page.getByRole('option', { name: 'Oldest first' }).click();
  await expect(trigger).toContainText('Oldest first');
});

test('uses the same lightweight surface for action dropdowns', async ({
  page,
}) => {
  await page.goto('/logs');
  await page.getByRole('button', { name: 'Density' }).click();

  const menu = page.getByRole('menu');
  const selected = page.getByRole('menuitemradio', { name: 'Compact' });
  await expect(menu).toBeVisible();
  await expect(menu).toHaveCSS('border-top-width', '0px');
  await expect(menu).toHaveCSS('border-radius', '8px');
  await expect(menu).toHaveCSS('box-shadow', /4px 12px/);
  await expect(selected).toHaveAttribute('data-state', 'checked');
  await expect(selected).toHaveCSS('min-height', '30px');
  await expect(selected).toHaveCSS('font-size', '12px');
  await expect(selected).toHaveCSS('font-weight', '600');
  expect(
    await selected.evaluate((element) => getComputedStyle(element).color),
  ).not.toBe(
    await page
      .getByRole('menuitemradio', { name: 'Standard' })
      .evaluate((element) => getComputedStyle(element).color),
  );
});

test('renders the global time range control as a lightweight workbench popover', async ({
  page,
}) => {
  await page.goto('/traces');

  const trigger = page.locator('[data-time-range-trigger="true"]');
  expect((await trigger.boundingBox())?.height).toBeGreaterThanOrEqual(26);
  await expect(trigger).toHaveCSS('border-top-width', '0px');
  await trigger.click();

  const surface = page.locator('[data-time-picker-surface="workbench"]');
  await expect(surface).toBeVisible();
  const surfaceBox = await surface.boundingBox();
  expect(surfaceBox?.width).toBeGreaterThanOrEqual(500);
  expect(surfaceBox?.width).toBeLessThanOrEqual(600);
  expect(surfaceBox?.height).toBeLessThan(320);
  await expect(surface).toHaveCSS('border-top-width', '0px');
  await expect(surface).toHaveCSS('border-radius', '8px');
  await expect(surface).toHaveCSS('box-shadow', /4px 12px/);
  await expect(surface.getByText('Select time range', { exact: true })).toBeVisible();
  const selectedRange = surface.getByRole('button', {
    name: 'Last 24 hours',
    exact: true,
  });
  const otherRange = surface.getByRole('button', {
    name: 'Last 1 hour',
    exact: true,
  });
  await expect(selectedRange).toHaveAttribute('aria-pressed', 'true');
  expect((await selectedRange.boundingBox())?.height).toBeGreaterThanOrEqual(26);
  expect(
    await selectedRange.evaluate((element) => getComputedStyle(element).backgroundColor),
  ).not.toBe(
    await otherRange.evaluate((element) => getComputedStyle(element).backgroundColor),
  );

  const absoluteInputs = surface.getByRole('button', {
    name: /Open date and time picker/,
  });
  await expect(absoluteInputs).toHaveCount(2);
  expect((await absoluteInputs.first().boundingBox())?.height).toBeGreaterThanOrEqual(26);
  await expect(absoluteInputs.first()).toHaveCSS('border-top-width', '0px');

  const fromInput = absoluteInputs.first();
  const fromInputBox = await fromInput.boundingBox();
  await fromInput.click();
  const dateTimeSurface = page.locator('[data-slot="date-time-picker-content"]');
  await expect(dateTimeSurface).toBeVisible();
  const dateTimeSurfaceBox = await dateTimeSurface.boundingBox();
  expect(Math.abs((dateTimeSurfaceBox?.x ?? 0) - (fromInputBox?.x ?? 0))).toBeLessThan(2);
  expect(
    Math.abs((dateTimeSurfaceBox?.width ?? 0) - (fromInputBox?.width ?? 0)),
  ).toBeLessThan(2);
  await expect(dateTimeSurface).toHaveCSS('border-top-width', '0px');
  await expect(
    dateTimeSurface.locator('[data-date-time-picker-section="time"]'),
  ).toHaveCSS('border-top-width', '0px');
  await expect(
    dateTimeSurface.locator('[data-date-time-picker-section="actions"]'),
  ).toHaveCSS('border-top-width', '0px');
  for (const label of ['Hour', 'Minute', 'Second']) {
    await expect(
      dateTimeSurface.getByRole('combobox', { name: label }),
    ).toHaveCSS('border-top-width', '0px');
  }
  const readValueTypography = (locator: typeof fromInput) =>
    locator.evaluate((element) => {
      const style = getComputedStyle(element);
      return {
        fontFamily: style.fontFamily,
        fontSize: style.fontSize,
        fontWeight: style.fontWeight,
      };
    });
  const fromTypography = await readValueTypography(fromInput);
  expect(
    await readValueTypography(
      dateTimeSurface.locator('[data-selected-single="true"]'),
    ),
  ).toEqual(fromTypography);
  expect(
    await readValueTypography(
      dateTimeSurface.getByRole('combobox', { name: 'Hour' }),
    ),
  ).toEqual(fromTypography);
  await page.keyboard.press('Escape');
  await expect(dateTimeSurface).toBeHidden();

  await otherRange.click();
  await expect(surface).toBeHidden();
  await expect(trigger).toContainText('Last 1 hour');

  await page.keyboard.press('Meta+Alt+E');
  const globalSurface = page.locator(
    '[role="dialog"][data-time-picker-surface="workbench"]',
  );
  await expect(globalSurface).toBeVisible();
  const globalSurfaceBox = await globalSurface.boundingBox();
  expect(globalSurfaceBox?.width).toBeGreaterThanOrEqual(500);
  expect(globalSurfaceBox?.width).toBeLessThanOrEqual(600);
  expect(globalSurfaceBox?.height).toBeLessThan(320);
  await expect(globalSurface).toHaveCSS('border-top-width', '0px');
  await expect(globalSurface).toHaveCSS('box-shadow', /4px 12px/);
});
