import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_PASSWORD, MOCK_RECOVERY_CODES, MOCK_TOTP_CODE } from '../fixtures/mockMfa';
import { signInWithPassword } from '../fixtures/signinFlow';

async function turnOnAuthenticator(page: Page) {
  await page.goto('/account/settings/security');
  const row = page.getByTestId('mfa-row');
  await expect(row).toContainText('Authenticator app');
  await row.getByRole('button', { name: 'Turn on' }).click();

  // A session alone doesn't get to add a second factor: the password comes first.
  const confirm = page.getByTestId('mfa-confirm-dialog');
  await confirm.getByTestId('mfa-password-input').fill('not the password');
  await confirm.getByRole('button', { name: 'Continue' }).click();
  await expect(page.getByText('That password isn’t right')).toBeVisible();
  await expect(confirm.getByTestId('mfa-password-input')).toHaveValue('');
  await confirm.getByTestId('mfa-password-input').fill(MOCK_PASSWORD);
  await confirm.getByRole('button', { name: 'Continue' }).click();

  const setup = page.getByTestId('mfa-setup-dialog');
  await expect(setup.getByTestId('mfa-qr')).toBeVisible();
  await expect(setup).toContainText('JBSW Y3DP');

  await setup.getByTestId('mfa-code-input').fill('000000');
  await setup.getByRole('button', { name: 'Turn on' }).click();
  await expect(page.getByText('That code isn’t right')).toBeVisible();
  // A wrong code empties the field for the next try.
  await expect(setup.getByTestId('mfa-code-input')).toHaveValue('');

  await setup.getByTestId('mfa-code-input').fill(MOCK_TOTP_CODE);
  await setup.getByRole('button', { name: 'Turn on' }).click();

  const codes = page.getByTestId('recovery-codes-dialog');
  await expect(codes.getByRole('listitem')).toHaveCount(10);
  await expect(codes).toContainText(MOCK_RECOVERY_CODES[0]!);
  // Shown once: Escape does not dismiss them, only the explicit button does.
  await page.keyboard.press('Escape');
  await expect(codes).toBeVisible();
  await codes.getByRole('button', { name: 'I’ve saved them' }).click();
  await expect(codes).toBeHidden();

  await expect(row).toContainText('On');
  await expect(page.getByTestId('recovery-codes-row')).toContainText('10 codes left');
}

async function signOutAndEnterPassword(page: Page) {
  await page.goto('/account/settings/sessions');
  await page.getByRole('button', { name: 'Sign out this device' }).click();
  await expect(page).toHaveURL(/\/signin/);

  await signInWithPassword(page, 'dev@molesignal.local', MOCK_PASSWORD);
  await expect(page.getByTestId('mfa-panel')).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Verify it’s you' })).toBeVisible();
}

test.describe('multi-factor authentication', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('an authenticator code finishes the sign-in once it is turned on', async ({ page }) => {
    await turnOnAuthenticator(page);
    await signOutAndEnterPassword(page);

    // Six digits are sent as soon as they are in; a wrong code is said and cleared.
    const code = page.getByLabel('Verification code');
    await code.fill('111111');
    await expect(page.getByText('That code isn’t right')).toBeVisible();
    await expect(code).toHaveValue('');

    await code.fill(MOCK_TOTP_CODE);
    await expect(page).not.toHaveURL(/\/signin/);
  });

  test('a recovery code signs in once and says how many are left', async ({ page }) => {
    await turnOnAuthenticator(page);
    await signOutAndEnterPassword(page);

    await page.getByRole('button', { name: 'Use a recovery code' }).click();
    await expect(page.getByText('Enter a recovery code you haven’t used yet.')).toBeVisible();
    await page.getByLabel('Recovery code').fill(MOCK_RECOVERY_CODES[1]!.toUpperCase());
    await page.getByRole('button', { name: /^Verify and sign in/ }).click();

    await expect(page.getByText('Recovery code used. 9 codes left.')).toBeVisible();
    await expect(page).not.toHaveURL(/\/signin/);
  });

  test('turning it off asks for a code first', async ({ page }) => {
    await turnOnAuthenticator(page);

    await page.getByTestId('mfa-row').getByRole('button', { name: 'Turn off' }).click();
    const dialog = page.getByTestId('mfa-disable-dialog');
    await expect(dialog.getByRole('button', { name: 'Turn off' })).toBeDisabled();
    await dialog.getByTestId('mfa-code-input').fill(MOCK_TOTP_CODE);
    await dialog.getByRole('button', { name: 'Turn off' }).click();

    await expect(page.getByText('Authenticator app is off')).toBeVisible();
    await expect(page.getByTestId('mfa-row').getByRole('button', { name: 'Turn on' })).toBeVisible();
    await expect(page.getByTestId('recovery-codes-row')).toHaveCount(0);
  });

  test('other devices can be signed out from the sessions page', async ({ page }) => {
    await page.goto('/account/settings/sessions');
    await page.getByTestId('sign-out-others').click();
    await expect(page.getByText('Your other devices have been signed out')).toBeVisible();
    // This browser stays signed in.
    await expect(page).toHaveURL(/\/account\/settings\/sessions/);
  });

  test('an administrator sees who has it on and can reset it', async ({ page }) => {
    await page.goto('/iam/users');
    const kenji = page.getByRole('row', { name: /Kenji Watanabe/ });
    await expect(kenji).toContainText('On');
    await expect(page.getByRole('row', { name: /Amara Okafor/ })).toContainText('Off');

    await page.getByRole('button', { name: 'Open actions for Kenji Watanabe' }).click();
    await page.getByRole('menuitem', { name: 'Reset multi-factor authentication' }).click();
    const confirm = page.getByRole('alertdialog').or(page.getByRole('dialog'));
    await expect(confirm).toContainText('Reset multi-factor authentication for “Kenji Watanabe”?');
    await confirm.getByRole('button', { name: 'Reset multi-factor authentication' }).click();

    await expect(
      page.getByText('Multi-factor authentication reset for “Kenji Watanabe”'),
    ).toBeVisible();
  });
});
