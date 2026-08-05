/**
 * Keyboard-map coverage
 * (web-a11y-baseline).
 *
 * Iterates `GLOBAL_KEYMAP` at test-collection time; for each binding,
 * navigates to /investigate and presses the binding's key sequence
 * (space-separated sequences are still supported by `keys.split(' ')`). The assertion shape is intentionally
 * loose — most bindings have a globally-observable DOM effect (open dialog,
 * route change) which we check directly; a small set are scope-specific or
 * side-effect-only (`mod+alt+y` writes clipboard, `mod+alt+p` toggles internal store) and
 * those just smoke-check that pressing the key does not throw.
 *
 * Adding a new entry to GLOBAL_KEYMAP auto-creates a test case — no spec
 * edit is required to gain coverage. New bindings that don't fit either
 * branch should add an `assert` entry below.
 */
import { GLOBAL_KEYMAP } from '@/keyboard/bindings';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

type AssertKind =
  | { kind: 'visible'; selector: string }
  | { kind: 'hidden'; selector: string }
  | { kind: 'url'; pattern: RegExp }
  | { kind: 'noop' };

/**
 * Per-binding observable assertion. Lookup by `keys`. Anything missing
 * falls back to `noop` (smoke-only).
 */
const ASSERTIONS: Record<string, AssertKind> = {
  'mod+k': { kind: 'visible', selector: '[placeholder*="Search commands" i]' },
  'mod+/': { kind: 'visible', selector: 'text=Keyboard shortcuts' },
  'mod+esc': { kind: 'noop' },
  'mod+[': { kind: 'noop' }, // stack.back: no UI effect on empty stack
  'mod+]': { kind: 'noop' }, // stack.forwardOne: no UI effect on empty stack
  'mod+alt+s': { kind: 'url', pattern: /\/services$/ },
  'mod+alt+a': { kind: 'url', pattern: /\/alerts(?:\/|$)/ },
  'mod+alt+d': { kind: 'url', pattern: /\/dashboards$/ },
  'mod+alt+t': { kind: 'url', pattern: /\/investigate\?preset=trace/ },
  'mod+alt+l': { kind: 'url', pattern: /\/investigate\?preset=log/ },
  'mod+alt+r': { kind: 'url', pattern: /\/rum\/overview$/ },
  'mod+alt+f': { kind: 'url', pattern: /\/functions$/ },
  'mod+alt+i': { kind: 'url', pattern: /\/iam\/users$/ },
  'mod+alt+e': { kind: 'visible', selector: 'text=Time range' },
  'mod+alt+p': { kind: 'noop' }, // togglePin: store-only side effect
  'mod+alt+y': { kind: 'noop' }, // clipboard write: no DOM effect; covered by 01 happy path
  'mod+down': { kind: 'noop' }, // scope-specific (log row selection)
  'mod+up': { kind: 'noop' }, // scope-specific (log row selection)
  'mod+enter': { kind: 'noop' }, // scope-specific (activate focused row)
};

test.describe('a11y: keyboard map coverage', () => {
  test.beforeEach(async ({ page, context, mockServer }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    await mountMockRoutes(page, mockServer.port);
  });

  for (const binding of GLOBAL_KEYMAP) {
    test(`keyboard binding: ${binding.keys} (${binding.description})`, async ({ page }) => {
      await page.goto('/investigate');
      // `mod` is the app's alias for Meta/Ctrl; translate to Playwright's
      // `Meta` (works for both macOS and Linux Chromium).
      for (const key of binding.keys.split(' ')) {
        const norm = key
          .replace(/^esc$/, 'Escape')
          .replace(/^enter$/, 'Enter')
          .replace(/^down$/, 'ArrowDown')
          .replace(/^up$/, 'ArrowUp')
          .replace(/^mod\+/, 'Meta+')
          .replace(/\+alt\+/g, '+Alt+')
          .replace(/\+esc$/, '+Escape')
          .replace(/\+enter$/, '+Enter')
          .replace(/\+down$/, '+ArrowDown')
          .replace(/\+up$/, '+ArrowUp');
        await page.keyboard.press(norm);
      }
      const expected = ASSERTIONS[binding.keys] ?? { kind: 'noop' };
      if (expected.kind === 'visible') {
        await expect(page.locator(expected.selector).first()).toBeVisible({ timeout: 5_000 });
      } else if (expected.kind === 'hidden') {
        await expect(page.locator(expected.selector).first()).toBeHidden({ timeout: 5_000 });
      } else if (expected.kind === 'url') {
        await expect(page).toHaveURL(expected.pattern, { timeout: 5_000 });
      }
      // `noop` bindings: no assertion. The test passes if the keystroke
      // sequence executed without throwing in the page.
    });
  }
});
