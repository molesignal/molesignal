import type { Locator, Page } from '@playwright/test';

import { FROZEN_NOW_ISO, expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { serveExplorerResults } from '../fixtures/mockExplorers';

/** The width the field had while every list got the same; now the most it takes. */
const WIDEST = 220;
const NOW = Date.parse(FROZEN_NOW_ISO) * 1_000;

const logStream = (name: string) => ({
  id: name,
  name,
  stream_type: 'logs',
  schema: {
    fields: [
      { name: '_timestamp', data_type: 'timestamp', nullable: false, indexed: true },
      { name: 'level', data_type: 'utf8', nullable: true, indexed: false },
    ],
  },
  retention: { days: 7 },
  effective_retention: { days: 7 },
  created_at_micros: NOW,
  updated_at_micros: NOW,
});

/** Opens Logs on a list of streams with these names and gives the stream field. */
async function openLogs(page: Page, names: string[]): Promise<Locator> {
  await serveExplorerResults(page);
  await page.route(/\/api\/v1\/streams(\?.*)?$/, (route) =>
    route.request().method() === 'GET' ? route.fulfill({ json: names.map(logStream) }) : route.fallback(),
  );
  await page.goto('/logs');

  const field = page.locator('[data-log-stream-select]');
  await expect(field).toBeVisible();
  // The product face is in: the widths below are those of the names as they are read.
  await page.evaluate(() => document.fonts.ready);

  return field;
}

/** The width an element is laid out at: the list grows into place as it opens, which its box on screen follows. */
const widthOf = (locator: Locator) =>
  locator.evaluate((element) => Number.parseFloat(getComputedStyle(element).width));

/** Whether a name is drawn short of its end. */
const isCut = (name: Locator) => name.evaluate((element) => element.scrollWidth > element.clientWidth);

/** The room between the end of the name the field shows and its arrow. */
const roomAfterName = (field: Locator) =>
  field.evaluate((element) => {
    const name = element.querySelector('[data-log-stream-name] [data-log-stream-name]')!;
    const range = document.createRange();
    range.selectNodeContents(name);

    return element.querySelector('svg')!.getBoundingClientRect().left - range.getBoundingClientRect().right;
  });

test.describe('Logs stream select', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await mountMockRoutes(page, mockServer.port);
  });

  test('is as wide as the longest name of its list, whichever is chosen, and the list as wide as it', async ({
    page,
  }) => {
    const names = ['application_logs', 'app', 'nginx'];
    const field = await openLogs(page, names);
    const list = page.locator('[data-log-stream-list]');

    const width = await widthOf(field);
    // Short names leave their room to the query beside the field.
    expect(width).toBeLessThan(WIDEST - 40);

    await field.click();
    await expect(list).toBeVisible();
    // The list opens as wide as the field, and no name in it is cut.
    expect(Math.abs((await widthOf(list)) - width)).toBeLessThan(1);
    for (const name of names) {
      expect(await isCut(list.getByRole('option', { name, exact: true }).locator('[data-log-stream-name]'))).toBe(false);
    }
    await page.keyboard.press('Escape');

    const rooms: number[] = [];
    for (const name of names) {
      await field.click();
      await list.getByRole('option', { name, exact: true }).click();
      await expect(field).toHaveText(name);
      // The field keeps its width: what follows it does not move when the stream changes.
      expect(Math.abs((await widthOf(field)) - width)).toBeLessThan(0.5);
      rooms.push(await roomAfterName(field));
    }

    // It is the longest name the field is cut to: only the gap before the arrow follows it.
    expect(rooms[0]).toBeGreaterThan(12);
    expect(rooms[0]).toBeLessThan(24);
    expect(rooms[1]).toBeGreaterThan(rooms[0]! + 50);
  });

  test('stops at the width it had, cuts a longer name and says it in full under the pointer', async ({ page }) => {
    const long = 'kubernetes_ingress_controller_access_logs_production_eu_west_1';
    const field = await openLogs(page, ['application_logs', 'app', long]);
    const list = page.locator('[data-log-stream-list]');

    expect(await widthOf(field)).toBe(WIDEST);

    await field.click();
    await expect(list).toBeVisible();
    expect(await widthOf(list)).toBe(WIDEST);

    const row = list.getByRole('option', { name: long, exact: true });
    const rowName = row.locator('[data-log-stream-name]');
    expect(await isCut(rowName)).toBe(true);
    expect(await rowName.evaluate((element) => getComputedStyle(element).textOverflow)).toBe('ellipsis');
    // Only a name that is cut is said again.
    await row.hover();
    await expect(row).toHaveAttribute('title', long);
    const short = list.getByRole('option', { name: 'app', exact: true });
    await short.hover();
    expect(await short.getAttribute('title')).toBeNull();

    await row.click();
    await expect(field).toHaveText(long);
    expect(await widthOf(field)).toBe(WIDEST);
    await page.mouse.move(0, 0);
    await field.hover();
    await expect(field).toHaveAttribute('title', long);
  });
});
