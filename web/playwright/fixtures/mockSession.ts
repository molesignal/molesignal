import type { Page } from '@playwright/test';

interface MockSessionOptions {
  token?: string;
  userId?: string;
  orgId?: string;
  orgName?: string;
  role?: string;
  displayName?: string;
  email?: string;
  permissions?: string[];
}

/**
 * Seeds a normal authenticated Zustand session for browser-only tests.
 * API behavior must still be mocked explicitly by the calling spec.
 */
export async function installMockSession(
  page: Page,
  options: MockSessionOptions = {},
): Promise<void> {
  const role = options.role ?? 'Owner';
  const session = {
    state: {
      token: options.token ?? 'mock-e2e-token',
      ctx: {
        user_id: options.userId ?? 'test-user',
        org_id: options.orgId ?? 'acme-prod',
        org_name: options.orgName ?? 'acme-prod',
        display_role: role,
        roles: [
          {
            id: `role-${role.toLowerCase()}`,
            key: role.toLowerCase(),
            name: role,
            builtin: true,
          },
        ],
        display_name: options.displayName ?? 'Test User',
        email: options.email ?? 'test@molesignal.local',
      },
    },
    version: 0,
  };

  await page.addInitScript(
    ({ auth }) => {
      localStorage.setItem('molesignal-auth', JSON.stringify(auth));
    },
    { auth: session },
  );
}

/**
 * Adds the authenticated shell bootstrap endpoints used by isolated specs.
 * Feature-specific endpoints remain owned by each test.
 */
export async function installMockShellSession(
  page: Page,
  options: MockSessionOptions = {},
): Promise<void> {
  await installMockSession(page, options);

  const role = options.role ?? 'Owner';
  const orgId = options.orgId ?? 'acme-prod';
  const orgName = options.orgName ?? 'acme-prod';
  const userId = options.userId ?? 'test-user';
  const displayName = options.displayName ?? 'Test User';
  const email = options.email ?? 'test@molesignal.local';
  const permissions = options.permissions ?? [
    'streams.read',
    'streams.query',
    'alerts.read',
    'alerts.manage',
    'alerts.acknowledge',
  ];

  // Register the catch-all first. Playwright resolves the newest matching
  // route first, so the typed bootstrap and feature-specific handlers below
  // override this without letting any request escape to a real backend.
  await page.route('**/api/v1/**', (route) =>
    route.fulfill({ json: {} }),
  );
  await page.route('**/api/v1/iam/capabilities', (route) =>
    route.fulfill({
      json: {
        organization_id: orgId,
        scope: 'organization',
        display_role: role,
        roles: [
          {
            id: `role-${role.toLowerCase()}`,
            key: role.toLowerCase(),
            name: role,
            builtin: true,
          },
        ],
        permissions,
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
            id: 'account.settings',
            path_pattern: '/account/settings/*',
            allowed: true,
          },
          {
            id: 'home',
            path_pattern: '/home',
            allowed: true,
            navigation_group: 'home',
            navigation_position: 10,
          },
          {
            id: 'logs',
            path_pattern: '/logs/*',
            allowed: true,
            navigation_group: 'investigate',
            navigation_position: 20,
          },
          {
            id: 'traces',
            path_pattern: '/traces/*',
            allowed: true,
            navigation_group: 'investigate',
            navigation_position: 30,
          },
          {
            id: 'alerts',
            path_pattern: '/alerts/*',
            allowed: true,
            navigation_group: 'investigate',
            navigation_position: 40,
          },
        ],
      },
    }),
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
  await page.route('**/api/v1/version', (route) =>
    route.fulfill({
      json: {
        version: '26.0.0.0',
        commit: 'mock-test',
        branch: 'test',
        build_epoch_secs: 1_785_087_406,
        build_id: 'playwright-session',
        release_channel: 'alpha',
        edition: 'community',
      },
    }),
  );
  await page.route('**/api/v1/me/profile', (route) =>
    route.fulfill({
      json: {
        user_id: userId,
        email,
        display_name: displayName,
        org_id: orgId,
        org_name: orgName,
        org_slug: orgName,
        display_role: role,
      },
    }),
  );
  await page.route('**/api/v1/me/preferences', (route) =>
    route.fulfill({ json: {} }),
  );
  await page.route('**/api/v1/orgs**', (route) =>
    route.fulfill({ json: [] }),
  );
  await page.route('**/api/v1/iam/users**', (route) =>
    route.fulfill({ json: [] }),
  );
  await page.route('**/api/v1/alerts/incidents/*/rca**', (route) =>
    route.fulfill({ status: 404, json: { error: 'not found' } }),
  );
}
