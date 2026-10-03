import type { BrowserContext, Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const AUTH_KEY = 'molesignal-auth';

/**
 * A tab of one browser. Like a sign-in without "remember me", the session of a
 * signed-in tab lives in its own sessionStorage and nowhere else.
 */
async function openTab(
  context: BrowserContext,
  port: number,
  { signedIn }: { signedIn: boolean },
): Promise<Page> {
  const page = await context.newPage();
  await mountMockRoutes(page, port);
  // Runs after the fixture's own script, which seeds localStorage. Only the
  // tab's first document is seeded: a reload after signing out must stay signed out.
  await page.addInitScript(
    ({ key, signedIn }) => {
      const seeded = localStorage.getItem(key);
      localStorage.removeItem(key);
      if (sessionStorage.getItem('__tab-seeded')) return;
      sessionStorage.setItem('__tab-seeded', '1');
      if (signedIn && seeded) sessionStorage.setItem(key, seeded);
    },
    { key: AUTH_KEY, signedIn },
  );
  return page;
}

const shellVisible = (page: Page) => expect(page.getByTestId('user-menu-trigger')).toBeVisible();

test.describe('sessions across tabs', () => {
  test('a tab opened by hand takes the session of an open tab instead of asking to sign in', async ({
    context,
    mockServer,
  }) => {
    const first = await openTab(context, mockServer.port, { signedIn: true });
    await first.goto('/home');
    await shellVisible(first);

    const second = await openTab(context, mockServer.port, { signedIn: false });
    await second.goto('/alerts');

    await expect(second).toHaveURL(/\/alerts(\/|$)/);
    await shellVisible(second);
    // Kept for this tab only: it goes with the tab, not with the browser session.
    expect(await second.evaluate((key) => sessionStorage.getItem(key) !== null, AUTH_KEY)).toBe(true);
    expect(await second.evaluate((key) => localStorage.getItem(key), AUTH_KEY)).toBeNull();
  });

  test('a tab with nobody to borrow from asks to sign in, and returns to the page afterwards', async ({
    context,
    mockServer,
  }) => {
    const lone = await openTab(context, mockServer.port, { signedIn: false });
    await lone.goto('/alerts');

    await expect(lone).toHaveURL(/\/signin\?next=%2Falerts/);
  });

  test('signing out in one tab signs out the others, and later tabs have nothing to borrow', async ({
    context,
    mockServer,
  }) => {
    const first = await openTab(context, mockServer.port, { signedIn: true });
    await first.goto('/home');
    await shellVisible(first);
    const second = await openTab(context, mockServer.port, { signedIn: false });
    await second.goto('/alerts');
    await shellVisible(second);

    await first.getByTestId('user-menu-trigger').click();
    await first.getByRole('menuitem', { name: 'Sign out' }).click();

    await expect(first).toHaveURL(/\/signin/);
    await expect(second).toHaveURL(/\/signin\?next=%2Falerts/);
    // The token is gone from the tab (the store persists the signed-out state).
    const kept = await second.evaluate(
      (key) => JSON.parse(sessionStorage.getItem(key) ?? '{"state":{}}').state.token ?? null,
      AUTH_KEY,
    );
    expect(kept).toBeNull();

    const third = await openTab(context, mockServer.port, { signedIn: false });
    await third.goto('/home');
    await expect(third).toHaveURL(/\/signin\?next=%2Fhome/);
  });
});
