/**
 * 04 — keyboard-only navigation + axe a11y baseline
 *
 * Three checks:
 *   1. Palette opens on ⌘K and help overlay opens on ⌘/ — without ever
 *      touching the mouse. The ⌘K dialog is waited out (detached) before
 *      ⌘/ is pressed so the two dialogs never race.
 *   2. axe-core finds zero `critical` violations on the post-login shell.
 *   3. Tab focus stays visible. From the top of a route the first 24 stops
 *      are the top bar and the sidebar, so the Logs and Traces explorers are
 *      also walked from the link that skips to the content, through their own
 *      controls: as the shared mock leaves them, with nothing to list, and
 *      with results from `mockExplorers`.
 */
import AxeBuilder from '@axe-core/playwright';
import type { ElementHandle, Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import {
  EXPLORER_ROW,
  expectExplorerResults,
  serveExplorerResults,
  type Explorer,
} from '../fixtures/mockExplorers';

/** A control the focus stopped on. */
interface Stop {
  /** What a failure calls it. */
  name: string;
  /** The same for a control that is drawn many times over, a field of a list and the next: such look alike. */
  kind: string;
  inPage: boolean;
  hiddenAncestor: boolean;
  onScreen: boolean;
  /** Text is typed in it: there the caret marks the focus, and a picture does not hold a caret still. */
  typedIn: boolean;
  /** A row of the explorer's list. */
  row: boolean;
  /** What of it is on the screen. */
  box: { x: number; y: number; width: number; height: number };
}

type Control = ElementHandle<HTMLElement>;

/** What the focus is on, if it is on anything. */
async function focused(page: Page, row: string): Promise<{ stop: Stop; control: Control } | null> {
  const control = (await page.evaluateHandle(() => document.activeElement)).asElement() as Control | null;
  if (!control) return null;

  const stop = await control.evaluate((active, row): Stop | null => {
    if (active === document.body) return null;

    const rect = active.getBoundingClientRect();
    const left = Math.max(0, Math.floor(rect.left));
    const top = Math.max(0, Math.floor(rect.top));
    const states = ['role', 'aria-pressed', 'aria-selected', 'aria-expanded', 'aria-checked', 'data-state'];
    const said = active.innerText.replace(/\s+/g, ' ').trim().slice(0, 40);
    const untyped = ['checkbox', 'radio', 'range', 'button', 'submit', 'reset', 'file', 'color'];

    return {
      name: active.getAttribute('aria-label') ?? (said || active.tagName),
      kind: [active.tagName, ...states.map((state) => active.getAttribute(state)), active.className].join(' '),
      inPage: Boolean(active.closest('#main')),
      hiddenAncestor: Boolean(active.closest('[aria-hidden="true"], [inert]')),
      onScreen:
        rect.width > 0 &&
        rect.height > 0 &&
        rect.bottom > 0 &&
        rect.right > 0 &&
        rect.top < window.innerHeight &&
        rect.left < window.innerWidth,
      typedIn:
        active.matches('textarea, [contenteditable="true"]') ||
        (active instanceof HTMLInputElement && !untyped.includes(active.type)),
      row: active.matches(row),
      box: {
        x: left,
        y: top,
        width: Math.max(1, Math.min(window.innerWidth, Math.ceil(rect.right)) - left),
        height: Math.max(1, Math.min(window.innerHeight, Math.ceil(rect.bottom)) - top),
      },
    };
  }, row);

  return stop && { stop, control };
}

/** The part of the page a control is drawn in, with every transition at its end. */
const picture = (page: Page, box: Stop['box']) => page.screenshot({ clip: box, animations: 'disabled' });

/**
 * Whether the focus shows on `control`: it has to look different with the
 * focus on it than with the focus on the stop before it. Where a control shows
 * the focus differs (a fill of its own, the row it heads lit as a whole, a
 * line drawn inside it), so what is compared is the picture, not a property.
 * Any difference counts: this finds a control the focus leaves as it was, and
 * does not judge how strong a mark is.
 */
async function showsFocus(page: Page, stop: Stop, control: Control, before: Control): Promise<boolean> {
  const lit = await picture(page, stop.box);

  // Back to the stop before it. A stop that is gone by now (the buttons of a row the focus
  // has left) cannot take the focus, and then nothing has it.
  await control.evaluate((control, before) => {
    before.focus({ preventScroll: true });
    if (document.activeElement === control) control.blur();
  }, before);
  const unlit = await picture(page, stop.box);
  await control.evaluate((control) => control.focus({ preventScroll: true }));

  return !lit.equals(unlit);
}

test.describe('a11y / keyboard', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('keyboard-only palette + help; axe scan has no critical violations', async ({ page }) => {
    await page.goto('/investigate');

    // Palette open + close (Esc). Wait for the input to fully detach so the
    // next help keystroke does not get swallowed by an in-flight unmount.
    await page.keyboard.press('Meta+K');
    const paletteInput = page.getByPlaceholder(/search commands/i);
    await expect(paletteInput).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(paletteInput).toBeHidden({ timeout: 5_000 });

    // Help overlay open via modified shortcut.
    await page.keyboard.press('Meta+/');
    await expect(page.getByText(/keyboard shortcuts/i)).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByText(/keyboard shortcuts/i)).toBeHidden({ timeout: 5_000 });

    // axe scan on the resting shell. Exclude live-region status strip so
    // mid-route announce events don't get sampled.
    const results = await new AxeBuilder({ page })
      .exclude('[role="status"]')
      .exclude('[aria-live]')
      .analyze();
    const critical = results.violations.filter((v) => v.impact === 'critical');
    expect(critical, JSON.stringify(critical, null, 2)).toEqual([]);
  });

  // One test for each route: a page load and its 24 tab stops apiece, so no
  // route waits on the time the others took.
  for (const route of ['/home', '/logs', '/metrics', '/traces', '/alerts']) {
    test(`${route} keeps tab focus visible and out of hidden subtrees`, async ({
      page,
    }) => {
      await page.goto(route);
      await page.waitForLoadState('networkidle').catch(() => undefined);
      await page.locator('body').click({ position: { x: 2, y: 2 } });

      for (let index = 0; index < 24; index += 1) {
        await page.keyboard.press('Tab');
        const focus = await page.evaluate(() => {
          const active = document.activeElement as HTMLElement | null;
          if (!active || active === document.body) return null;
          const rect = active.getBoundingClientRect();
          const style = getComputedStyle(active);
          return {
            tag: active.tagName,
            hiddenAncestor: Boolean(active.closest('[aria-hidden="true"], [inert]')),
            visible:
              rect.width > 0 &&
              rect.height > 0 &&
              rect.bottom > 0 &&
              rect.right > 0 &&
              rect.top < window.innerHeight &&
              rect.left < window.innerWidth,
            filter: style.filter,
            background: style.backgroundColor,
          };
        });
        expect(focus, `${route} tab ${index + 1} did not reach a control`).not.toBeNull();
        expect(focus?.hiddenAncestor, `${route} focused a hidden control`).toBe(false);
        expect(focus?.visible, `${route} focus moved off screen`).toBe(true);
        expect(
          focus?.filter !== 'none' || focus?.background !== 'rgba(0, 0, 0, 0)',
          `${route} focus has no visible fill/foreground treatment`,
        ).toBe(true);
      }
    });
  }

  for (const explorer of ['logs', 'traces'] as Explorer[]) {
    for (const results of [false, true]) {
      test(`/${explorer}${results ? ' with results' : ''} shows the tab focus on each of its own controls`, async ({
        page,
      }) => {
        // Two pictures of every kind of control the page has.
        test.slow();
        if (results) await serveExplorerResults(page);
        await page.goto(`/${explorer}`);
        await page.waitForLoadState('networkidle').catch(() => undefined);
        if (results) await expectExplorerResults(page, explorer);

        // The way past the shell from the keyboard: the first stop of the document skips to the content.
        await page.keyboard.press('Tab');
        const skip = page.getByRole('link', { name: 'Skip to content' });
        await expect(skip).toBeFocused();
        let before = (await skip.elementHandle()) as Control;
        await page.keyboard.press('Enter');

        const row = EXPLORER_ROW[explorer];
        const pictured = new Set<string>();
        const unmarked: string[] = [];
        let stops = 0;

        /** Tab once, and hold what the focus lands on to what is asked of a stop. `null` once it has left the page. */
        const next = async (): Promise<Stop | null> => {
          await page.keyboard.press('Tab');
          const at = await focused(page, row);
          if (!at?.stop.inPage) return null;

          const { stop, control } = at;
          stops += 1;
          expect(stop.hiddenAncestor, `/${explorer} focused a hidden control: ${stop.name}`).toBe(false);
          expect(stop.onScreen, `/${explorer} focus moved off screen: ${stop.name}`).toBe(true);
          // One of a kind is pictured: the fields of a list and the rows of a result are one control each.
          if (!stop.typedIn && !pictured.has(stop.kind)) {
            pictured.add(stop.kind);
            if (!(await showsFocus(page, stop, control, before))) unmarked.push(stop.name);
          }
          before = control;

          return stop;
        };

        // From the head of the page into the list: what a page with nothing to list has is passed
        // on the way, and the list is left after its second row.
        for (let rows = 0; rows < 2 && stops < 120; ) {
          const stop = await next();
          if (!stop) break;
          if (stop.row) rows += 1;
        }

        // What stands after the list is reached from its last row.
        if (results) {
          const last = page.locator(row).last();
          await last.focus();
          before = (await last.elementHandle()) as Control;
          while ((await next()) && stops < 130);
        }

        // The page has controls of its own, and the walk reached them.
        expect(stops, `/${explorer} stops inside the page`).toBeGreaterThan(10);
        expect(unmarked, `/${explorer}: controls that look the same with the tab focus on them as without`).toEqual(
          [],
        );
      });
    }
  }
});
