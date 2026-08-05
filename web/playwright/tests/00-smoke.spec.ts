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
    await page.keyboard.press('Meta+K');
    await expect(page.getByPlaceholder(/search commands/i)).toBeVisible();
    await expect(page.getByText(/Go to APM Services/i)).toBeVisible();
  });

  test('time picker opens on modified shortcut', async ({ page }) => {
    await page.goto('/investigate');
    await page.keyboard.press('Meta+Alt+E');
    await expect(page.getByText(/Time range/i)).toBeVisible();
  });

  test('help overlay opens on modified shortcut', async ({ page }) => {
    await page.goto('/investigate');
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
        external_url: '',
        signup_enabled: false,
        version: '26.0.0.0',
        release_channel: 'alpha',
      },
    }),
  );
  await page.route('**/api/v1/iam/capabilities', (route) =>
    route.fulfill({
      json: {
        organization_id: 'acme-prod',
        scope: 'organization',
        display_role: 'Owner',
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
        route_catalog_version: 1,
        routes: [
          {
            id: 'root',
            path_pattern: '/',
            allowed: true,
          },
          {
            id: 'investigate',
            path_pattern: '/investigate',
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
        user_id: 'test-user',
        email: 'test@molesignal.local',
        display_name: 'Test User',
        org_id: 'acme-prod',
        org_name: 'acme-prod',
        org_slug: 'acme-prod',
        display_role: 'Owner',
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
        build_epoch_secs: 1_785_087_406,
        build_id: 'playwright-smoke',
        release_channel: 'alpha',
        edition: 'community',
      },
    }),
  );
  await page.route('**/api/v1/auth/signin', (route) =>
    route.fulfill({
      json: {
        token: 'mock-signin-token',
        user_id: 'test-user',
        email: 'test@molesignal.local',
        display_name: 'Test User',
        org_id: 'acme-prod',
        org_name: 'acme-prod',
        display_role: 'Owner',
        roles: [
          {
            id: 'role-owner',
            key: 'owner',
            name: 'Owner',
            builtin: true,
          },
        ],
      },
    }),
  );
  await page.goto('/login?next=/investigate');

  await expect(page.getByRole('button', { name: 'Sign in' })).toBeVisible();
  await expect(page.getByText(/continue offline|offline dev/i)).toHaveCount(0);
  await page.getByLabel('Email').fill('test@molesignal.local');
  await page.getByLabel('Password').fill('password');
  await page.getByRole('button', { name: 'Sign in' }).click();
  await expect(page).toHaveURL(/\/investigate$/);
});
