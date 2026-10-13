import type { Locator } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

// Forty folders: the list of dashboards is long enough to scroll, and "Move to folder" holds
// more than a window shows.
const FOLDERS = Array.from({ length: 40 }, (_, index) => ({
  id: `folder-${index + 1}`,
  org_id: 'default',
  name: `Folder ${String(index + 1).padStart(2, '0')}`,
}));

test.beforeEach(async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);
  await page.route('**/api/v1/folders', (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: FOLDERS }) : route.fallback(),
  );
});

/**
 * The distance a menu keeps from the edges of the window: 8px, less a pixel, because its place is
 * rounded to a device pixel and its height is not.
 */
const CLEAR = 8 - 1;

/** A menu grows out of its trigger as it opens: it is measured once it stands at its full size. */
async function opened(menu: Locator) {
  await expect(menu).toBeVisible();
  await menu.evaluate((element) =>
    Promise.all(element.getAnimations().map((animation) => animation.finished)),
  );
}

/** Where a menu stands in the window, and how much of itself it keeps to scroll to. */
const frameOf = (menu: Locator) =>
  menu.evaluate((element) => {
    const rect = element.getBoundingClientRect();

    return {
      top: rect.top,
      bottom: rect.bottom,
      window: window.innerHeight,
      beyond: element.scrollHeight - element.clientHeight,
    };
  });

/** The space between an item and the two edges of the menu it is in. */
async function roomAround(menu: Locator, item: Locator) {
  const [frame, box] = await Promise.all([menu.boundingBox(), item.boundingBox()]);
  if (!frame || !box) throw new Error('not laid out');

  return { above: box.y - frame.y, below: frame.y + frame.height - (box.y + box.height) };
}

test('a menu taller than the room on either side of its button scrolls inside the room', async ({
  page,
}) => {
  // A window too short for a dashboard's eight actions above its button or below it.
  await page.setViewportSize({ width: 1280, height: 400 });
  await page.goto('/dashboards');

  const button = page.getByRole('button', { name: 'More actions for API latency', exact: true });
  await button.evaluate((element) => element.scrollIntoView({ block: 'center' }));
  await button.click();
  const menu = page.getByRole('menu', { name: 'More actions for API latency' });
  await opened(menu);

  // The menu stands clear of the window's edges, and what does not fit is there to scroll to.
  const frame = await frameOf(menu);
  expect(frame.top).toBeGreaterThanOrEqual(CLEAR);
  expect(frame.window - frame.bottom).toBeGreaterThanOrEqual(CLEAR);
  expect(frame.beyond).toBeGreaterThan(0);
  const first = menu.getByRole('menuitem', { name: 'Open' });
  const last = menu.getByRole('menuitem', { name: 'Delete' });
  await expect(last).not.toBeInViewport();

  // The keyboard brings the item it moves to into view, with the menu's padding around it.
  await page.keyboard.press('End');
  await expect(last).toBeFocused();
  await expect(last).toBeInViewport({ ratio: 1 });
  expect((await roomAround(menu, last)).below).toBeGreaterThanOrEqual(3);
  await page.keyboard.press('Home');
  await expect(first).toBeFocused();
  await expect(first).toBeInViewport({ ratio: 1 });
  expect((await roomAround(menu, first)).above).toBeGreaterThanOrEqual(3);
  await expect(last).not.toBeInViewport();

  // The wheel scrolls to the last action, and it takes the click.
  await menu.hover();
  await page.mouse.wheel(0, 2000);
  await expect(last).toBeInViewport({ ratio: 1 });
  expect((await roomAround(menu, last)).below).toBeGreaterThanOrEqual(3);
  await last.click();
  await expect(page.getByRole('dialog', { name: 'Delete “API latency”?' })).toBeVisible();
});

test('a submenu longer than the window scrolls inside it', async ({ page }) => {
  // The window the suite runs in. The forty folders and the one a dashboard starts in make a
  // list of 1238px.
  await page.setViewportSize({ width: 1280, height: 720 });
  await page.goto('/dashboards');

  await page.getByRole('button', { name: 'More actions for API latency', exact: true }).click();
  const move = page
    .getByRole('menu', { name: 'More actions for API latency' })
    .getByRole('menuitem', { name: 'Move to folder' });
  await move.click();
  const folders = page.getByRole('menu', { name: 'Move to folder' });
  await opened(folders);

  // The list takes the height of the window, less the distance it keeps from both edges.
  const frame = await frameOf(folders);
  expect(frame.top).toBeGreaterThanOrEqual(CLEAR);
  expect(frame.window - frame.bottom).toBeGreaterThanOrEqual(CLEAR);
  expect(frame.beyond).toBeGreaterThan(0);
  const first = folders.getByRole('menuitem', { name: 'Folder 01', exact: true });
  const last = folders.getByRole('menuitem', { name: 'Folder 40', exact: true });
  await expect(last).not.toBeInViewport();

  // By keyboard: into the list, to its end and back to its start.
  await page.keyboard.press('ArrowRight');
  await expect(first).toBeFocused();
  await page.keyboard.press('End');
  await expect(last).toBeFocused();
  await expect(last).toBeInViewport({ ratio: 1 });
  expect((await roomAround(folders, last)).below).toBeGreaterThanOrEqual(3);
  await page.keyboard.press('Home');
  await expect(first).toBeFocused();
  await expect(first).toBeInViewport({ ratio: 1 });
  await expect(last).not.toBeInViewport();

  // By hand: straight across from "Move to folder" into the list, the wheel to its end, and the
  // last folder takes the click.
  const [from, into] = await Promise.all([move.boundingBox(), folders.boundingBox()]);
  if (!from || !into) throw new Error('not laid out');
  await page.mouse.move(into.x + into.width / 2, from.y + from.height / 2, { steps: 12 });
  await page.mouse.wheel(0, 4000);
  await expect(last).toBeInViewport({ ratio: 1 });
  expect((await roomAround(folders, last)).below).toBeGreaterThanOrEqual(3);

  const saved = page.waitForRequest(
    (request) => request.method() === 'PUT' && request.url().includes('/api/v1/dashboards/'),
  );
  await last.click();
  expect((await saved).postDataJSON()).toMatchObject({ folder_id: 'folder-40' });
  await expect(page.getByText('Dashboard moved: API latency')).toBeVisible();
});
