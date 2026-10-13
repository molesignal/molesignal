import { expect, type Page } from '@playwright/test';

import { mountMockRoutes, test } from '../fixtures/mockBackend';

// A head row (a title, the line under it, its actions) has the same room over it as under it.
// With more on one side, the title looks pushed against an edge, and a button in the row shows
// it at once: it is a box, and a box off the middle of its band reads as a mistake.

interface Room {
  above: number;
  below: number;
}

/**
 * The room over a head and under it, between boxes: from the top edge of what frames it down to
 * the head, and from the head down to the first thing under it.
 */
async function roomAround(page: Page, head: string, frame: string, under: string): Promise<Room> {
  await expect(page.locator(head).first()).toBeVisible();
  await expect(page.locator(under).first()).toBeVisible();

  return page.evaluate(
    ({ head, frame, under }) => {
      const headBox = document.querySelector(head)!.getBoundingClientRect();
      const frameBox = document.querySelector(frame)!.getBoundingClientRect();
      const underBox = document.querySelector(under)!.getBoundingClientRect();

      return { above: headBox.top - frameBox.top, below: underBox.top - headBox.bottom };
    },
    { head, frame, under },
  );
}

/** How far the middle of a part of a row is from the middle of the row. */
async function offCentre(page: Page, row: string, part: string): Promise<number> {
  const [rowBox, partBox] = await Promise.all([
    page.locator(row).first().boundingBox(),
    page.locator(part).first().boundingBox(),
  ]);

  return Math.abs(rowBox!.y + rowBox!.height / 2 - (partBox!.y + partBox!.height / 2));
}

test.beforeEach(async ({ page, mockServer }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await mountMockRoutes(page, mockServer.port, { theme: 'light' });
});

test.describe('the head of a list under Access', () => {
  const head = '[data-iam-list-page] [data-page-title-row]';

  for (const route of ['/iam/users', '/iam/roles', '/iam/sso']) {
    test(`${route} has as much room over it as under it`, async ({ page }) => {
      await page.goto(route);

      const room = await roomAround(page, head, '[data-iam-list-page]', '[data-iam-list-body] > *');

      expect(Math.abs(room.above - room.below)).toBeLessThanOrEqual(0.5);
      // Close to its list, not floating over it.
      expect(room.above).toBeGreaterThanOrEqual(6);
      expect(room.above).toBeLessThanOrEqual(12);
    });
  }

  test('its action stands at the middle of the head', async ({ page }) => {
    await page.goto('/iam/users');
    await expect(page.locator(`${head} button`).first()).toBeVisible();

    expect(await offCentre(page, head, `${head} button`)).toBeLessThanOrEqual(0.5);
  });
});

test.describe('the head of a Settings page', () => {
  const head = '[data-management-content] [data-page-title-row]';

  for (const route of ['/settings/regex_patterns', '/settings/notify/policies']) {
    test(`${route} has as much room over it as under it`, async ({ page }) => {
      await page.goto(route);

      const room = await roomAround(
        page,
        head,
        '[data-management-content]',
        "[data-settings-body='content'] > *",
      );

      expect(Math.abs(room.above - room.below)).toBeLessThanOrEqual(0.5);
    });
  }

  test('a page that opens on a section title gives the head the same room, by the titles', async ({
    page,
  }) => {
    await page.goto('/settings/general');
    await expect(page.locator("[data-settings-body='sections']")).toBeVisible();

    // The section title brings room of its own over its text: the two are compared where the
    // text is, not where the boxes are.
    const text = await page.evaluate((head) => {
      const lineOf = (element: Element) => {
        const range = document.createRange();
        range.selectNodeContents(element);
        const rect = range.getBoundingClientRect();
        const size = Number.parseFloat(getComputedStyle(element).fontSize);
        const middle = (rect.top + rect.bottom) / 2;
        return { top: middle - size / 2, bottom: middle + size / 2 };
      };

      const frame = document.querySelector('[data-management-content]')!.getBoundingClientRect();
      const row = document.querySelector(head)!;
      const title = lineOf(row.querySelector('[data-page-title-heading]')!);
      const line = lineOf(row.querySelector('[data-page-title-meta]')!);
      const section = lineOf(document.querySelector('[data-settings-section-title]')!);

      return { above: title.top - frame.top, below: section.top - line.bottom };
    }, head);

    expect(Math.abs(text.above - text.below)).toBeLessThanOrEqual(2.5);
  });
});

test.describe('the head of a list card in User Experience', () => {
  const head = '[data-rum-section-head]';

  // The Pages list stands in the same card; the mock backend has no pages to fill it with.
  for (const route of ['/rum/applications']) {
    test(`${route} has as much room over it as under it`, async ({ page }) => {
      await page.goto(route);
      await expect(page.locator(head).first()).toBeVisible();

      const room = await page.evaluate((selector) => {
        const row = document.querySelector(selector)!;
        const rowBox = row.getBoundingClientRect();
        const card = row.parentElement!.getBoundingClientRect();
        const under = row.nextElementSibling!.getBoundingClientRect();
        return { above: rowBox.top - card.top, below: under.top - rowBox.bottom };
      }, head);

      expect(Math.abs(room.above - room.below)).toBeLessThanOrEqual(0.5);
    });
  }

  test('its filters stand at the middle of the head, not on the table under it', async ({ page }) => {
    await page.goto('/rum/applications');
    await expect(page.locator(`${head} [data-rum-filter]`).first()).toBeVisible();

    expect(await offCentre(page, head, `${head} [data-rum-filter]`)).toBeLessThanOrEqual(0.5);
  });
});
