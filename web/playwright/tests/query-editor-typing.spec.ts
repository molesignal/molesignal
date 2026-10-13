import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { expectExplorerResults, serveExplorerResults } from '../fixtures/mockExplorers';

const front = (page: Page) => page.locator('[data-query-tab-panel][data-active="true"]');

/** Where the caret is against the end of the text on its line, and what that text is. */
async function caret(page: Page) {
  return front(page)
    .locator('[data-query-editor-state="expanded"] .monaco-editor')
    .first()
    .evaluate((editor) => {
      const cursor = editor.querySelector('.cursors-layer .cursor')!.getBoundingClientRect();
      const line = editor.querySelector('.view-lines .view-line')!;
      const range = document.createRange();

      range.selectNodeContents(line);

      return {
        // Monaco writes a space as a no-break space.
        text: (line.textContent ?? '').replace(/\u00a0/g, ' '),
        gap: cursor.left + cursor.width / 2 - Math.max(...Array.from(range.getClientRects()).map((rect) => rect.right)),
        blink: getComputedStyle(editor.querySelector('.cursors-layer')!).animationName,
        glide: getComputedStyle(editor.querySelector('.cursors-layer .cursor')!).transitionDuration,
      };
    });
}

test.describe('Typing in the query editor', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await mountMockRoutes(page, mockServer.port);
    await serveExplorerResults(page);
  });

  // The editor tells the page every keystroke and the page hands the text back
  // a render later. Typed faster than the page renders, that text is older than
  // what the editor holds: taken as the page's word it undid the keystrokes
  // since and threw the caret to the start of the line.
  test('keeps what is typed, and the caret at the end of it, however fast the keys come', async ({ page }) => {
    await page.goto('/logs');
    await expectExplorerResults(page, 'logs');
    await front(page).getByRole('button', { name: 'Expand query' }).click();
    await front(page).locator('[data-query-editor-state="expanded"] .monaco-editor').first().click();

    const typed = "level = 'ERROR' AND status >= 500 AND service.name = 'payment-gateway'";
    await page.keyboard.type(typed, { delay: 4 });

    await expect.poll(async () => (await caret(page)).text).toBe(typed);
    const settled = await caret(page);
    expect(Math.abs(settled.gap)).toBeLessThan(2);
    // The caret is there or it is not, and it is where the text is at once: it neither
    // fades in and out nor glides from place to place.
    expect(settled.blink).not.toMatch(/smooth|phase|expand/);
    expect(settled.glide).toBe('0s');

    // Text that is not Latin keeps the caret at its end too.
    await page.keyboard.type(" AND message contains '上游超时'", { delay: 4 });
    await expect.poll(async () => (await caret(page)).text).toBe(`${typed} AND message contains '上游超时'`);
    expect(Math.abs((await caret(page)).gap)).toBeLessThan(2);
  });

  test('a clause the field list writes into the query still becomes its text', async ({ page }) => {
    await page.goto('/logs');
    await expectExplorerResults(page, 'logs');
    const row = front(page).locator('[data-log-field-row="level"]');

    await row.hover();
    await row.getByRole('button', { name: 'Add level to query' }).click();

    // Not typed in the editor: the page's own text, written in over whatever was there.
    await expect.poll(async () => (await caret(page)).text).toBe("level = ''");
  });
});
