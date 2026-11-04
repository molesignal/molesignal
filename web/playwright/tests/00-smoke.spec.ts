import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('shell smoke', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('authenticated session lands on /investigate and shows ⌘K hint', async ({ page }) => {
    await page.goto('/investigate');
    await expect(page).toHaveURL(/\/investigate/);
    await expect(page.getByText(/press/i).first()).toBeVisible();
  });

  test('palette opens on ⌘K and lists static actions', async ({ page }) => {
    await page.goto('/investigate');
    await expect(page.getByRole('button', { name: 'User menu' })).toBeVisible();
    await page.keyboard.press('Meta+K');
    await expect(page.getByPlaceholder(/search commands/i)).toBeVisible();
    await expect(page.getByText(/Go to APM Services/i)).toBeVisible();
  });

  test('time picker opens on modified shortcut', async ({ page }) => {
    await page.goto('/investigate');
    await expect(page.getByRole('button', { name: 'User menu' })).toBeVisible();
    await page.keyboard.press('Meta+Alt+E');
    await expect(page.getByText(/Time range/i)).toBeVisible();
  });

  test('help overlay opens on modified shortcut', async ({ page }) => {
    await page.goto('/investigate');
    await expect(page.getByRole('button', { name: 'User menu' })).toBeVisible();
    await page.keyboard.press('Meta+/');
    await expect(page.getByText(/Keyboard shortcuts/i)).toBeVisible();
  });
});

test('sign-in page has no development bypass and uses authenticated sign-in', async ({ page }) => {
  await page.route('**/api/v1/auth/sso/providers', (route) =>
    route.fulfill({ json: [] }),
  );
  await page.route('**/api/v1/instance', (route) =>
    route.fulfill({
      json: {
        externalUrl: '',
        signupEnabled: false,
        version: '26.0.0.0',
      },
    }),
  );
  await page.route('**/api/v1/iam/capabilities', (route) =>
    route.fulfill({
      json: {
        organizationId: 'acme-prod',
        scope: 'organization',
        displayRole: 'Owner',
        roles: [
          {
            id: 'role-owner',
            key: 'owner',
            name: 'Owner',
            builtin: true,
          },
        ],
        permissions: ['streams.read', 'streams.query'],
        features: [],
        version: 1,
        routeCatalogVersion: 1,
        routes: [
          {
            id: 'root',
            pathPattern: '/',
            allowed: true,
          },
          {
            id: 'investigate',
            pathPattern: '/investigate',
            allowed: true,
          },
        ],
      },
    }),
  );
  await page.route('**/api/v1/me/preferences', (route) =>
    route.fulfill({ json: {} }),
  );
  await page.route('**/api/v1/me/profile', (route) =>
    route.fulfill({
      json: {
        userId: 'test-user',
        email: 'test@molesignal.local',
        displayName: 'Test User',
        orgId: 'acme-prod',
        orgName: 'acme-prod',
        orgSlug: 'acme-prod',
        displayRole: 'Owner',
      },
    }),
  );
  await page.route('**/api/v1/orgs**', (route) =>
    route.fulfill({ json: [] }),
  );
  await page.route('**/api/v1/version', (route) =>
    route.fulfill({
      json: {
        version: '26.0.0.0',
        commit: 'mock-test',
        branch: 'test',
        buildEpochSecs: 1_785_087_406,
        buildId: 'playwright-smoke',
        releaseChannel: 'alpha',
        edition: 'community',
      },
    }),
  );
  await page.route('**/api/v1/auth/signin', (route) =>
    route.fulfill({
      json: {
        login: {
          userId: 'test-user',
          email: 'test@molesignal.local',
          displayName: 'Test User',
          orgId: 'acme-prod',
          orgName: 'acme-prod',
          displayRole: 'Owner',
          roles: [
            {
              id: 'role-owner',
              key: 'owner',
              name: 'Owner',
              builtin: true,
            },
          ],
        },
      },
    }),
  );
  await page.goto('/signin?next=/investigate');

  await expect(page.getByRole('button', { name: 'Continue' })).toBeVisible();
  await expect(page.getByText(/continue offline|offline dev/i)).toHaveCount(0);
  await page.getByLabel('Email').fill('test@molesignal.local');
  await page.getByRole('button', { name: 'Continue' }).click();
  await page.getByLabel('Password', { exact: true }).fill('password');
  await page.getByRole('button', { name: 'Sign in' }).click();
  await expect(page).toHaveURL(/\/investigate$/);
});
