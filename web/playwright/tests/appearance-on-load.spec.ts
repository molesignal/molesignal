/**
 * A signed-in page boots in the look its user saved, and keeps it.
 *
 * The browser remembers the last theme and density, so the first paint already
 * has them. The shell then applies the saved preferences over what was
 * remembered, as soon as it has read them. When the two agree nothing moves;
 * when they do not, the page changes its look a moment after it loads.
 */
import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const THEME_KEY = 'molesignal-theme';
const DENSITY_KEY = 'molesignal-density';

// Any page inside the shell does; this one has little else to load.
const ROUTE = '/account/settings/profile';

/** Each theme and density `<html>` has had since the document was created, in order. */
interface Looks {
  theme: string[];
  density: string[];
}

type Watched = Window & { looks: Looks & { settled: boolean } };

type MountOptions = NonNullable<Parameters<typeof mountMockRoutes>[2]>;

/**
 * Starts recording the page's looks before the app boots.
 *
 * The shell stores the theme when it applies the saved preferences, whether or
 * not that changes anything. A theme stored on a page that already has one is
 * the shell doing so, and once the page has gone idle after it, nothing is left
 * that would change the look.
 */
async function watchLooks(page: Page): Promise<void> {
  await page.addInitScript((themeKey) => {
    const looks: Watched['looks'] = { theme: [], density: [], settled: false };
    const add = (values: string[], value: string | null) => {
      if (value !== null && values[values.length - 1] !== value) values.push(value);
    };

    new MutationObserver((mutations) => {
      const root = document.documentElement;
      for (const mutation of mutations) {
        if (mutation.target !== root) continue;
        add(
          mutation.attributeName === 'data-theme' ? looks.theme : looks.density,
          mutation.oldValue,
        );
      }
      add(looks.theme, root.getAttribute('data-theme'));
      add(looks.density, root.getAttribute('data-density'));
    }).observe(document, {
      subtree: true,
      attributeFilter: ['data-theme', 'data-density'],
      attributeOldValue: true,
    });

    const store = Storage.prototype.setItem;
    Storage.prototype.setItem = function setItem(key, value) {
      store.call(this, key, value);
      if (key === themeKey && document.querySelector('html[data-theme]')) {
        window.requestIdleCallback(
          () => {
            looks.settled = true;
          },
          { timeout: 1_000 },
        );
      }
    };

    (window as unknown as Watched).looks = looks;
  }, THEME_KEY);
}

/** The looks the page has had, read once the saved preferences have been applied. */
async function settledLooks(page: Page): Promise<Looks> {
  await page.waitForFunction(() => (window as unknown as Watched).looks.settled);
  return page.evaluate(() => {
    const { theme, density } = (window as unknown as Watched).looks;
    return { theme, density };
  });
}

const systemTheme = (page: Page) =>
  page.evaluate(() =>
    window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light',
  );

const SAVED_LOOKS: Array<{
  name: string;
  options: MountOptions;
  theme: 'system' | 'dark' | 'light';
  density: string;
}> = [
  { name: 'the look of a user who chose none', options: {}, theme: 'system', density: 'normal' },
  { name: 'a light theme', options: { theme: 'light' }, theme: 'light', density: 'normal' },
  {
    name: 'a dark theme at compact density',
    options: { theme: 'dark', density: 'compact' },
    theme: 'dark',
    density: 'compact',
  },
  {
    name: 'a look saved along with the other preferences',
    options: { preferences: { theme: 'light', density: 'compact' } },
    theme: 'light',
    density: 'compact',
  },
];

test.describe('appearance on page load', () => {
  for (const saved of SAVED_LOOKS) {
    test(`boots in ${saved.name} and keeps it`, async ({ page, mockServer }) => {
      await mountMockRoutes(page, mockServer.port, saved.options);
      await watchLooks(page);
      await page.goto(ROUTE);

      const theme = saved.theme === 'system' ? await systemTheme(page) : saved.theme;
      expect(await settledLooks(page)).toEqual({
        theme: [theme],
        density: [saved.density],
      });
    });
  }

  test.describe('on a system set to light', () => {
    test.use({ colorScheme: 'light' });

    test('follows the system when the user chose no theme', async ({ page, mockServer }) => {
      await mountMockRoutes(page, mockServer.port);
      await watchLooks(page);
      await page.goto(ROUTE);

      expect(await settledLooks(page)).toEqual({ theme: ['light'], density: ['normal'] });
    });
  });

  test('keeps a theme chosen in the top bar when the page is loaded again', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    await watchLooks(page);
    await page.goto(ROUTE);
    await settledLooks(page);
    // Nothing is chosen yet, so the shortcut leaves the system theme for the other one.
    const chosen = (await systemTheme(page)) === 'dark' ? 'light' : 'dark';

    const saving = page.waitForResponse(
      (response) =>
        response.url().includes('/api/v1/me/preferences') &&
        response.request().method() === 'PUT',
    );
    await page.getByTestId('theme-toggle').click();
    await saving;
    await expect(page.locator('html')).toHaveAttribute('data-theme', chosen);

    await page.reload();
    expect(await settledLooks(page)).toEqual({ theme: [chosen], density: ['normal'] });
  });

  test('takes the saved look over a different one the browser remembers', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, { theme: 'light', density: 'compact' });
    // As after someone else used this browser: what it remembers is not this user's look.
    await page.addInitScript(
      ({ themeKey, densityKey }) => {
        localStorage.setItem(themeKey, 'dark');
        localStorage.setItem(densityKey, 'normal');
      },
      { themeKey: THEME_KEY, densityKey: DENSITY_KEY },
    );
    await watchLooks(page);
    await page.goto(ROUTE);

    expect(await settledLooks(page)).toEqual({
      theme: ['dark', 'light'],
      density: ['normal', 'compact'],
    });
  });
});
