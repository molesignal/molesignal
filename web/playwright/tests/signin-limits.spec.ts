import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import { MOCK_PASSWORD } from '../fixtures/mockMfa';
import { signInWithPassword } from '../fixtures/signinFlow';

function refuseFor(seconds: number) {
  return {
    status: 429,
    headers: { 'retry-after': String(seconds) },
    json: {
      error: 'resource_exhausted',
      code: 'signin_limited',
      message: `too many failed password attempts; try again in ${seconds} seconds`,
    },
  };
}

async function instanceWith(page: Page, instance: Record<string, unknown>) {
  await page.route('**/api/v1/instance', (route) =>
    route.fulfill({
      json: {
        externalUrl: '',
        version: '26.0.0.0',
        signupEnabled: true,
        signupVerifiesEmail: false,
        passkeySignin: false,
        ...instance,
      },
    }),
  );
}

const PLATFORM = {
  scope: 'system' as const,
  orgId: 'system-org',
  orgName: '_sys',
  platformPermissions: ['sys.settings.signin.read', 'sys.settings.signin.manage'],
  capabilityPermissions: ['sys.settings.signin.read', 'sys.settings.signin.manage'],
};

test.describe('Too many failed passwords', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { signedOut: true });
  });

  test('says how long to wait and what else to try', async ({ page }) => {
    await page.route('**/api/v1/auth/signin', (route) => route.fulfill(refuseFor(840)));
    await page.goto('/signin');

    await signInWithPassword(page, 'dev@molesignal.local', 'wrong-password');

    await expect(
      page.getByText('Too many failed attempts. Try again in 14 minutes. You can also reset your password.'),
    ).toBeVisible();

    // The password stays, so nobody has to type it again after the wait.
    await expect(page.getByLabel('Password', { exact: true })).toHaveValue('wrong-password');
    await expect(page.getByRole('button', { name: 'Forgot password?' })).toBeVisible();
  });

  test('offers a passkey where this instance has them', async ({ page }) => {
    await instanceWith(page, { passkeySignin: true });
    await page.route('**/api/v1/auth/signin', (route) => route.fulfill(refuseFor(840)));
    await page.goto('/signin');

    await signInWithPassword(page, 'dev@molesignal.local', 'wrong-password');

    await expect(
      page.getByText(
        'Too many failed attempts. Try again in 14 minutes. You can also sign in with a passkey or reset your password.',
      ),
    ).toBeVisible();
  });

  test('says a short wait in seconds', async ({ page }) => {
    await page.route('**/api/v1/auth/signin', (route) => route.fulfill(refuseFor(42)));
    await page.goto('/signin');

    await signInWithPassword(page, 'dev@molesignal.local', 'wrong-password');

    await expect(page.getByText(/Try again in 42 seconds\./)).toBeVisible();
  });

  test('is said in the language of the page', async ({ page }) => {
    await page.addInitScript(() => {
      localStorage.setItem(
        'molesignal-ui-prefs',
        JSON.stringify({ state: { language: 'zh-cn' }, version: 0 }),
      );
    });
    await page.route('**/api/v1/auth/signin', (route) => route.fulfill(refuseFor(840)));
    await page.goto('/signin');

    await page.getByRole('button', { name: '用邮箱登录' }).click();
    await page.getByLabel('邮箱').fill('dev@molesignal.local');
    await page.getByRole('button', { name: '继续 →', exact: true }).click();
    await page.getByLabel('密码', { exact: true }).fill('wrong-password');
    await page.getByRole('button', { name: '登录 →', exact: true }).click();

    await expect(page.getByText('尝试次数太多，请 14 分钟后再试。也可以重置密码。')).toBeVisible();
  });

  test('a directory is told to wait, without a reset it cannot use', async ({ page }) => {
    await page.route('**/api/v1/auth/sso/providers', (route) =>
      route.fulfill({
        json: [{ id: 'corp', name: 'Corp LDAP', kind: 'ldap', platform: 'custom' }],
      }),
    );
    await page.route('**/api/v1/auth/sso/ldap/login', (route) => route.fulfill(refuseFor(840)));
    await page.goto('/signin');

    await page.getByRole('button', { name: 'Continue with Corp LDAP' }).click();
    await page.getByLabel('Username').fill('alice');
    await page.getByRole('button', { name: 'Continue →', exact: true }).click();
    await page.getByLabel('Password', { exact: true }).fill('directory-secret');
    await page.getByRole('button', { name: 'Sign in →', exact: true }).click();

    await expect(page.getByText('Too many failed attempts. Try again in 14 minutes.')).toBeVisible();
    await expect(page.getByText(/reset your password/)).toHaveCount(0);
  });

  test('a wrong password is still just a wrong password', async ({ page }) => {
    await page.route('**/api/v1/auth/signin', (route) =>
      route.fulfill({
        status: 401,
        json: { error: 'unauthorized', message: 'unauthorized: invalid credentials' },
      }),
    );
    await page.goto('/signin');

    await signInWithPassword(page, 'dev@molesignal.local', 'wrong-password');

    await expect(page.getByText('unauthorized: invalid credentials')).toBeVisible();
    await expect(page.getByText(/Too many failed attempts/)).toHaveCount(0);
  });
});

test.describe('Signing up on an instance that sends email', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { signedOut: true });
    await instanceWith(page, { signupVerifiesEmail: true });
  });

  test('asks only for the email and says to look there', async ({ page }) => {
    await page.route('**/api/v1/auth/signup', (route) =>
      route.fulfill({ status: 202, json: { status: 'verify_email' } }),
    );
    await page.goto('/signup');

    await expect(page.getByLabel('Name', { exact: true })).toHaveCount(0);
    await expect(page.getByLabel('Password', { exact: true })).toHaveCount(0);

    await page.getByLabel('Email').fill(' new@example.com ');

    const sent = page.waitForRequest('**/api/v1/auth/signup');
    await page.getByRole('button', { name: 'Continue →', exact: true }).click();
    const body = (await sent).postDataJSON();

    expect(body).toMatchObject({ email: 'new@example.com' });
    expect(body.locale).toBeTruthy();
    expect(body).not.toHaveProperty('password');

    await expect(page.getByRole('heading', { name: 'Check your email' })).toBeVisible();
    await expect(page.getByText(/sending a message to new@example.com/)).toBeVisible();

    await page.getByRole('button', { name: 'Back to sign-in' }).click();
    await expect(page).toHaveURL(/\/signin/);
  });

  test('coming from the sign-in page, the address is already filled in', async ({ page }) => {
    await page.route('**/api/v1/auth/signup', (route) =>
      route.fulfill({ status: 202, json: { status: 'verify_email' } }),
    );
    await page.goto('/signin');
    await page.getByRole('button', { name: 'Sign in with email' }).click();
    await page.getByLabel('Email').fill('new@example.com');
    await page.getByRole('button', { name: 'Continue →', exact: true }).click();
    await page.getByRole('button', { name: 'Sign up' }).click();

    await expect(page).toHaveURL(/\/signup$/);
    await expect(page.getByLabel('Email')).toHaveValue('new@example.com');
    await expect(page.getByLabel('Password', { exact: true })).toHaveCount(0);
  });

  test('the link asks for a name and a password, then signs in', async ({ page }) => {
    await page.route('**/api/v1/auth/signup/verify', (route) =>
      route.fulfill({ json: { email: 'new@example.com' } }),
    );
    await page.goto('/signup#signupToken=from-the-email');

    await expect(page.getByTestId('signin-account')).toHaveText('new@example.com');
    await expect(page.url()).not.toContain('signupToken');

    const submit = page.getByRole('button', { name: 'Create account →', exact: true });
    await page.getByLabel('Name', { exact: true }).fill('New');
    await page.getByLabel('Password', { exact: true }).fill('short');
    await expect(submit).toBeDisabled();
    await page.getByLabel('Password', { exact: true }).fill(MOCK_PASSWORD);

    const sent = page.waitForRequest('**/api/v1/auth/signup/complete');
    await submit.click();

    expect((await sent).postDataJSON()).toEqual({
      token: 'from-the-email',
      displayName: 'New',
      password: MOCK_PASSWORD,
    });

    await expect(page).toHaveURL(/\/home/);
  });

  test('a link that needs approval says so', async ({ page }) => {
    await page.route('**/api/v1/auth/signup/verify', (route) =>
      route.fulfill({ json: { email: 'new@example.com' } }),
    );
    await page.route('**/api/v1/auth/signup/complete', (route) =>
      route.fulfill({
        json: {
          status: 'pending',
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

    await page.getByLabel('Name', { exact: true }).fill('New');
    await page.getByLabel('Password', { exact: true }).fill(MOCK_PASSWORD);
    await page.getByRole('button', { name: 'Create account →', exact: true }).click();

    await expect(page.getByText('Registration submitted')).toBeVisible();
    await expect(page).toHaveURL(/\/signin/);
  });

  test('a link that no longer works offers to start again', async ({ page }) => {
    await page.route('**/api/v1/auth/signup/verify', (route) =>
      route.fulfill({
        status: 400,
        json: { error: 'invalid_argument', message: 'signup link is invalid or has expired' },
      }),
    );
    await page.goto('/signup#signupToken=old');

    await expect(page.getByRole('heading', { name: 'This link no longer works' })).toBeVisible();
    await expect(page.getByLabel('Password', { exact: true })).toHaveCount(0);

    await page.getByRole('button', { name: 'Start again →', exact: true }).click();

    await expect(page.getByRole('heading', { name: 'Create your account' })).toBeVisible();
    await expect(page.getByLabel('Email')).toBeVisible();
  });

  test('a link used up while the form was open offers to start again', async ({ page }) => {
    await page.route('**/api/v1/auth/signup/verify', (route) =>
      route.fulfill({ json: { email: 'new@example.com' } }),
    );
    await page.route('**/api/v1/auth/signup/complete', (route) =>
      route.fulfill({
        status: 400,
        json: { error: 'invalid_argument', message: 'signup link is invalid or has expired' },
      }),
    );
    await page.goto('/signup#signupToken=from-the-email');

    await page.getByLabel('Name', { exact: true }).fill('New');
    await page.getByLabel('Password', { exact: true }).fill(MOCK_PASSWORD);
    await page.getByRole('button', { name: 'Create account →', exact: true }).click();

    await expect(page.getByRole('heading', { name: 'This link no longer works' })).toBeVisible();
  });
});

test.describe('Signing up on an instance that cannot send email', () => {
  test('asks for everything at once and waits for approval', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, { signedOut: true });
    await instanceWith(page, { signupVerifiesEmail: false });
    await page.route('**/api/v1/auth/signup', (route) =>
      route.fulfill({ status: 202, json: { status: 'pending' } }),
    );
    await page.goto('/signup');

    await page.getByLabel('Email').fill('new@example.com');
    await page.getByRole('button', { name: 'Continue →', exact: true }).click();
    await page.getByLabel('Name', { exact: true }).fill('New');
    await page.getByLabel('Password', { exact: true }).fill(MOCK_PASSWORD);

    const sent = page.waitForRequest('**/api/v1/auth/signup');
    await page.getByRole('button', { name: 'Create account →', exact: true }).click();

    expect((await sent).postDataJSON()).toEqual({
      email: 'new@example.com',
      displayName: 'New',
      password: MOCK_PASSWORD,
    });

    await expect(page.getByText('Registration submitted')).toBeVisible();
    await expect(page).toHaveURL(/\/signin/);
  });
});

test.describe('Sign-in limit settings', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, PLATFORM);
  });

  test('reads and saves the limits', async ({ page }) => {
    await page.goto('/settings/signin_limits');

    const lock = page.getByRole('spinbutton', { name: 'Lock and count for' });
    await expect(lock).toHaveValue('15');
    await expect(page.getByRole('spinbutton', { name: 'One account from one address' })).toHaveValue('5');
    await expect(page.getByRole('spinbutton', { name: 'One address, any account' })).toHaveValue('30');

    const save = page.getByRole('button', { name: 'Save changes' });
    await expect(save).toBeDisabled();

    await lock.fill('0');
    await expect(save).toBeDisabled();
    await expect(page.getByText(/Enter whole numbers/)).toBeVisible();

    await lock.fill('30');
    const sent = page.waitForRequest(
      (request) =>
        request.url().includes('/api/v1/settings/signin_limits') && request.method() === 'PUT',
    );
    await save.click();

    expect((await sent).postDataJSON()).toEqual({
      accountSourceFailures: 5,
      sourceFailures: 30,
      accountFailures: 20,
      accountIntervalSecs: 60,
      lockMinutes: 30,
    });

    await expect(page.getByText('Sign-in limits saved')).toBeVisible();

    await page.reload();
    await expect(lock).toHaveValue('30');
  });

  test('says an address counts', async ({ page }) => {
    await page.goto('/settings/signin_limits');

    const source = page.getByTestId('signin-limits-source');
    await expect(source).toContainText('8.8.8.8');
    await expect(source).toContainText('Counted by address');
    await expect(source.getByRole('button', { name: 'Open Client IP recognition' })).toHaveCount(0);
  });

  test('warns about a proxy nobody set up and shows the way there', async ({ page }) => {
    await page.route('**/api/v1/settings/signin_limits', (route) =>
      route.fulfill({
        json: {
          accountSourceFailures: 5,
          sourceFailures: 30,
          accountFailures: 20,
          accountIntervalSecs: 60,
          lockMinutes: 15,
          source: { address: '10.0.0.1', counted: false, behindUnconfiguredProxy: true },
        },
      }),
    );
    await page.goto('/settings/signin_limits');

    const source = page.getByTestId('signin-limits-source');
    await expect(source).toContainText('Set up trusted proxies');
    await expect(page.getByText(/probably behind a proxy/)).toBeVisible();

    await source.getByRole('button', { name: 'Open Client IP recognition' }).click();
    await expect(page).toHaveURL(/\/settings\/client_ip/);
  });

  test('private addresses can be allowed in every address mode', async ({ page }) => {
    await page.goto('/settings/client_ip');

    await expect(page.getByRole('heading', { name: 'Client IP recognition' })).toBeVisible();
    await expect(page.getByRole('switch', { name: 'Count private addresses as clients' })).toBeVisible();
  });

  test('is in the settings list of the platform organization', async ({ page }) => {
    await page.goto('/settings/signin_limits');

    await expect(page.getByRole('link', { name: 'Sign-in limits' })).toBeVisible();
  });
});

test.describe('Signing up without approval', () => {
  const PLATFORM_SESSION = {
    scope: 'system' as const,
    platformPermissions: ['sys.settings.signin.read', 'sys.settings.signin.manage'],
    capabilityPermissions: ['sys.settings.signin.read', 'sys.settings.signin.manage'],
  };

  async function openPolicy(page: Page, canVerifyEmail: boolean) {
    await page.route('**/api/v1/settings/signup', (route) =>
      route.fulfill({
        json: {
          signupEnabled: true,
          signupRequireApproval: false,
          canVerifyEmail: canVerifyEmail,
        },
      }),
    );
    await page.goto('/settings/signin_requirements');
  }

  test('is not on offer while the instance cannot send email', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, PLATFORM_SESSION);
    await openPolicy(page, false);

    const approval = page.getByRole('switch', { name: 'Require admin approval' });
    await expect(approval).toBeChecked();
    await expect(approval).toBeDisabled();
    await expect(page.getByText(/can’t confirm who owns an address/)).toBeVisible();
  });

  test('is a choice once it can', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, PLATFORM_SESSION);
    await openPolicy(page, true);

    const approval = page.getByRole('switch', { name: 'Require admin approval' });
    await expect(approval).not.toBeChecked();
    await expect(approval).toBeEnabled();
  });
});
