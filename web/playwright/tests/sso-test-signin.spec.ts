import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';
import {
  MOCK_DIRECTORY_PASSWORD,
  MOCK_IDP_ORIGIN,
  type MockSsoTestOutcome,
} from '../fixtures/mockSsoTests';

/**
 * Acts out the trip to the identity provider: it ends the test on the mock, as the
 * real callback does on the server, and sends the browser to the result page.
 */
async function actOutIdentityProvider(page: Page, port: number, outcome: MockSsoTestOutcome) {
  const origin = new URL(page.url()).origin;

  await page.context().route(`${MOCK_IDP_ORIGIN}/**`, async (route) => {
    const id = new URL(route.request().url()).searchParams.get('test') ?? '';

    await fetch(`http://127.0.0.1:${port}/__mock/sso-tests/${id}/finish`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ outcome }),
    });

    await route.fulfill({
      contentType: 'text/html',
      body: `<script>location.replace(${JSON.stringify(`${origin}/iam/sso/tests/${id}`)})</script>`,
    });
  });
}

/** Clicks what starts a test and follows it into the tab it opens. */
async function testInNewTab(
  page: Page,
  port: number,
  outcome: MockSsoTestOutcome,
  start: () => Promise<void>,
) {
  let release = () => {};
  const released = new Promise<void>((resolve) => {
    release = resolve;
  });

  await actOutIdentityProvider(page, port, outcome);

  // The start waits, so the tab has its mock routes before it goes anywhere.
  await page.route('**/api/v1/sso/providers/*/tests', async (route) => {
    await released;
    await route.fallback();
  });

  const opened = page.context().waitForEvent('page');
  await start();
  const tab = await opened;
  await mountMockRoutes(tab, port);
  release();

  await expect(tab).toHaveURL(/\/iam\/sso\/tests\//);
  await tab.waitForLoadState('load');
  return tab;
}

const providerRow = (page: Page, name: RegExp | string) => page.getByRole('row', { name });

const testButton = (page: Page, name: RegExp | string) =>
  providerRow(page, name).getByRole('button', { name: 'Test', exact: true });

/** The provider list as a spec needs it: each row is the shared Okta row with what is given. */
async function serveProviders(page: Page, port: number, rows: object[]) {
  const response = await fetch(`http://127.0.0.1:${port}/api/v1/sso/providers`);
  const [okta] = (await response.json()) as object[];

  await page.route('**/api/v1/sso/providers', async (route) => {
    if (route.request().method() !== 'GET') {
      await route.fallback();
      return;
    }
    await route.fulfill({ json: rows.map((row) => ({ ...okta, ...row })) });
  });
}

const DIRECTORY_ROW = {
  id: 'directory',
  name: 'Directory',
  kind: 'ldap',
  platform: 'openldap',
  oidc: undefined,
  ldap: {
    url: 'ldaps://ldap.example.com',
    startTls: false,
    bindDn: 'cn=reader,dc=example,dc=com',
    hasBindPassword: true,
    baseDn: 'dc=example,dc=com',
    userFilter: '(mail={username})',
    fieldMapping: { subject: 'dn', email: 'mail', displayName: 'displayName', groups: 'memberOf' },
    groupRoleMapping: {},
    allowedGroups: [],
  },
  test: { status: 'untested' },
};

test.describe('a test sign-in', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('the list says how each provider stands, and opens a result that exists', async ({
    page,
    mockServer,
  }) => {
    await serveProviders(page, mockServer.port, [
      { id: 'okta', name: 'Okta', test: { status: 'passed', testId: 'sso-test-okta' } },
      { id: 'one', name: 'Failed one', test: { status: 'failed', testId: 'sso-test-okta' } },
      { id: 'two', name: 'Changed one', test: { status: 'stale', testId: 'sso-test-okta' } },
      { id: 'three', name: 'New one', test: { status: 'untested' } },
    ]);
    await page.goto('/iam/sso');

    await expect(providerRow(page, /Okta/).getByRole('link', { name: 'Passed' })).toBeVisible();
    await expect(providerRow(page, /Failed one/).getByRole('link', { name: 'Failed' })).toBeVisible();
    await expect(
      providerRow(page, /Changed one/).getByRole('link', { name: 'Settings changed' }),
    ).toBeVisible();
    await expect(providerRow(page, /New one/)).toContainText('Not tested');
    await expect(providerRow(page, /New one/).getByRole('link')).toHaveCount(0);

    await providerRow(page, /Okta/).getByRole('link', { name: 'Passed' }).click();
    await expect(page).toHaveURL(/\/iam\/sso\/tests\/sso-test-okta$/);
    await expect(page.getByTestId('sso-test-verdict')).toContainText('This provider signs in');
  });

  test('it runs in another tab, leaves this page signed in, and the list follows', async ({
    page,
    mockServer,
  }) => {
    await page.goto('/iam/sso');
    const before = providerRow(page, /Okta/).getByRole('link', { name: 'Passed' });
    await expect(before).toHaveAttribute('href', /sso-test-okta$/);

    const started = page.waitForRequest('**/api/v1/sso/providers/okta/tests');
    const tab = await testInNewTab(page, mockServer.port, 'passed', () =>
      testButton(page, /Okta/).click(),
    );
    await started;

    const result = tab.getByTestId('sso-test-result');
    await expect(result.getByTestId('sso-test-verdict')).toContainText('This provider signs in');
    await expect(result.getByTestId('sso-test-verdict')).toContainText('no session started');

    const identity = result.getByTestId('sso-test-identity');
    await expect(identity).toContainText('dev@molesignal.local');
    await expect(identity).toContainText('00u1molesignal');
    await expect(identity.getByTestId('sso-test-groups')).toContainText('sre');

    const mapping = result.getByTestId('sso-test-mapping');
    await expect(mapping).toContainText('Signs in to your account, dev@molesignal.local.');
    await expect(mapping.getByTestId('sso-test-roles')).toHaveText('Admin');
    await expect(mapping).toContainText('From the groups: sre');
    await expect(result.getByTestId('sso-test-notes')).toHaveCount(0);

    // The page that started it stays where it was, and follows to the new result.
    await expect(page).toHaveURL(/\/iam\/sso$/);
    await expect(before).not.toHaveAttribute('href', /sso-test-okta$/);
    expect(await tab.evaluate(() => window.opener)).toBeNull();
  });

  test('a blocked tab takes this page to the identity provider instead', async ({
    page,
    mockServer,
  }) => {
    await page.addInitScript(() => {
      window.open = () => null;
    });
    await page.goto('/iam/sso');
    await actOutIdentityProvider(page, mockServer.port, 'passed');

    await testButton(page, /Okta/).click();

    await expect(page).toHaveURL(/\/iam\/sso\/tests\//);
    await page.waitForLoadState('load');
    await expect(page.getByTestId('sso-test-verdict')).toContainText('This provider signs in');
  });

  test('a test that could not start closes its tab and says why', async ({ page, context }) => {
    await page.route('**/api/v1/sso/providers/okta/tests', (route) =>
      route.fulfill({
        status: 429,
        json: {
          error: 'resource_exhausted',
          message: 'resource exhausted: too many test sign-ins for this provider; wait a minute',
        },
      }),
    );
    await page.goto('/iam/sso');

    const pages = context.pages().length;
    await testButton(page, /Okta/).click();

    await expect(page.getByText('too many test sign-ins', { exact: false })).toBeVisible();
    await expect.poll(() => context.pages().length).toBe(pages);
    await expect(providerRow(page, /Okta/).getByRole('link', { name: 'Passed' })).toBeVisible();
  });

  test('closing the tab stops the waiting', async ({ page, context }) => {
    await context.route(`${MOCK_IDP_ORIGIN}/**`, (route) =>
      route.fulfill({ contentType: 'text/html', body: '<h1>The identity provider’s error page</h1>' }),
    );
    await page.goto('/iam/sso');

    const opened = context.waitForEvent('page');
    await testButton(page, /Okta/).click();
    const tab = await opened;

    await expect(providerRow(page, /Okta/)).toContainText('Waiting…');

    await tab.close();

    await expect(providerRow(page, /Okta/).getByRole('link', { name: 'Passed' })).toBeVisible();
  });

  test('a failure says what went wrong and what to do', async ({ page, mockServer }) => {
    await page.goto('/iam/sso');

    const tab = await testInNewTab(page, mockServer.port, 'token_exchange', () =>
      testButton(page, /Okta/).click(),
    );

    const verdict = tab.getByTestId('sso-test-verdict');
    await expect(verdict).toContainText('The identity provider refused the authorization code');
    await expect(verdict).toContainText('Check the client ID and client secret');
    await expect(tab.getByTestId('sso-test-detail')).toHaveText(
      'idp token endpoint 401 Unauthorized: invalid_client',
    );
    await expect(tab.getByTestId('sso-test-identity')).toHaveCount(0);
    await expect(tab.getByTestId('sso-test-mapping')).toHaveCount(0);

    await expect(providerRow(page, /Okta/).getByRole('link', { name: 'Failed' })).toBeVisible();
  });

  test('someone else’s account and changing roles are pointed out', async ({ page, mockServer }) => {
    await page.goto('/iam/sso');

    const tab = await testInNewTab(page, mockServer.port, 'another_account', () =>
      testButton(page, /Okta/).click(),
    );

    const notes = tab.getByTestId('sso-test-notes');
    await expect(notes).toContainText('None of these groups is in the role binding.');
    await expect(notes).toContainText('This isn’t the account you’re using now.');
    await expect(notes).toContainText('Signing in replaces this member’s roles');

    const mapping = tab.getByTestId('sso-test-mapping');
    await expect(mapping).toContainText('Signs in to the account someone@example.com.');
    await expect(mapping.getByTestId('sso-test-roles')).toHaveText('Admin → Viewer');
    await expect(mapping).toContainText('The provider’s default role');
  });

  test('a new account and an account waiting to be linked are told apart', async ({
    page,
    mockServer,
  }) => {
    await page.goto('/iam/sso');

    const created = await testInNewTab(page, mockServer.port, 'new_account', () =>
      testButton(page, /Okta/).click(),
    );
    await expect(created.getByTestId('sso-test-mapping')).toContainText(
      'No account has this email. The first sign-in creates new@example.com',
    );
    await expect(created.getByTestId('sso-test-identity')).toContainText('None');
    await created.close();

    await page.bringToFront();
    await page.context().unroute(`${MOCK_IDP_ORIGIN}/**`);

    const linked = await testInNewTab(page, mockServer.port, 'link', () =>
      testButton(page, /Okta/).click(),
    );
    await expect(linked.getByTestId('sso-test-mapping')).toContainText(
      'Your account dev@molesignal.local has this email',
    );
  });

  test('a page opened before the provider answers waits and then shows the result', async ({
    page,
    mockServer,
  }) => {
    const started = await fetch(`http://127.0.0.1:${mockServer.port}/api/v1/sso/providers/okta/tests`, {
      method: 'POST',
    });
    const { id } = (await started.json()) as { id: string };

    await page.goto(`/iam/sso/tests/${id}`);

    const verdict = page.getByTestId('sso-test-verdict');
    await expect(verdict).toContainText('Waiting for the identity provider');
    await expect(verdict).toContainText('redirect URI');

    await fetch(`http://127.0.0.1:${mockServer.port}/__mock/sso-tests/${id}/finish`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ outcome: 'passed' }),
    });

    await expect(verdict).toContainText('This provider signs in');
  });

  test('a test that never came back says what to check', async ({ page }) => {
    await page.route('**/api/v1/sso/tests/late', (route) =>
      route.fulfill({
        json: {
          id: 'late',
          providerId: 'okta',
          providerName: 'Okta',
          kind: 'oidc',
          platform: 'okta',
          status: 'expired',
          current: true,
          startedBy: 'Dev User',
          startedByYou: true,
          startedAt: 1_779_530_400_000_000,
        },
      }),
    );
    await page.goto('/iam/sso/tests/late');

    const verdict = page.getByTestId('sso-test-verdict');
    await expect(verdict).toContainText('No result came back');
    await expect(verdict).toContainText('within 10 minutes');
  });

  test('a directory is tested with an account typed in a dialog', async ({ page, mockServer }) => {
    await serveProviders(page, mockServer.port, [DIRECTORY_ROW]);
    await page.goto('/iam/sso');

    const pages = page.context().pages().length;
    await testButton(page, /Directory/).click();

    const dialog = page.getByTestId('sso-directory-test');
    await expect(dialog).toContainText('Test sign-in · Directory');
    await expect(dialog).toContainText('lockout policy');
    expect(page.context().pages().length).toBe(pages);

    const submit = dialog.getByRole('button', { name: 'Test', exact: true });
    await expect(submit).toBeDisabled();

    await dialog.getByLabel('Directory account').fill('dev');
    await dialog.getByLabel('Password').fill('wrong-password-1');

    const bodies: unknown[] = [];
    page.on('request', (request) => {
      if (request.url().includes('/tests/ldap')) bodies.push(request.postDataJSON());
    });

    await submit.click();

    await expect(page).toHaveURL(/\/iam\/sso\/tests\//);
    const verdict = page.getByTestId('sso-test-verdict');
    await expect(verdict).toContainText('The directory refused this password');
    await expect(page.getByTestId('sso-test-detail')).toContainText('rc=49');
    expect(page.url()).not.toContain('wrong-password');
    await expect(page.locator('body')).not.toContainText('wrong-password-1');
    expect(bodies).toEqual([{ username: 'dev', password: 'wrong-password-1' }]);

    // Testing again starts over: the password typed before is gone.
    await page.getByTestId('sso-test-again').click();
    const again = page.getByTestId('sso-directory-test');
    await expect(again.getByLabel('Password')).toHaveValue('');
    await again.getByLabel('Directory account').fill('dev');
    await again.getByLabel('Password').fill(MOCK_DIRECTORY_PASSWORD);
    await again.getByRole('button', { name: 'Test', exact: true }).click();

    await expect(verdict).toContainText('This provider signs in');
    await expect(page.getByTestId('sso-test-notes')).toContainText('second factor');
    await expect(page.getByTestId('sso-test-identity')).toContainText('uid=dev,ou=people');
  });

  test('too many wrong passwords are refused, and the dialog stays to try later', async ({
    page,
    mockServer,
  }) => {
    await serveProviders(page, mockServer.port, [DIRECTORY_ROW]);
    await page.route('**/api/v1/sso/providers/directory/tests/ldap', (route) =>
      route.fulfill({
        status: 429,
        headers: { 'retry-after': '600' },
        json: {
          error: 'resource_exhausted',
          code: 'signin_limited',
          message: 'too many sign-in attempts; try again in 10 minutes',
        },
      }),
    );
    await page.goto('/iam/sso');

    await testButton(page, /Directory/).click();
    const dialog = page.getByTestId('sso-directory-test');
    await dialog.getByLabel('Directory account').fill('dev');
    await dialog.getByLabel('Password').fill('another-guess');
    await dialog.getByRole('button', { name: 'Test', exact: true }).click();

    await expect(page.getByText('too many sign-in attempts', { exact: false })).toBeVisible();
    await expect(dialog).toBeVisible();
    await expect(page).toHaveURL(/\/iam\/sso$/);
  });

  test('closing the dialog forgets the password', async ({ page, mockServer }) => {
    await serveProviders(page, mockServer.port, [DIRECTORY_ROW]);
    await page.goto('/iam/sso');

    await testButton(page, /Directory/).click();
    const dialog = page.getByTestId('sso-directory-test');
    await dialog.getByLabel('Password').fill('typed-then-left');
    await dialog.getByRole('button', { name: 'Cancel' }).click();
    await expect(dialog).toBeHidden();

    await testButton(page, /Directory/).click();
    await expect(page.getByTestId('sso-directory-test').getByLabel('Password')).toHaveValue('');
  });

  test('saved changes void the result, and a drawer with unsaved ones won’t test', async ({ page }) => {
    await page.goto('/iam/sso');

    await providerRow(page, /Okta/).getByRole('button', { name: 'Edit' }).click();
    const row = page.getByTestId('sso-provider-test');
    await expect(row).toContainText('Passed');

    const button = row.getByRole('button', { name: 'Test', exact: true });
    await expect(button).toBeEnabled();

    await page.getByLabel('Client ID').fill('0oa1another');
    await expect(row).toContainText('Save your changes first');
    await expect(row.getByRole('button', { name: 'Test', exact: true })).toBeDisabled();

    await page.getByRole('button', { name: 'Save changes' }).click();

    const stale = providerRow(page, /Okta/).getByRole('link', { name: 'Settings changed' });
    await expect(stale).toBeVisible();

    await stale.click();
    const voided = page.getByTestId('sso-test-stale');
    await expect(voided).toContainText('This result no longer counts');
    await expect(page.getByTestId('sso-test-verdict')).toContainText('This provider signs in');
  });

  test('renaming a provider keeps its result', async ({ page }) => {
    await page.goto('/iam/sso');

    await providerRow(page, /Okta/).getByRole('button', { name: 'Edit' }).click();
    await page.getByRole('textbox', { name: 'Name', exact: true }).fill('Okta production');
    await page.getByRole('button', { name: 'Save changes' }).click();

    await expect(page.getByText('SSO provider updated', { exact: false })).toBeVisible();
    await expect(providerRow(page, /Okta/).getByRole('link', { name: 'Passed' })).toBeVisible();
  });

  test('a new provider has nothing to test until it is saved', async ({ page }) => {
    await page.goto('/iam/sso');

    await page.getByRole('button', { name: 'New provider' }).click();
    await page.getByRole('button', { name: /Okta/ }).first().click();

    await expect(page.getByTestId('sso-provider-platform')).toBeVisible();
    await expect(page.getByTestId('sso-provider-test')).toHaveCount(0);
  });
});

test('the result is in Chinese for a Chinese reader', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port, { preferences: { language: 'zh-cn' } });
  await page.goto('/iam/sso/tests/sso-test-okta');

  await expect(page.getByTestId('sso-test-verdict')).toContainText('这个服务能正常登录');
  await expect(page.getByTestId('sso-test-identity')).toContainText('身份源返回了什么');
  await expect(page.getByTestId('sso-test-mapping')).toContainText('登录到你的账号');
  await expect(page.getByRole('button', { name: '再测一次' })).toBeVisible();
});

test.describe('only allowing single sign-on', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('a provider that passed is named, and one click is enough', async ({ page }) => {
    await page.goto('/iam/sso');

    const sso = page.getByTestId('signin-requirement-sso');
    await expect(sso).not.toContainText('No provider has passed');
    await sso.getByRole('switch').click();

    const confirm = page.getByRole('dialog');
    await expect(confirm).toContainText('Only allow single sign-on?');
    await expect(confirm.locator('[data-confirm-consequences]')).toContainText('Okta: passed');
    await expect(page.getByTestId('confirm-type-name')).toHaveCount(0);

    await confirm.getByRole('button', { name: 'Turn on', exact: true }).click();
    await expect(sso.getByRole('switch')).toBeChecked();
  });

  for (const [standing, word] of [
    ['untested', 'Not tested'],
    ['failed', 'Failed'],
    ['stale', 'Settings changed'],
  ] as const) {
    test(`a provider that is “${word}” takes the workspace’s name to go ahead`, async ({
      page,
      mockServer,
    }) => {
      await serveProviders(page, mockServer.port, [
        { id: 'okta', name: 'Okta', test: { status: standing } },
      ]);
      await page.goto('/iam/sso');

      const sso = page.getByTestId('signin-requirement-sso');
      await expect(sso).toContainText('No provider has passed a test sign-in');
      await sso.getByRole('switch').click();

      const confirm = page.getByRole('dialog');
      await expect(confirm).toContainText('Only allow single sign-on without a passing test?');

      const facts = confirm.locator('[data-confirm-consequences]');
      await expect(facts).toContainText(`Okta: ${word}`);
      await expect(facts).toContainText('Only the root user can still get in');
      await expect(facts).toContainText('Database clients can no longer connect');

      const go = confirm.getByRole('button', { name: 'Turn on anyway' });
      await expect(go).toBeDisabled();
      await page.getByTestId('confirm-type-name').fill('wrong name');
      await expect(go).toBeDisabled();
      await page.getByTestId('confirm-type-name').fill('acme-prod');
      await expect(go).toBeEnabled();

      await go.click();
      await expect(sso.getByRole('switch')).toBeChecked();
    });
  }

  test('backing out of it leaves single sign-on off', async ({ page, mockServer }) => {
    await serveProviders(page, mockServer.port, [
      { id: 'okta', name: 'Okta', test: { status: 'untested' } },
    ]);
    await page.goto('/iam/sso');

    const sso = page.getByTestId('signin-requirement-sso');
    await sso.getByRole('switch').click();
    await page.getByRole('dialog').getByRole('button', { name: 'Cancel' }).click();

    await expect(sso.getByRole('switch')).not.toBeChecked();
  });

  test('one provider that passed is enough, whichever others have not', async ({
    page,
    mockServer,
  }) => {
    await serveProviders(page, mockServer.port, [
      { id: 'okta', name: 'Okta', test: { status: 'passed', finishedAt: 1_779_530_400_000_000 } },
      { id: 'other', name: 'Other', test: { status: 'failed' } },
    ]);
    await page.goto('/iam/sso');

    await page.getByTestId('signin-requirement-sso').getByRole('switch').click();

    const confirm = page.getByRole('dialog');
    await expect(confirm).toContainText('Only allow single sign-on?');
    await expect(confirm.locator('[data-confirm-consequences]')).toContainText('Okta: passed');
    await expect(page.getByTestId('confirm-type-name')).toHaveCount(0);
  });

  test('a directory doesn’t count as single sign-on', async ({ page, mockServer }) => {
    await serveProviders(page, mockServer.port, [
      { ...DIRECTORY_ROW, test: { status: 'passed', testId: 'sso-test-okta' } },
    ]);
    await page.goto('/iam/sso');

    const sso = page.getByTestId('signin-requirement-sso');
    await expect(sso).toContainText('Enable an OIDC, OAuth 2.0 or SAML provider first.');
    await expect(sso.getByRole('switch')).toBeDisabled();
  });
});

test.describe('without the right to change settings', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, {
      capabilityPermissions: ['org.sso.read'],
    });
  });

  test('the word shows, but it opens nothing and nothing can be started', async ({ page }) => {
    await page.goto('/iam/sso');

    const row = providerRow(page, /Okta/);
    await expect(row).toContainText('Passed');
    await expect(row.getByRole('link')).toHaveCount(0);
    await expect(row.getByRole('button', { name: 'Test', exact: true })).toBeDisabled();
  });

  test('a result is denied where it was asked for', async ({ page }) => {
    await page.goto('/iam/sso/tests/sso-test-okta');

    await expect(page.getByTestId('access-denied')).toBeVisible();
    await expect(page.getByTestId('sso-test-result')).toHaveCount(0);
  });
});
