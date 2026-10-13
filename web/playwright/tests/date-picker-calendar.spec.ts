import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('Calendar of the date-time field', () => {
  for (const language of ['zh-cn', 'en-us'] as const) {
    test(`a day is its number, and the day picked is a whole circle at either end of a week (${language})`, async ({
      page,
      mockServer,
    }) => {
      await page.addInitScript((chosen) => {
        localStorage.setItem('molesignal-ui-prefs', JSON.stringify({ state: { language: chosen }, version: 0 }));
      }, language);
      await mountMockRoutes(page, mockServer.port, { preferences: { language } });
      await page.goto('/rum/overview');

      await page.locator('[data-time-range-trigger="true"]').click();
      // The start of a range of one's own: the first date-time field of the panel.
      await page.locator('[data-time-picker-surface="workbench"] label button').first().click();
      const calendar = page.locator('[data-slot="date-time-picker-content"]');
      await expect(calendar).toBeVisible();

      // A language's numeral for the day ("13日") says nothing under the month's name, and seven of
      // them crowd a row.
      const days = calendar.locator('.rdp-day button');
      expect(await days.allTextContents()).toEqual(expect.arrayContaining(['1', '15', '28']));
      expect((await days.allTextContents()).every((text) => /^\d{1,2}$/.test(text))).toBe(true);

      for (const column of [6, 0]) {
        const day = calendar.locator('.rdp-week').nth(1).locator('.rdp-day').nth(column).locator('button');
        await day.click();
        await expect(day).toHaveAttribute('data-selected-single', 'true');

        // Its outer side was squared in the first and the last column, the way a band of days is
        // ended: one day picked there looked cut off by the edge of the panel.
        const corners = await day.evaluate((element) => {
          const style = getComputedStyle(element);
          const { width } = element.getBoundingClientRect();

          return [
            style.borderTopLeftRadius,
            style.borderTopRightRadius,
            style.borderBottomRightRadius,
            style.borderBottomLeftRadius,
          ].map((radius) => Number.parseFloat(radius) >= width / 2);
        });
        expect(corners).toEqual([true, true, true, true]);
      }
    });
  }
});
