import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_PASSWORD, MOCK_RECOVERY_CODES } from '../fixtures/mockMfa';
import { signInWithPassword } from '../fixtures/signinFlow';

/**
 * A software authenticator in the browser: it keeps discoverable credentials, like
 * iCloud Keychain and Google Password Manager do, and answers every prompt at once
 * as if the person had confirmed with a fingerprint.
 */
async function addVirtualAuthenticator(page: Page) {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('WebAuthn.enable');

  const { authenticatorId } = await cdp.send('WebAuthn.addVirtualAuthenticator', {
    options: {
      protocol: 'ctap2',
      ctap2Version: 'ctap2_1',
      transport: 'internal',
      hasResidentKey: true,
      hasUserVerification: true,
      isUserVerified: true,
      automaticPresenceSimulation: true,
    },
  });

  return {
    credentials: async () =>
      (await cdp.send('WebAuthn.getCredentials', { authenticatorId })).credentials,
    /** The person backs out of the browser's prompt from now on. */
    refuse: () =>
      cdp.send('WebAuthn.setUserVerified', { authenticatorId, isUserVerified: false }),
  };
}

/** Records the page's requests to the browser for a passkey, from the next page on. */
async function watchPasskeyRequests(page: Page) {
  await page.addInitScript(() => {
    const original = navigator.credentials.get.bind(navigator.credentials);
    const seen: CredentialRequestOptions[] = [];
    (window as unknown as { __passkeyRequests: CredentialRequestOptions[] }).__passkeyRequests =
      seen;
    navigator.credentials.get = (options) => {
      if (options) seen.push(options);
      return original(options);
    };
  });

  return () =>
    page.evaluate(() =>
      (
        window as unknown as { __passkeyRequests: CredentialRequestOptions[] }
      ).__passkeyRequests.map((request) => ({
        mediation: request.mediation ?? null,
        aborted: request.signal?.aborted ?? false,
      })),
    );
}

/** Counts the sign-ins the page asked the server for. */
function countPasskeySignins(page: Page) {
  let count = 0;
  page.on('request', (request) => {
    if (request.method() === 'POST' && request.url().endsWith('/api/v1/auth/signin/passkey')) {
      count += 1;
    }
  });
  return () => count;
}

async function instanceAllowsPasskeySignin(page: Page, allowed: boolean) {
  await page.route('**/api/v1/instance', (route) =>
    route.fulfill({
      json: {
        externalUrl: '',
        signupEnabled: false,
        passkeySignin: allowed,
        version: '26.0.0.0',
      },
    }),
  );
}

/** Adds the mock user's first passkey: it turns multi-factor authentication on. */
async function addFirstPasskey(page: Page, name: string) {
  await page.goto('/account/settings/security');
  const section = page.getByTestId('passkeys');
  await section.getByRole('button', { name: 'Add a passkey' }).click();

  const confirm = page.getByTestId('passkey-confirm-dialog');
  await confirm.getByTestId('mfa-password-input').fill(MOCK_PASSWORD);
  await confirm.getByRole('button', { name: 'Continue' }).click();

  const add = page.getByTestId('passkey-add-dialog');
  await add.getByTestId('passkey-name-input').fill(name);
  await add.getByRole('button', { name: 'Create' }).click();

  const codes = page.getByTestId('recovery-codes-dialog');
  await expect(codes.getByRole('listitem')).toHaveCount(10);
  await expect(codes).toContainText(MOCK_RECOVERY_CODES[0]!);
  await codes.getByRole('button', { name: 'I’ve saved them' }).click();

  await expect(section.getByRole('listitem').first()).toContainText(name);
}

async function signOut(page: Page) {
  await page.goto('/account/settings/sessions');
  await page.getByRole('button', { name: 'Sign out this device' }).click();
  await expect(page).toHaveURL(/\/signin/);
}

test.describe('passkeys', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('a first passkey is resident, gives recovery codes and says where it is kept', async ({
    page,
  }) => {
    const authenticator = await addVirtualAuthenticator(page);
    await addFirstPasskey(page, 'My MacBook');

    const credentials = await authenticator.credentials();
    expect(credentials).toHaveLength(1);
    expect(credentials[0]!.isResidentCredential).toBe(true);

    const row = page.getByTestId('passkeys').getByRole('listitem').first();
    await expect(row).toContainText('iCloud Keychain');
    await expect(row).toContainText('Not used yet');
    await expect(page.getByTestId('recovery-codes-row')).toContainText('10 codes left');
  });

  test('a passkey without a name goes by where it is kept, and can be renamed', async ({
    page,
  }) => {
    await addVirtualAuthenticator(page);
    await addFirstPasskey(page, 'My MacBook');

    const row = page.getByTestId('passkeys').getByRole('listitem').first();
    await row.getByRole('button', { name: 'Rename' }).click();
    const rename = page.getByTestId('passkey-rename-dialog');
    await rename.getByRole('textbox').fill('');
    await rename.getByRole('button', { name: 'Save' }).click();
    await expect(rename).toBeHidden();
    await expect(row.locator('div.text-sm')).toHaveText('iCloud Keychain');

    await row.getByRole('button', { name: 'Rename' }).click();
    await rename.getByRole('textbox').fill('Work laptop');
    await rename.getByRole('button', { name: 'Save' }).click();
    await expect(row.locator('div.text-sm')).toHaveText('Work laptop');
  });

  test('the username field offers the passkey and signs in without a password', async ({
    page,
  }) => {
    const signins = countPasskeySignins(page);
    await addVirtualAuthenticator(page);
    await addFirstPasskey(page, 'My MacBook');
    await instanceAllowsPasskeySignin(page, true);

    // A virtual authenticator answers the request behind the field by itself, as a
    // person does by picking the suggestion: nothing else on the page is pressed. The
    // sign-in page may be gone again before a check of the address could see it.
    const signedIn = page.waitForResponse((response) =>
      response.url().endsWith('/api/v1/auth/signin/passkey'),
    );
    await signOut(page);
    await page.getByRole('button', { name: 'Sign in with email' }).click();
    expect((await signedIn).status()).toBe(200);
    await expect(page).not.toHaveURL(/\/signin/);

    // Once signed in, no new request waits behind the field to sign in a second time.
    await page.waitForTimeout(500);
    expect(signins()).toBe(1);

    // The next visit points at the passkey.
    expect(await page.evaluate(() => localStorage.getItem('molesignal.signin.last'))).toBe(
      '{"channel":{"kind":"passkey"},"account":"dev@molesignal.local"}',
    );
  });

  test('the button signs in where the browser has no such suggestions', async ({ page }) => {
    await page.addInitScript(() => {
      PublicKeyCredential.isConditionalMediationAvailable = () => Promise.resolve(false);
    });
    await addVirtualAuthenticator(page);
    await addFirstPasskey(page, 'My MacBook');
    await instanceAllowsPasskeySignin(page, true);
    await signOut(page);

    await page.getByRole('button', { name: 'Sign in with email' }).click();
    await expect(page.getByLabel('Email')).toHaveAttribute('autocomplete', 'username webauthn');
    await page.waitForTimeout(500);
    await expect(page).toHaveURL(/\/signin/);

    await page.getByRole('button', { name: 'Back' }).click();
    await page.getByTestId('signin-passkey').click();
    await expect(page).not.toHaveURL(/\/signin/);
  });

  test('the request behind the field goes with the field, and the list asks on its own', async ({
    page,
  }) => {
    const requests = await watchPasskeyRequests(page);
    // No passkey yet: the request behind the field stays open, and the button's finds none.
    const authenticator = await addVirtualAuthenticator(page);
    await authenticator.refuse();
    await instanceAllowsPasskeySignin(page, true);
    await signOut(page);

    // The browser wants the field on the page before it is asked: not on the list yet.
    await expect(page.getByTestId('signin-passkey')).toBeEnabled();
    expect(await requests()).toEqual([]);

    await page.getByRole('button', { name: 'Sign in with email' }).click();
    await expect.poll(requests).toEqual([{ mediation: 'conditional', aborted: false }]);

    await page.getByRole('button', { name: 'Back' }).click();
    await expect.poll(requests).toEqual([{ mediation: 'conditional', aborted: true }]);

    await page.getByTestId('signin-passkey').click();
    await expect
      .poll(async () => (await requests()).map((request) => request.mediation))
      .toEqual(['conditional', null]);

    // Nothing was wrong, so nothing is said, and the page still offers both ways.
    await expect(page.getByText('Sign-in failed')).toHaveCount(0);
    await expect(page.getByTestId('signin-passkey')).toBeEnabled();
  });

  test('where only single sign-on enters, the field says so and goes back to the providers', async ({
    page,
    mockServer,
  }) => {
    await addVirtualAuthenticator(page);
    await addFirstPasskey(page, 'My MacBook');
    await instanceAllowsPasskeySignin(page, true);
    await fetch(`http://127.0.0.1:${mockServer.port}/api/v1/settings/signin-policy`, {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ requireMfa: false, ssoOnly: true }),
    });
    await signOut(page);

    await page.getByRole('button', { name: 'Sign in with email' }).click();

    await expect(
      page.getByText('Your workspace only allows single sign-on. Sign in with a provider below.'),
    ).toBeVisible();
    await expect(page.getByRole('button', { name: 'Continue with Okta' })).toBeVisible();
    await expect(page.getByLabel('Email')).toHaveCount(0);
  });

  test('a passkey finishes the second step after the password', async ({ page }) => {
    await addVirtualAuthenticator(page);
    await addFirstPasskey(page, 'My MacBook');
    await instanceAllowsPasskeySignin(page, false);
    await signOut(page);

    await expect(page.getByRole('button', { name: 'Sign in with email' })).toBeVisible();
    await expect(page.getByTestId('signin-passkey')).toHaveCount(0);
    await signInWithPassword(page, 'dev@molesignal.local', MOCK_PASSWORD);

    const panel = page.getByTestId('mfa-panel');
    await expect(panel).toBeVisible();
    await expect(page.getByText('Verify it’s you with your passkey.')).toBeVisible();
    // This account has no authenticator app to fall back on.
    await expect(panel.getByRole('button', { name: 'Use the authenticator app' })).toHaveCount(0);
    await expect(panel.getByRole('button', { name: 'Use a recovery code' })).toBeVisible();

    await panel.getByRole('button', { name: 'Verify with a passkey' }).click();
    await expect(page).not.toHaveURL(/\/signin/);
  });

  test('deleting the last passkey is confirmed with a passkey and turns the rest off', async ({
    page,
  }) => {
    await addVirtualAuthenticator(page);
    await addFirstPasskey(page, 'My MacBook');

    const section = page.getByTestId('passkeys');
    await section.getByRole('listitem').first().getByRole('button', { name: 'Delete' }).click();

    const dialog = page.getByTestId('passkey-remove-dialog');
    await expect(dialog).toContainText('Delete the passkey “My MacBook”?');
    await expect(dialog).toContainText('turns multi-factor authentication off');
    await dialog.getByRole('button', { name: 'Delete' }).click();

    await expect(page.getByText('Passkey deleted')).toBeVisible();
    await expect(section.getByRole('listitem')).toHaveCount(0);
    await expect(page.getByTestId('recovery-codes-row')).toHaveCount(0);
  });

  test('backing out of the browser prompt leaves the dialog open and says nothing', async ({
    page,
  }) => {
    const authenticator = await addVirtualAuthenticator(page);
    await authenticator.refuse();

    await page.goto('/account/settings/security');
    await page.getByTestId('passkeys').getByRole('button', { name: 'Add a passkey' }).click();
    const confirm = page.getByTestId('passkey-confirm-dialog');
    await confirm.getByTestId('mfa-password-input').fill(MOCK_PASSWORD);
    await confirm.getByRole('button', { name: 'Continue' }).click();

    const add = page.getByTestId('passkey-add-dialog');
    await add.getByRole('button', { name: 'Create' }).click();
    await expect(add.getByRole('button', { name: 'Create' })).toBeEnabled({ timeout: 15_000 });
    await expect(add).toBeVisible();
    await expect(page.getByText('Couldn’t finish that')).toHaveCount(0);
  });

  test('the button says why passkeys are unavailable on this instance', async ({ page }) => {
    await page.route('**/api/v1/me/mfa', (route) =>
      route.fulfill({
        json: {
          totpEnabled: false,
          totpEnabledAt: null,
          recoveryCodesLeft: 0,
          passkeys: [],
          passkeysUnavailable: 'not_configured',
          addingRequires: 'password',
          requiredBy: [],
        },
      }),
    );

    await page.goto('/account/settings/security');
    await expect(
      page.getByTestId('passkeys').getByRole('button', { name: 'Add a passkey' }),
    ).toBeDisabled();
  });
});
