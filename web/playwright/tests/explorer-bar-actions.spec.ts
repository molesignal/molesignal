import type { Locator, Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { expectExplorerResults, serveExplorerResults } from '../fixtures/mockExplorers';

/** A screen wide enough that the bar once had room for a word on every button. */
const WIDE = { width: 1728, height: 1000 };

/** The actions of each explorer's bar, in the order they stand in: all of them signs. */
const BAR = {
  logs: ['Run', 'Refresh query', 'Share'],
  traces: ['Query', 'Refresh', 'Share'],
  metrics: ['Run', 'Refresh', 'Add to dashboard', 'Share'],
} as const;

async function openExplorer(page: Page, explorer: keyof typeof BAR) {
  await page.goto(`/${explorer}`);

  if (explorer === 'metrics') await expect(page.locator('[data-query-share]')).toBeVisible();
  else await expectExplorerResults(page, explorer);
}

const sizeOf = async (locator: Locator) => {
  const box = (await locator.boundingBox())!;

  return { width: box.width, height: box.height };
};

/** Every action is a sign without a word, named by what it does, and the box the first one is. */
async function expectSigns(signs: Locator, names: readonly string[]) {
  await expect(signs).toHaveCount(names.length);

  const first = await sizeOf(signs.first());
  for (const [index, name] of names.entries()) {
    const sign = signs.nth(index);

    await expect(sign).toHaveAccessibleName(name);
    // No word, however wide the screen: the row keeps its room for the query.
    await expect(sign).toHaveText('');
    expect(await sizeOf(sign)).toEqual(first);
  }
}

test.describe('Explorer bar actions', () => {
  for (const explorer of ['logs', 'traces', 'metrics'] as const) {
    test(`${explorer}: every action of the bar is a sign without a word, all of one size`, async ({
      page,
      mockServer,
    }) => {
      await page.setViewportSize(WIDE);
      await mountMockRoutes(page, mockServer.port);
      await serveExplorerResults(page);
      await openExplorer(page, explorer);

      const signs = page.locator('[data-query-toolbar] [data-query-icon-action]');
      await expectSigns(signs, BAR[explorer]);

      // The run is the one in the product's colour; the rest are gray.
      const run = page.locator('[data-query-toolbar] [data-query-run]');
      await expect(run).toHaveCount(1);
      await expect(run).toHaveAccessibleName(BAR[explorer][0]);

      // The word is under the pointer.
      await signs.last().hover();
      await expect(page.getByRole('tooltip')).toHaveText('Share');
    });
  }

  for (const language of ['en-us', 'zh-cn'] as const) {
    test(`the row over the results is signs too (${language})`, async ({ page, mockServer }) => {
      const zh = language === 'zh-cn';
      await page.setViewportSize(WIDE);
      await mountMockRoutes(page, mockServer.port, { preferences: { language } });
      await serveExplorerResults(page);

      await openExplorer(page, 'logs');
      const row = page.locator('[data-log-result-actions] [data-query-icon-action]');
      await expectSigns(
        row,
        zh ? ['列设置', '隐藏直方图', '下载日志'] : ['Columns', 'Hide histogram', 'Download logs'],
      );
      // The same box as the signs of the bar above.
      expect(await sizeOf(row.first())).toEqual(
        await sizeOf(page.locator('[data-query-toolbar] [data-query-icon-action]').last()),
      );

      await openExplorer(page, 'traces');
      const histogram = page.locator('[data-module-state]');
      await expect(histogram).toHaveCount(1);
      await expect(histogram).toHaveText('');
      await expect(histogram).toHaveAccessibleName(zh ? '隐藏直方图' : 'Hide histogram');
    });
  }

  test('a menu that closes does not leave its button a tip', async ({ page, mockServer }) => {
    await page.setViewportSize(WIDE);
    await mountMockRoutes(page, mockServer.port);
    await serveExplorerResults(page);
    await openExplorer(page, 'logs');

    const columns = page.locator('[data-log-columns]');
    await columns.click();
    await expect(page.getByRole('menu')).toBeVisible();
    // Closed by the pointer, away from the button: focus goes back to it, and nothing is said.
    await page.mouse.click(400, 600);
    await expect(page.getByRole('menu')).toHaveCount(0);
    await expect(columns).toBeFocused();
    await expect(page.getByRole('tooltip')).toHaveCount(0);

    // Reached by the keyboard, a moment later, it says what it is.
    await page.waitForTimeout(400);
    await page.keyboard.press('Shift+Tab');
    await page.keyboard.press('Tab');
    await expect(columns).toBeFocused();
    await expect(page.getByRole('tooltip')).toHaveText('Columns');
  });
});
