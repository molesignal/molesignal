import type { Locator, Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

/** The code editor whose text is named `name`. */
const jsonEditor = (page: Page, name: string) =>
  page.locator('.code-editor-shell').filter({ has: page.getByRole('textbox', { name, exact: true }) });

/** The lines the editor shows, top to bottom. Monaco writes a space as a no-break space. */
function shownText(editor: Locator): Promise<string> {
  return editor.locator('.view-lines').evaluate((lines) =>
    Array.from(lines.querySelectorAll<HTMLElement>('.view-line'))
      .sort((a, b) => Number.parseFloat(a.style.top) - Number.parseFloat(b.style.top))
      .map((line) => (line.textContent ?? '').replace(/\u00a0/g, ' '))
      .join('\n'),
  );
}

/**
 * Empties the focused editor. Monaco takes its keys from the platform the user agent names, not
 * the machine the browser runs on: the project's Chrome says Windows, where select-all is
 * Control+A, while `ControlOrMeta` presses Meta on a Mac.
 */
async function clear(page: Page, editor: Locator): Promise<void> {
  const mac = await page.evaluate(() => navigator.userAgent.includes('Macintosh'));
  await expect(editor.locator('.monaco-editor')).toHaveClass(/\bfocused\b/);
  await page.keyboard.press(mac ? 'Meta+a' : 'Control+a');
  await page.keyboard.press('Backspace');
  await expect.poll(() => shownText(editor)).toBe('');
}

/** What pasting `text` from the clipboard hands the editor. */
function paste(editor: Locator, text: string): Promise<void> {
  return editor.locator('.native-edit-context').evaluate((input, value) => {
    const data = new DataTransfer();
    data.setData('text/plain', value);
    input.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
  }, text);
}

/** The colour each token of the editor is drawn in, by its text. */
function tokenColours(editor: Locator): Promise<Record<string, string>> {
  return editor.locator('.view-lines').evaluate((lines) =>
    Object.fromEntries(
      Array.from(lines.querySelectorAll<HTMLElement>('.view-line span span'), (token) => [
        (token.textContent ?? '').trim(),
        getComputedStyle(token).color,
      ]),
    ),
  );
}

test.describe('JSON in the code editor', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test("a JSON field's own words focus its editor, as they would a text area", async ({ page }) => {
    await page.goto('/settings/notify/policies');
    await page.getByRole('button', { name: 'New policy' }).first().click();
    const drawer = page.getByRole('dialog');
    await expect(jsonEditor(page, 'Matchers (JSON)').locator('.view-line').first()).toBeVisible();
    await drawer.getByText('Matchers (JSON)', { exact: true }).click();
    await expect(page.getByRole('textbox', { name: 'Matchers (JSON)', exact: true })).toBeFocused();
  });

  test("a template's preview attributes: laid out, a guide down every indent and no line numbers", async ({ page }) => {
    await page.goto('/settings/notify/templates');
    await page.getByRole('button', { name: 'New template' }).click();
    await page.getByRole('button', { name: 'Preview event attributes (JSON)', exact: true }).click();
    const editor = jsonEditor(page, 'Preview event attributes (JSON)');
    await expect(editor.locator('.view-line').first()).toBeVisible();

    // One key a line with two-space indents. Only the top is compared: a long value wraps on
    // screen, and lines scrolled out of the editor are not drawn.
    const shown = (await shownText(editor)).split('\n');
    expect(shown.slice(0, 3)).toEqual(['{', '  "alert_id": "incident-preview",', '  "rule_id": "rule-preview",']);
    await expect(editor.locator('.core-guide-indent').first()).toBeAttached();
    await expect(editor.locator('.line-numbers')).toHaveCount(0);
  });

  test('attributes typed, pasted and left are laid out with two-space indents', async ({ page }) => {
    await page.goto('/settings/notify/templates');
    await page.getByRole('button', { name: 'New template' }).click();
    const drawer = page.getByRole('dialog', { name: 'New notify template' });
    await drawer.getByRole('button', { name: 'Preview event attributes (JSON)', exact: true }).click();
    const editor = jsonEditor(page, 'Preview event attributes (JSON)');
    await expect(editor.locator('.view-line').first()).toBeVisible();

    await editor.locator('.view-lines').click();
    await expect(page.getByRole('textbox', { name: 'Preview event attributes (JSON)', exact: true })).toBeFocused();

    // While it is typed the text stays as it is; left, it is laid out.
    await clear(page, editor);
    await page.keyboard.insertText('{"severity":"critical","labels":{"service":"api"}}');
    await expect.poll(() => shownText(editor)).toBe('{"severity":"critical","labels":{"service":"api"}}');
    await drawer.getByRole('heading', { name: 'New notify template' }).click();
    await expect
      .poll(() => shownText(editor))
      .toBe('{\n  "severity": "critical",\n  "labels": {\n    "service": "api"\n  }\n}');

    // With the caret inside a block, its indent guide is the darker one, and nothing is drawn
    // from bracket to bracket.
    await editor.locator('.view-line', { hasText: '"service": "api"' }).click();
    await expect(editor.locator('.core-guide-indent.indent-active').first()).toBeAttached();
    await expect(editor.locator('.core-guide:not(.core-guide-indent)')).toHaveCount(0);

    // Pasted, it is laid out at once, every digit of a long number kept.
    await editor.locator('.view-lines').click();
    await clear(page, editor);
    await paste(editor, '{"service":"api","value":12345678901234567890,"tags":["edge"],"muted":false,"owner":null}');
    await expect
      .poll(() => shownText(editor))
      .toBe(
        '{\n  "service": "api",\n  "value": 12345678901234567890,\n  "tags": [\n    "edge"\n  ],\n  "muted": false,\n  "owner": null\n}',
      );

    // A key, a string, a number, a literal and a bracket each in a colour of its own. The editor
    // colours a line when it has a moment to spare, so the tokens are waited for.
    const tokens = ['"service"', '"api"', '12345678901234567890', 'false', '{'];
    await expect.poll(async () => Object.keys(await tokenColours(editor))).toEqual(expect.arrayContaining(tokens));
    const colours = await tokenColours(editor);
    expect(new Set(tokens.map((token) => colours[token])).size).toBe(tokens.length);

    // Enter indents the next line, and a bracket or a quote is closed as it is opened.
    await clear(page, editor);
    await page.keyboard.type('{');
    await page.keyboard.press('Enter');
    await page.keyboard.type('"service": "api"');
    await expect.poll(() => shownText(editor)).toBe('{\n  "service": "api"\n}');
  });
});
