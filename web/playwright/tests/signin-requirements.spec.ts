import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_PASSWORD, MOCK_RECOVERY_CODES, MOCK_TOTP_CODE } from '../fixtures/mockMfa';
import { signInWithPassword } from '../fixtures/signinFlow';

async function requires(
  port: number,
  policy: { requireMfa?: boolean; ssoOnly?: boolean },
) {
  await fetch(`http://127.0.0.1:${port}/api/v1/settings/signin-policy`, {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ requireMfa: false, ssoOnly: false, ...policy }),
  });
}

async function signOutAndEnterPassword(page: Page) {
  await page.goto('/account/settings/sessions');
  await page.getByRole('button', { name: 'Sign out this device' }).click();
  await expect(page).toHaveURL(/\/signin/);

  await signInWithPassword(page, 'dev@molesignal.local', MOCK_PASSWORD);
}

async function addVirtualAuthenticator(page: Page) {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('WebAuthn.enable');
  await cdp.send('WebAuthn.addVirtualAuthenticator', {
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
}

/** The answer of `POST /orgs/acme-staging/select` until `then` lets the sign-in in. */
async function switchingAnswers(page: Page, waiting: object) {
  const proofs: unknown[] = [];

  await page.route('**/api/v1/orgs/acme-staging/select', (route) => {
    const proof = route.request().postDataJSON()?.proof;

    if (!proof) {
      void route.fulfill({ json: waiting });
      return;
    }

    proofs.push(proof);
    void route.fulfill({
      json: {
        userId: 'u1',
        orgId: 'acme-staging',
        orgName: 'acme-staging',
        displayRole: 'Admin',
        roles: [{ id: 'role-admin', key: 'admin', name: 'Admin', builtin: true }],
        system: false,
      },
    });
  });

  return proofs;
}

async function switchToStaging(page: Page) {
  await page.goto('/account/settings/profile');
  await page.getByTestId('org-status-trigger').click();
  await page
    .locator("[data-ui='org-status-menu'] [data-ui='workspace-option']")
    .filter({ hasText: 'acme-staging' })
    .click();
}

test.describe('sign-in requirements', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('a requirement is turned on after asking and off without', async ({ page }) => {
    await page.goto('/iam/sso');

    const mfa = page.getByTestId('signin-requirement-mfa');
    await expect(mfa).toContainText('Require multi-factor authentication');
    const toggle = mfa.getByRole('switch');
    await expect(toggle).not.toBeChecked();

    await toggle.click();
    const confirm = page.getByRole('dialog');
    await expect(confirm).toContainText('Require multi-factor authentication?');
    await confirm.getByRole('button', { name: 'Cancel' }).click();
    await expect(toggle).not.toBeChecked();

    await toggle.click();
    await page.getByRole('dialog').getByRole('button', { name: 'Turn on' }).click();
    await expect(page.getByText('Sign-in requirements saved')).toBeVisible();
    await expect(toggle).toBeChecked();

    await toggle.click();
    await expect(toggle).not.toBeChecked();
    await expect(page.getByRole('dialog')).toHaveCount(0);
  });

  test('single sign-on only needs a provider that can sign in', async ({ page }) => {
    await page.route('**/api/v1/sso/providers', (route) => route.fulfill({ json: [] }));
    await page.goto('/iam/sso');

    const sso = page.getByTestId('signin-requirement-sso');
    await expect(sso).toContainText('Enable an OIDC, OAuth 2.0 or SAML provider first.');
    await expect(sso.getByRole('switch')).toBeDisabled();
  });

  test('turning on single sign-on says who it signs out', async ({ page }) => {
    await page.goto('/iam/sso');

    const sso = page.getByTestId('signin-requirement-sso');
    await sso.getByRole('switch').click();
    const confirm = page.getByRole('dialog');
    await expect(confirm).toContainText('Only allow single sign-on?');
    await expect(confirm).toContainText('database clients can no longer connect');
    await confirm.getByRole('button', { name: 'Turn on' }).click();
    await expect(sso.getByRole('switch')).toBeChecked();
  });

  test('the last provider of a single sign-on only workspace stays, and says so', async ({
    page,
  }) => {
    await page.route('**/api/v1/sso/providers/okta/disable', (route) =>
      route.fulfill({
        status: 409,
        json: {
          error: 'conflict',
          message:
            'conflict: this organization only allows single sign-on and this is its last provider: allow other sign-ins first',
        },
      }),
    );
    await page.goto('/iam/sso');

    await page.getByRole('row', { name: /Okta/ }).getByRole('button', { name: 'Disable' }).click();
    await expect(page.getByText('this is its last working provider', { exact: false })).toBeVisible();
  });

  test('a password sign-in sets up an authenticator on the spot, then enters', async ({
    page,
    mockServer,
  }) => {
    await requires(mockServer.port, { requireMfa: true });
    await signOutAndEnterPassword(page);

    const panel = page.getByTestId('mfa-enroll-panel');
    await expect(panel).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Turn on multi-factor authentication' })).toBeVisible();
    await expect(page.getByText('“acme-prod” asks for multi-factor authentication.')).toBeVisible();

    await panel.getByRole('button', { name: 'Use an authenticator app' }).click();
    await expect(panel.getByTestId('mfa-qr')).toBeVisible();

    const code = panel.getByLabel('Verification code');
    await code.fill('000000');
    await panel.getByRole('button', { name: /Turn on and sign in/ }).click();
    await expect(page.getByText('That code isn’t right')).toBeVisible();
    await expect(code).toHaveValue('');

    await code.fill(MOCK_TOTP_CODE);
    await code.press('Enter');

    // The session has started, and the codes are shown before the page moves on.
    const codes = page.getByTestId('recovery-codes-dialog');
    await expect(codes).toContainText(MOCK_RECOVERY_CODES[0]!);
    await expect(page).toHaveURL(/\/signin/);
    await codes.getByRole('button', { name: 'I’ve saved them' }).click();
    await expect(page).not.toHaveURL(/\/signin/);
  });

  test('a password sign-in can set up a passkey instead', async ({ page, mockServer }) => {
    await addVirtualAuthenticator(page);
    await requires(mockServer.port, { requireMfa: true });
    await signOutAndEnterPassword(page);

    const panel = page.getByTestId('mfa-enroll-panel');
    await panel.getByRole('button', { name: 'Use a passkey' }).click();
    await panel.getByRole('button', { name: 'Create a passkey and sign in' }).click();

    const codes = page.getByTestId('recovery-codes-dialog');
    await expect(codes.getByRole('listitem')).toHaveCount(10);
    await codes.getByRole('button', { name: 'I’ve saved them' }).click();
    await expect(page).not.toHaveURL(/\/signin/);
  });

  test('another way can be chosen, and a sign-in that ended goes back to the start', async ({
    page,
    mockServer,
  }) => {
    await requires(mockServer.port, { requireMfa: true });
    await signOutAndEnterPassword(page);

    const panel = page.getByTestId('mfa-enroll-panel');
    await panel.getByRole('button', { name: 'Use an authenticator app' }).click();
    await panel.getByRole('button', { name: 'Choose another way' }).click();
    await expect(panel.getByRole('button', { name: 'Use a passkey' })).toBeVisible();

    await page.route('**/api/v1/auth/signin/enroll/totp/confirm', (route) =>
      route.fulfill({
        status: 401,
        json: {
          error: 'unauthorized',
          message: 'unauthorized: this sign-in has expired; sign in again',
        },
      }),
    );
    // The same key comes back: it may already be in their app.
    await panel.getByRole('button', { name: 'Use an authenticator app' }).click();
    await panel.getByLabel('Verification code').fill(MOCK_TOTP_CODE);
    await panel.getByRole('button', { name: /Turn on and sign in/ }).click();

    await expect(page.getByText('This sign-in has expired. Sign in again.')).toBeVisible();
    await expect(page.getByLabel('Password', { exact: true })).toBeVisible();
  });

  test('a password sign-in says why when only single sign-on enters', async ({
    page,
    mockServer,
  }) => {
    await requires(mockServer.port, { ssoOnly: true });
    await signOutAndEnterPassword(page);

    await expect(
      page.getByText('Your workspace only allows single sign-on. Sign in with a provider below.'),
    ).toBeVisible();
    await expect(page).toHaveURL(/\/signin/);
    // The password step has no provider on it: the page goes back to where they are.
    await expect(page.getByRole('button', { name: 'Continue with Okta' })).toBeVisible();
  });

  test('the last second factor stays while a workspace asks for one', async ({
    page,
    mockServer,
  }) => {
    await page.goto('/account/settings/security');
    await page.getByTestId('mfa-row').getByRole('button', { name: 'Turn on' }).click();
    const confirm = page.getByTestId('mfa-confirm-dialog');
    await confirm.getByTestId('mfa-password-input').fill(MOCK_PASSWORD);
    await confirm.getByRole('button', { name: 'Continue' }).click();
    const setup = page.getByTestId('mfa-setup-dialog');
    await setup.getByTestId('mfa-code-input').fill(MOCK_TOTP_CODE);
    await setup.getByRole('button', { name: 'Turn on' }).click();
    await page.getByRole('button', { name: 'I’ve saved them' }).click();

    await expect(
      page.getByTestId('mfa-row').getByRole('button', { name: 'Turn off' }),
    ).toBeEnabled();

    await requires(mockServer.port, { requireMfa: true });
    await page.reload();
    await expect(
      page.getByTestId('mfa-row').getByRole('button', { name: 'Turn off' }),
    ).toBeDisabled();
  });

  test('entering a workspace can ask for a second factor on the spot', async ({ page }) => {
    const proofs = await switchingAnswers(page, {
      mfa: { organizationName: 'acme-staging', methods: ['totp', 'recovery_code'] },
    });
    await switchToStaging(page);

    const dialog = page.getByTestId('org-entry-proof');
    await expect(dialog).toContainText('Verify it’s you to enter “acme-staging”');
    await dialog.getByTestId('mfa-code-input').fill(MOCK_TOTP_CODE);
    await dialog.getByRole('button', { name: 'Verify and enter' }).click();

    await expect(dialog).toBeHidden();
    await expect(page.getByTestId('org-status-trigger')).toContainText('acme-staging');
    expect(proofs).toEqual([{ method: 'totp', code: MOCK_TOTP_CODE }]);
  });

  test('backing out of entering a workspace says nothing and stays', async ({ page }) => {
    await switchingAnswers(page, {
      mfa: { organizationName: 'acme-staging', methods: ['totp', 'recovery_code'] },
    });
    await switchToStaging(page);

    const dialog = page.getByTestId('org-entry-proof');
    await dialog.getByRole('button', { name: 'Cancel' }).click();

    await expect(dialog).toBeHidden();
    await expect(page.getByTestId('org-status-trigger')).toContainText('acme-prod');
    await expect(page.getByText(/Couldn’t switch|Failed to switch/)).toHaveCount(0);
  });

  test('entering a workspace can send the account to set up a second factor', async ({ page }) => {
    await switchingAnswers(page, { mfaSetup: { organizationName: 'acme-staging' } });
    await switchToStaging(page);

    const dialog = page.getByTestId('org-entry-setup');
    await expect(dialog).toContainText('“acme-staging” asks for multi-factor authentication');
    await dialog.getByRole('button', { name: 'Turn it on' }).click();

    await expect(page).toHaveURL(/\/account\/settings\/security/);
    await expect(dialog).toBeHidden();
  });

  test('entering a workspace can offer only its own single sign-on', async ({ page }) => {
    await switchingAnswers(page, {
      sso: {
        organizationName: 'acme-staging',
        providers: [{ id: 'okta', name: 'Okta', kind: 'oidc', platform: 'okta' }],
      },
    });
    await switchToStaging(page);

    const dialog = page.getByTestId('org-entry-sso');
    await expect(dialog).toContainText('“acme-staging” only allows single sign-on');
    await expect(dialog.getByRole('button', { name: 'Okta' })).toBeVisible();
    await dialog.getByRole('button', { name: 'Cancel' }).click();
    await expect(page.getByTestId('org-status-trigger')).toContainText('acme-prod');
  });

  test('going through its provider is the way in the next visit points at', async ({ page }) => {
    await switchingAnswers(page, {
      sso: {
        organizationName: 'acme-staging',
        providers: [{ id: 'okta', name: 'Okta', kind: 'oidc', platform: 'okta' }],
      },
    });
    await page.route('**/api/v1/auth/sso/login*', (route) =>
      route.fulfill({ contentType: 'text/html', body: '' }),
    );
    await switchToStaging(page);

    await page.getByTestId('org-entry-sso').getByRole('button', { name: 'Okta' }).click();
    await page.waitForURL(/\/auth\/sso\/login/);
    expect(await page.evaluate(() => sessionStorage.getItem('molesignal.sso.provider'))).toBe('okta');
  });
});

test('the platform organization can only ask for multi-factor authentication', async ({
  page,
  mockServer,
}) => {
  await mountMockRoutes(page, mockServer.port, {
    scope: 'system',
    orgId: 'system-org',
    orgName: '_sys',
    platformPermissions: ['sys.settings.signin.read', 'sys.settings.signin.manage'],
    capabilityPermissions: ['sys.settings.signin.read', 'sys.settings.signin.manage'],
  });
  await page.goto('/settings/signin_requirements');

  await expect(page.getByTestId('signin-requirement-mfa')).toContainText(
    'Entering the platform organization takes multi-factor authentication.',
  );
  await expect(page.getByTestId('signin-requirement-sso')).toHaveCount(0);

  await page.getByTestId('signin-requirement-mfa').getByRole('switch').click();
  await page.getByRole('dialog').getByRole('button', { name: 'Turn on' }).click();
  await expect(page.getByTestId('signin-requirement-mfa').getByRole('switch')).toBeChecked();
});

test('signing up where a workspace asks for a second factor goes on to sign in', async ({
  page,
  mockServer,
}) => {
  await mountMockRoutes(page, mockServer.port, { signedOut: true });
  await page.route('**/api/v1/auth/signup/verify', (route) =>
    route.fulfill({ json: { email: 'new@example.com' } }),
  );
  await page.route('**/api/v1/auth/signup/complete', (route) =>
    route.fulfill({
      json: {
        status: 'signin_required',
        userId: 'u9',
        email: 'new@example.com',
        displayName: 'New',
        orgId: null,
        orgName: null,
        displayRole: null,
        roles: [],
      },
    }),
  );
  await page.goto('/signup#signupToken=from-the-email');
  await page.getByLabel(/name/i).first().fill('New');
  await page.getByLabel('Password', { exact: true }).fill(MOCK_PASSWORD);
  await page.getByRole('button', { name: /Create account|Sign up/i }).click();

  await expect(page.getByText('Account created')).toBeVisible();
  await expect(page).toHaveURL(/\/signin/);
});
