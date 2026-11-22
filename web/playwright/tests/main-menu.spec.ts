import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.beforeEach(async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
});

const trigger = (page: Page) => page.getByTestId('main-menu-trigger');
const menu = (page: Page) => page.getByTestId('main-menu');
const recent = (page: Page) =>
  menu(page).getByRole('region', { name: 'Recent' }).locator('[data-menu-entry]').evaluateAll((nodes) =>
    nodes.map((node) => node.getAttribute('data-menu-entry')),
  );
const savedMenu = (page: Page) =>
  page.waitForRequest((request) => request.method() === 'PUT' && request.url().includes('/me/ui-preference/navigation.menu/menu'));

test('drops every area from under the top bar, by group, with the current one marked', async ({ page }) => {
  await page.goto('/metrics');
  await expect(page.getByTestId('primary-sidebar')).toHaveCount(0);
  await trigger(page).click();

  await expect(menu(page)).toBeVisible();
  await expect(trigger(page)).toHaveAttribute('aria-expanded', 'true');
  await expect(trigger(page)).toHaveAccessibleName('Close menu');
  // Once it has slid down, the menu starts where the top bar ends.
  const bar = (await page.getByRole('banner').boundingBox())!;
  await expect.poll(async () => Math.round((await menu(page).boundingBox())!.y)).toBe(Math.round(bar.y + bar.height));
  for (const group of ['Observe', 'Reliability', 'Data', 'AI ops', 'Collaboration', 'Admin']) {
    await expect(menu(page).getByRole('heading', { name: group, exact: true })).toBeVisible();
  }
  await expect(menu(page).getByRole('link', { name: 'Home', exact: true })).toBeVisible();
  const current = menu(page).locator('[data-menu-entry="metrics"]');
  await expect(current).toHaveAttribute('aria-current', 'page');
  await expect(current).toBeFocused();
});

test('opens under a resting mouse, stays while the mouse is over it, and closes once it leaves', async ({ page }) => {
  await page.goto('/metrics');
  await expect(trigger(page)).toBeVisible();

  await trigger(page).hover();
  await expect(menu(page)).toBeVisible();
  await expect(trigger(page)).toHaveAttribute('aria-expanded', 'true');

  // Down from the button into the menu: it stays.
  const entry = menu(page).locator('[data-menu-entry="logs"]');
  await entry.hover();
  await page.waitForTimeout(400);
  await expect(menu(page)).toBeVisible();

  // Out to the page below it: it closes.
  const box = (await menu(page).boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height + 120);
  await expect(menu(page)).toHaveCount(0);
});

test('a click on the button pins a menu the mouse opened, and a second click closes it', async ({ page }) => {
  await page.goto('/metrics');
  await trigger(page).hover();
  await expect(menu(page)).toBeVisible();

  await trigger(page).click();
  const box = (await menu(page).boundingBox())!;
  await page.mouse.move(box.x + box.width / 2, box.y + box.height + 120);
  await page.waitForTimeout(400);
  await expect(menu(page)).toBeVisible();

  await trigger(page).click();
  await expect(menu(page)).toHaveCount(0);
});

test('closes on Escape, on a press outside it and on a choice', async ({ page }) => {
  await page.goto('/metrics');

  await trigger(page).click();
  await page.keyboard.press('Escape');
  await expect(menu(page)).toHaveCount(0);
  await expect(trigger(page)).toBeFocused();
  await expect(trigger(page)).toHaveAttribute('aria-expanded', 'false');

  await trigger(page).click();
  await page.mouse.click(700, 760);
  await expect(menu(page)).toHaveCount(0);

  await trigger(page).click();
  await page.getByTestId('command-palette-trigger').click();
  await expect(menu(page)).toHaveCount(0);
  await page.keyboard.press('Escape');

  await trigger(page).click();
  await menu(page).getByRole('link', { name: 'Logs', exact: true }).click();
  await expect(page).toHaveURL(/\/logs/);
  await expect(menu(page)).toHaveCount(0);
});

test('keeps the areas opened lately, the latest first, without the one open now', async ({ page }) => {
  await page.goto('/metrics');
  for (const area of ['Logs', 'Traces']) {
    await trigger(page).click();
    await menu(page).getByRole('link', { name: area, exact: true }).click();
    await expect(menu(page)).toHaveCount(0);
  }
  const saved = savedMenu(page);
  await trigger(page).click();
  expect(await recent(page)).toEqual(['logs', 'metrics']);

  // Kept on the server: another load, or another browser, finds the same list.
  expect((await saved).postDataJSON()).toMatchObject({ value: { recent: ['traces', 'logs', 'metrics'] } });
  await page.reload();
  await trigger(page).click();
  expect(await recent(page)).toEqual(['logs', 'metrics']);
});

test('opens an area in a new tab by its arrow or a modified click, and always when asked', async ({ page, context }) => {
  await page.goto('/metrics');
  await trigger(page).click();

  const row = menu(page).locator('[data-menu-entry="logs"]');
  const arrow = menu(page).locator('[data-menu-new-tab="logs"]');
  await row.hover();
  await expect(arrow).toHaveCSS('opacity', '1');
  await expect(arrow).toHaveAttribute('target', '_blank');
  await expect(arrow).toHaveAccessibleName('Open Logs in a new tab');
  const byArrow = context.waitForEvent('page');
  await arrow.click();
  await expect(await byArrow).toHaveURL(/\/logs/);
  await expect(page).toHaveURL(/\/metrics/);

  await trigger(page).click();
  const byModifier = context.waitForEvent('page');
  await menu(page).locator('[data-menu-entry="traces"]').click({ modifiers: ['ControlOrMeta'] });
  await expect(await byModifier).toHaveURL(/\/traces/);
  await expect(page).toHaveURL(/\/metrics/);

  if (await menu(page).count() === 0) await trigger(page).click();
  const saved = savedMenu(page);
  await menu(page).getByRole('switch', { name: 'Always open in a new tab' }).click();
  await expect(menu(page).locator('[data-menu-entry="logs"]')).toHaveAttribute('target', '_blank');
  await expect(menu(page).locator('[data-menu-new-tab]')).toHaveCount(0);
  expect((await saved).postDataJSON()).toMatchObject({ value: { newTab: true } });

  await page.reload();
  await trigger(page).click();
  await expect(menu(page).getByRole('switch', { name: 'Always open in a new tab' })).toBeChecked();
  await expect(menu(page).locator('[data-menu-entry="logs"]')).toHaveAttribute('target', '_blank');
});
