import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_PASSWORD } from '../fixtures/mockMfa';
import { signInWithPassword } from '../fixtures/signinFlow';

const LAST_SIGNIN = 'molesignal.signin.last';

/** What the browser kept of the last sign-in. The page may be on its way elsewhere: try again. */
function keptSignin(page: Page) {
  return page
    .evaluate((key) => JSON.parse(localStorage.getItem(key) ?? 'null') as unknown, LAST_SIGNIN)
    .catch(() => undefined);
}

/** As if an earlier visit had signed in this way. */
async function signedInBefore(page: Page, last: object) {
  await page.addInitScript(
    ([key, value]) => localStorage.setItem(key!, value!),
    [LAST_SIGNIN, JSON.stringify(last)],
  );
}

async function allowSignup(page: Page) {
  await page.route('**/api/v1/instance', (route) =>
    route.fulfill({ json: { externalUrl: '', version: '26.0.0.0', signupEnabled: true } }),
  );
}

async function openSignin(page: Page, instance: { signupEnabled: boolean }) {
  await page.route('**/api/v1/auth/sso/providers', (route) => route.fulfill({ json: [] }));
  await page.route('**/api/v1/instance', (route) =>
    route.fulfill({
      json: { externalUrl: '', version: '26.0.0.0', ...instance },
    }),
  );
  await page.goto('/signin');
  await expect(page.getByTestId('signin-card')).toBeVisible();
}

test.describe('Sign-in page', () => {
  test('puts one form card in a centred column, between the brand and the version', async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await openSignin(page, { signupEnabled: false });

    const card = (await page.getByTestId('signin-card').boundingBox())!;
    const logo = (await page.getByTestId('signin-logo').boundingBox())!;
    const version = (await page.getByTestId('signin-build-info').boundingBox())!;

    // Centred in the viewport, and a form-sized card, not a split panel.
    expect(Math.abs(card.x + card.width / 2 - 720)).toBeLessThanOrEqual(1);
    expect(card.width).toBeLessThanOrEqual(440);
    // Reading order: brand, card, version.
    expect(logo.y + logo.height).toBeLessThanOrEqual(card.y);
    expect(card.y + card.height).toBeLessThanOrEqual(version.y);
  });

  test('centres the statement and the promise under the logo by what they draw', async ({ page }) => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.addInitScript(() => {
      localStorage.setItem('molesignal-ui-prefs', JSON.stringify({ state: { language: 'zh-cn' }, version: 0 }));
    });
    await openSignin(page, { signupEnabled: false });
    await page.evaluate(() => document.fonts.ready);

    // A line ending on fullwidth punctuation draws the mark in the first 0.38 em of its box: the
    // drawn text runs from the first character to there, and its middle is under the logo's.
    const centres = await page.evaluate(() => {
      const middle = (box: DOMRect) => box.left + box.width / 2;
      const logo = middle(document.querySelector('[data-testid="signin-logo"]')!.getBoundingClientRect());
      const drawn = (element: Element) => {
        const text = [...element.querySelectorAll('*'), element].flatMap((node) =>
          [...node.childNodes].filter((child): child is Text => child instanceof Text && child.data.trim() !== ''),
        );
        const range = document.createRange();
        range.setStart(text[0]!, 0);
        range.setEnd(text[0]!, 1);
        const first = range.getBoundingClientRect();
        const last = text.at(-1)!;
        range.setStart(last, last.data.length - 1);
        range.setEnd(last, last.data.length);
        const end = range.getBoundingClientRect();
        const size = Number.parseFloat(getComputedStyle(last.parentElement!).fontSize);
        const right = /[，。]$/u.test(last.data) ? end.left + 0.38 * size : end.right;
        return (first.left + right) / 2 - logo;
      };
      const [statement, promise] = [...document.querySelectorAll('[data-testid="signin-brand"] p')];
      return [...statement!.children, promise!].map(drawn);
    });
    for (const offset of centres) expect(Math.abs(offset)).toBeLessThanOrEqual(1.5);
  });

  test('shows the product version and nothing about the release channel', async ({ page }) => {
    await openSignin(page, { signupEnabled: false });

    await expect(page.getByTestId('signin-build-info')).toHaveText('v26.0.0.0');
    await expect(page.getByText(/alpha|beta|stable/i)).toHaveCount(0);
  });

  test('offers the sign-up entry only when the instance allows it', async ({ page }) => {
    await openSignin(page, { signupEnabled: false });
    await expect(page.getByRole('button', { name: /Sign up/ })).toHaveCount(0);

    await page.unrouteAll();
    await openSignin(page, { signupEnabled: true });
    await expect(page.getByRole('button', { name: /Sign up/ })).toBeVisible();
  });

  for (const viewport of [
    { width: 390, height: 844 },
    { width: 320, height: 568 },
  ]) {
    test(`fits a ${viewport.width}px phone without scrolling sideways`, async ({ page }) => {
      await page.setViewportSize(viewport);
      await openSignin(page, { signupEnabled: true });

      const overflow = await page.evaluate(() => {
        const stage = document.querySelector<HTMLElement>('[data-testid="auth-stage"]')!;
        return {
          page: document.documentElement.scrollWidth - window.innerWidth,
          stage: stage.scrollWidth - stage.clientWidth,
        };
      });
      expect(overflow.page).toBeLessThanOrEqual(0);
      expect(overflow.stage).toBeLessThanOrEqual(0);

      // A short screen scrolls the stage instead of clipping the form.
      const submit = page.getByRole('button', { name: 'Continue →', exact: true });
      await submit.scrollIntoViewIfNeeded();
      const box = (await submit.boundingBox())!;
      expect(box.y).toBeGreaterThanOrEqual(0);
      expect(box.y + box.height).toBeLessThanOrEqual(viewport.height);
    });
  }
});

test.describe('Signing in one question at a time', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { signedOut: true });
  });

  test('goes from the ways in to the email to the password, and back', async ({ page }) => {
    await page.goto('/signin');

    const channels = page.getByTestId('signin-channels');
    await expect(channels.getByRole('button', { name: 'Continue with Okta' })).toBeVisible();
    await expect(page.getByText('Last used')).toHaveCount(0);
    await expect(page.getByLabel('Email')).toHaveCount(0);
    await expect(page.getByLabel('Password', { exact: true })).toHaveCount(0);

    await page.getByRole('button', { name: 'Sign in with email' }).click();
    const email = page.getByLabel('Email');
    await expect(email).toBeFocused();
    await expect(page.getByLabel('Password', { exact: true })).toHaveCount(0);
    const next = page.getByRole('button', { name: 'Continue →', exact: true });
    await expect(next).toBeDisabled();

    await email.fill('dev@molesignal.local');
    await next.click();

    await expect(page.getByLabel('Password', { exact: true })).toBeFocused();
    await expect(page.getByTestId('signin-account')).toHaveText('dev@molesignal.local');
    await expect(page.getByRole('button', { name: 'Sign in →', exact: true })).toBeDisabled();

    // One question back each time, and what was typed is still there.
    await page.getByRole('button', { name: 'Back' }).click();
    await expect(email).toHaveValue('dev@molesignal.local');
    await page.getByRole('button', { name: 'Back' }).click();
    await expect(channels).toBeVisible();
    await page.getByRole('button', { name: 'Sign in with email' }).click();
    await expect(email).toHaveValue('dev@molesignal.local');
  });

  test('leaves a password manager the account on both steps', async ({ page }) => {
    await page.goto('/signin');
    await page.getByRole('button', { name: 'Sign in with email' }).click();

    const email = page.locator('form input[name="username"]');
    await expect(email).toHaveAttribute('autocomplete', 'username');
    await email.fill('dev@molesignal.local');
    await page.keyboard.press('Enter');

    const password = page.locator('form input[type="password"]');
    await expect(password).toHaveAttribute('autocomplete', 'current-password');

    // The same form still holds the account, in a field nobody sees or tabs into.
    const account = page.locator('form input[autocomplete="username"]');
    await expect(account).toHaveValue('dev@molesignal.local');
    await expect(account).toHaveAttribute('tabindex', '-1');
    await expect(account).toHaveAttribute('aria-hidden', 'true');

    await password.fill(MOCK_PASSWORD);
    await page.keyboard.press('Enter');
    await expect(page).not.toHaveURL(/\/signin/);
  });

  test('keeps the way in and the account for the next visit, never the password', async ({
    page,
  }) => {
    test.slow();
    await page.goto('/signin');
    await signInWithPassword(page, 'dev@molesignal.local', MOCK_PASSWORD);
    await expect(page).not.toHaveURL(/\/signin/);

    await expect
      .poll(() => keptSignin(page))
      .toEqual({ channel: { kind: 'password' }, account: 'dev@molesignal.local' });
    const stored = await page.evaluate(() =>
      JSON.stringify([{ ...localStorage }, { ...sessionStorage }]),
    );
    expect(stored).not.toContain(MOCK_PASSWORD);

    // Signing out doesn't forget it: that is when it's needed.
    await page.goto('/account/settings/sessions');
    await page.getByRole('button', { name: 'Sign out this device' }).click();
    const row = page.getByRole('button', { name: /Continue with email/ });
    await expect(row).toContainText('dev@molesignal.local');
    await expect(row).toContainText('Last used');
  });

  test('offers the account that used its password last, straight on to the password', async ({
    page,
  }) => {
    await signedInBefore(page, { channel: { kind: 'password' }, account: 'dev@molesignal.local' });
    await page.goto('/signin');

    const row = page.getByRole('button', { name: /Continue with email/ });
    await expect(row).toContainText('dev@molesignal.local');
    await expect(row).toContainText('Last used');
    await expect(page.getByRole('button', { name: 'Continue with Okta' })).not.toContainText(
      'Last used',
    );

    await row.click();
    await expect(page.getByTestId('signin-account')).toHaveText('dev@molesignal.local');
    await expect(page.getByLabel('Password', { exact: true })).toBeFocused();

    // To sign in as someone else, the way back leads to the question it skipped.
    await page.getByRole('button', { name: 'Back' }).click();
    await expect(page.getByLabel('Email')).toHaveValue('dev@molesignal.local');
    await page.getByRole('button', { name: 'Back' }).click();
    await expect(row).toBeVisible();
  });

  test('marks the provider used last, and whose account it was', async ({ page }) => {
    await signedInBefore(page, {
      channel: { kind: 'provider', id: 'okta' },
      account: 'alice@example.com',
    });
    await page.goto('/signin');

    const row = page.getByRole('button', { name: /Continue with Okta/ });
    await expect(row).toContainText('Last used');
    await expect(row).toContainText('alice@example.com');
  });

  test('keeps a provider only once its sign-in has come back and finished', async ({ page }) => {
    // The trip to the identity provider and back.
    await page.route('**/api/v1/auth/sso/login*', (route) =>
      route.fulfill({
        contentType: 'text/html',
        body: "<script>location.replace('/signin?sso=link-demo')</script>",
      }),
    );
    await page.goto('/signin');
    await page.getByRole('button', { name: 'Continue with Okta' }).click();

    // Two full page loads on the way, not one.
    await expect(page.getByRole('heading', { name: 'Confirm it’s your account' })).toBeVisible({
      timeout: 15_000,
    });
    expect(await keptSignin(page)).toBeNull();

    await page.getByLabel('Password', { exact: true }).fill(MOCK_PASSWORD);
    await page.getByRole('button', { name: 'Confirm and sign in →' }).click();

    await expect
      .poll(() => keptSignin(page))
      .toEqual({ channel: { kind: 'provider', id: 'okta' }, account: 'dev@molesignal.local' });
  });

  test('asks a directory for its username, and keeps that, not the email', async ({ page }) => {
    let accepted = false;
    await page.route('**/api/v1/auth/sso/providers', (route) =>
      route.fulfill({
        json: [{ id: 'corp', name: 'Corp LDAP', kind: 'ldap', platform: 'custom' }],
      }),
    );
    await page.route('**/api/v1/auth/sso/ldap/login', (route) =>
      accepted
        ? route.fulfill({
            json: {
              login: {
                userId: 'dev',
                email: 'dev@molesignal.local',
                orgId: 'acme-prod',
                displayRole: 'Owner',
                roles: [],
                scope: 'organization',
              },
            },
          })
        : route.fulfill({
            status: 401,
            json: { error: 'unauthorized', message: 'unauthorized: invalid credentials' },
          }),
    );
    await page.goto('/signin');

    await page.getByRole('button', { name: 'Continue with Corp LDAP' }).click();
    await expect(page.getByRole('heading', { name: 'Sign in with Corp LDAP' })).toBeVisible();
    await page.getByLabel('Username').fill('alice');
    await page.getByRole('button', { name: 'Continue →', exact: true }).click();

    await expect(page.getByTestId('signin-account')).toHaveText('alice');
    // The password is the directory's: this instance can't reset it.
    await expect(page.getByRole('button', { name: 'Forgot password?' })).toHaveCount(0);

    await page.getByLabel('Password', { exact: true }).fill('directory-secret');
    const sent = page.waitForRequest('**/api/v1/auth/sso/ldap/login');
    await page.getByRole('button', { name: 'Sign in →', exact: true }).click();
    expect((await sent).postDataJSON()).toMatchObject({
      providerId: 'corp',
      username: 'alice',
      password: 'directory-secret',
    });

    // A failed try stays on the step, and is nothing to point at next time.
    await expect(page.getByText('unauthorized: invalid credentials')).toBeVisible();
    await expect(page.getByLabel('Password', { exact: true })).toHaveValue('directory-secret');
    expect(await keptSignin(page)).toBeNull();

    accepted = true;
    await page.getByRole('button', { name: 'Sign in →', exact: true }).click();
    await expect
      .poll(() => keptSignin(page))
      .toEqual({ channel: { kind: 'provider', id: 'corp' }, account: 'alice' });
  });

  test('a directory used last goes straight to its password', async ({ page }) => {
    await page.route('**/api/v1/auth/sso/providers', (route) =>
      route.fulfill({
        json: [{ id: 'corp', name: 'Corp LDAP', kind: 'ldap', platform: 'custom' }],
      }),
    );
    await signedInBefore(page, { channel: { kind: 'provider', id: 'corp' }, account: 'alice' });
    await page.goto('/signin');

    await page.getByRole('button', { name: /Continue with Corp LDAP/ }).click();
    await expect(page.getByTestId('signin-account')).toHaveText('alice');

    await page.getByRole('button', { name: 'Back' }).click();
    await expect(page.getByLabel('Username')).toHaveValue('alice');
    await page.getByRole('button', { name: 'Back' }).click();

    // Its username has no place in the email field.
    await page.getByRole('button', { name: 'Sign in with email' }).click();
    await expect(page.getByLabel('Email')).toHaveValue('');
  });

  test('a forgotten password starts from the email typed, and comes back to the step', async ({
    page,
  }) => {
    await page.goto('/signin');
    await page.getByRole('button', { name: 'Sign in with email' }).click();
    await page.getByLabel('Email').fill('dev@molesignal.local');
    await page.getByRole('button', { name: 'Continue →', exact: true }).click();

    await page.getByRole('button', { name: 'Forgot password?' }).click();
    await expect(page.getByLabel('Email')).toHaveValue('dev@molesignal.local');

    await page.getByRole('button', { name: 'Back to sign in' }).click();
    await expect(page.getByTestId('signin-account')).toHaveText('dev@molesignal.local');
    await expect(page.getByLabel('Password', { exact: true })).toBeVisible();
  });

  test('carries the typed email to sign-up, which asks for a name and a password', async ({
    page,
  }) => {
    await allowSignup(page);
    await page.goto('/signin');
    await page.getByRole('button', { name: 'Sign in with email' }).click();
    await page.getByLabel('Email').fill('new@example.com');
    await page.getByRole('button', { name: 'Continue →', exact: true }).click();
    await page.getByRole('button', { name: 'Sign up' }).click();

    await expect(page).toHaveURL(/\/signup$/);
    await expect(page.getByTestId('signin-account')).toHaveText('new@example.com');
    await expect(page.getByText('Use at least 8 characters.')).toBeVisible();

    const name = page.getByLabel('Name', { exact: true });
    const password = page.getByLabel('Password', { exact: true });
    const submit = page.getByRole('button', { name: 'Create account →', exact: true });
    await expect(name).toBeFocused();
    await expect(submit).toBeDisabled();

    await name.fill('New');
    await password.fill('short');
    await expect(submit).toBeDisabled();
    await password.fill(MOCK_PASSWORD);
    await expect(submit).toBeEnabled();

    await page.getByRole('button', { name: 'Show password' }).click();
    await expect(password).toHaveAttribute('type', 'text');
    await page.getByRole('button', { name: 'Hide password' }).click();
    await expect(password).toHaveAttribute('type', 'password');

    await page.getByRole('button', { name: 'Back' }).click();
    await expect(page.getByLabel('Email')).toHaveValue('new@example.com');
    await page.getByRole('button', { name: 'Back' }).click();
    await expect(page).toHaveURL(/\/signin$/);
  });

  test('sign-up starts with the email when nobody typed one', async ({ page }) => {
    await allowSignup(page);
    await signedInBefore(page, { channel: { kind: 'password' }, account: 'dev@molesignal.local' });
    await page.goto('/signin');

    // The account that signed in last already has one.
    await page.getByRole('button', { name: 'Sign in with email' }).click();
    await expect(page.getByLabel('Email')).toHaveValue('dev@molesignal.local');
    await page.getByRole('button', { name: 'Sign up' }).click();

    await expect(page).toHaveURL(/\/signup$/);
    await expect(page.getByLabel('Email')).toHaveValue('');
    await expect(page.getByLabel('Name', { exact: true })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Continue →', exact: true })).toBeDisabled();
  });

  test('does not hold the email back for a slow answer about the providers', async ({ page }) => {
    let answer = () => {};
    const answered = new Promise<void>((resolve) => {
      answer = resolve;
    });
    await page.route('**/api/v1/auth/sso/providers', async (route) => {
      await answered;
      await route.fulfill({
        json: [{ id: 'okta', name: 'Okta', kind: 'oidc', platform: 'okta' }],
      });
    });
    await page.goto('/signin');

    const email = page.getByLabel('Email');
    await email.fill('dev@molesignal.local');
    answer();

    // Nothing is taken away from under the typing: what arrived late is one step back.
    await page.getByRole('button', { name: 'Back' }).click();
    await expect(page.getByRole('button', { name: 'Continue with Okta' })).toBeVisible();
  });

  test('cuts a long account on a phone instead of scrolling sideways', async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 568 });
    const account = `${'someone.with.a.very.long.name'.repeat(3)}@example.com`;
    await signedInBefore(page, { channel: { kind: 'password' }, account });
    await page.goto('/signin');

    const overflow = () =>
      page.evaluate(() => {
        const stage = document.querySelector<HTMLElement>('[data-testid="auth-stage"]')!;
        return [
          document.documentElement.scrollWidth - window.innerWidth,
          stage.scrollWidth - stage.clientWidth,
        ];
      });

    await expect(page.getByTestId('signin-channels')).toContainText('Last used');
    expect(Math.max(...(await overflow()))).toBeLessThanOrEqual(0);

    await page.getByRole('button', { name: /Continue with email/ }).click();
    await expect(page.getByTestId('signin-account')).toContainText(account);
    expect(Math.max(...(await overflow()))).toBeLessThanOrEqual(0);
  });
});
