import type { Locator } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

async function centreY(locator: Locator): Promise<number> {
  const box = await locator.boundingBox();
  expect(box).not.toBeNull();
  return box!.y + box!.height / 2;
}

test.describe('Primary sidebar active indicator', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('sits beside the active row and glides to the next one', async ({ page }) => {
    await page.goto('/metrics');
    const indicator = page.getByTestId('sidebar-active-indicator');
    const metrics = page.getByRole('link', { name: 'Metrics', exact: true });

    await expect(indicator).toHaveCSS('opacity', '1');
    await expect
      .poll(async () => Math.abs((await centreY(indicator)) - (await centreY(metrics))))
      .toBeLessThanOrEqual(1);

    await page.getByRole('link', { name: 'Logs', exact: true }).click();
    await expect(page).toHaveURL(/\/logs/);
    const logs = page.getByRole('link', { name: 'Logs', exact: true });
    await expect
      .poll(async () => Math.abs((await centreY(indicator)) - (await centreY(logs))))
      .toBeLessThanOrEqual(1);
  });

  test('keeps the glide on the rail edge and follows the rail when it expands', async ({
    page,
  }) => {
    await page.goto('/traces');
    const indicator = page.getByTestId('sidebar-active-indicator');
    const traces = page.getByRole('link', { name: 'Traces', exact: true });

    // The bar hugs the rail's left edge...
    expect((await indicator.boundingBox())!.x).toBeLessThanOrEqual(1);

    // ...and group labels appearing when the rail expands push the rows down:
    // the indicator moves with its row instead of staying where it was.
    await page.getByTestId('sidebar-toggle').click();
    await expect(page.getByTestId('primary-sidebar')).toHaveCSS('width', '220px');
    await expect
      .poll(async () => Math.abs((await centreY(indicator)) - (await centreY(traces))))
      .toBeLessThanOrEqual(1);
  });

  test('stays on the Settings entry while any Settings page is open', async ({ page }) => {
    await page.goto('/settings/audit');
    const settings = page.getByRole('link', { name: 'Settings', exact: true });
    const indicator = page.getByTestId('sidebar-active-indicator');

    // Audit is not the page the Settings entry opens, but it is inside Settings.
    await expect(settings).toHaveAttribute('aria-current', 'page');
    await expect
      .poll(async () => Math.abs((await centreY(indicator)) - (await centreY(settings))))
      .toBeLessThanOrEqual(1);

    await page.goto('/iam/roles');
    const access = page.getByRole('link', { name: 'Identity & Access', exact: true });
    await expect(access).toHaveAttribute('aria-current', 'page');
    await expect(settings).not.toHaveAttribute('aria-current', 'page');
  });

  test('sets the expanded sidebar in blocks and names the one you are in', async ({ page }) => {
    await page.goto('/metrics');
    await page.getByTestId('sidebar-toggle').click();
    const sidebar = page.getByTestId('primary-sidebar');
    await expect(sidebar).toHaveCSS('width', '220px');

    // Labels ease between palettes when the saved theme lands just after first
    // paint, and computed colours come back in different colour spaces. So: paint
    // each colour to compare it, and poll until it matches the token it should be.
    const painted = (value: string) =>
      page.evaluate((colourValue) => {
        const canvas = document.createElement('canvas');
        canvas.width = 1;
        canvas.height = 1;
        const context = canvas.getContext('2d')!;
        context.fillStyle = colourValue;
        context.fillRect(0, 0, 1, 1);
        return Array.from(context.getImageData(0, 0, 1, 1).data).join();
      }, value);
    const token = (variable: string) =>
      page.evaluate((name) => {
        const probe = document.createElement('span');
        probe.style.color = `var(${name})`;
        document.body.append(probe);
        const value = getComputedStyle(probe).color;
        probe.remove();
        return value;
      }, variable);
    const labelIs = async (name: string, variable: string) =>
      (await painted(
        await sidebar.getByText(name, { exact: true }).evaluate((el) => getComputedStyle(el).color),
      )) === (await painted(await token(variable)));

    // Metrics is in OBSERVE: that label is a tier darker than the other blocks'.
    await expect.poll(() => labelIs('Observe', '--tx-0')).toBe(true);
    await expect.poll(() => labelIs('Reliability', '--tx-2')).toBe(true);
    await expect.poll(() => labelIs('Data', '--tx-2')).toBe(true);

    // Rows are regular weight; only the current one is bold.
    const weight = (name: string) =>
      sidebar
        .getByRole('link', { name, exact: true })
        .locator('span')
        .first()
        .evaluate((el) => getComputedStyle(el).fontWeight);
    expect(await weight('Logs')).toBe('500');
    expect(await weight('Metrics')).toBe('600');
  });

  test('lights only the active icon: the rest of the rail stays neutral', async ({ page }) => {
    await page.goto('/metrics');
    // Computed colours come back in different colour spaces (rgb, oklab, srgb);
    // draw them onto a canvas to compare what is actually painted.
    const painted = (colour: string) =>
      page.evaluate((value) => {
        const canvas = document.createElement('canvas');
        canvas.width = 1;
        canvas.height = 1;
        const context = canvas.getContext('2d')!;
        context.fillStyle = value;
        context.fillRect(0, 0, 1, 1);
        return Array.from(context.getImageData(0, 0, 1, 1).data);
      }, colour);
    const iconColour = (name: string) =>
      page
        .getByRole('link', { name, exact: true })
        .locator('svg')
        .first()
        .evaluate((icon) => getComputedStyle(icon).color);
    const token = (name: string) =>
      page.evaluate((variable) => {
        const probe = document.createElement('span');
        probe.style.color = `var(${variable})`;
        document.body.append(probe);
        const colour = getComputedStyle(probe).color;
        probe.remove();
        return colour;
      }, name);

    // Poll, and read icon and token together: the app applies the saved theme a
    // moment after first paint, and the icons ease between the two palettes.
    await expect
      .poll(async () => {
        const [icon, expected] = [
          await painted(await iconColour('Logs')),
          await painted(await token('--tx-2')),
        ];
        return icon.join() === expected.join();
      })
      .toBe(true);
    await expect
      .poll(async () => {
        const [icon, expected] = [
          await painted(await iconColour('Metrics')),
          await painted(await token('--indigo-soft')),
        ];
        return icon.join() === expected.join();
      })
      .toBe(true);
  });
});
