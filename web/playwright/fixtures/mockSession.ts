import type { Page } from '@playwright/test';

interface MockSessionOptions {
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
      ctx: {
        userId: options.userId ?? 'test-user',
        orgId: options.orgId ?? 'acme-prod',
        orgName: options.orgName ?? 'acme-prod',
        displayRole: role,
        roles: [
          {
            id: `role-${role.toLowerCase()}`,
            key: role.toLowerCase(),
            name: role,
            builtin: true,
          },
        ],
        displayName: options.displayName ?? 'Test User',
        email: options.email ?? 'test@molesignal.local',
      },
    },
    version: 1,
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
    'log_patterns.read',
    'debug_artifacts.read',
    'streams.query',
    'logs.read',
    'traces.read',
    'metrics.read',
    'profiles.read',
    'query.jobs.read',
    'query.jobs.run',
    'query.slow.read',
    'apm.services.read',
    'apm.errors.read',
    'apm.error_rules.read',
    'apm.definitions.read',
    'apm.deployments.read',
    'apm.slos.read',
    'apm.settings.read',
    'rum.events.read',
    'rum.analytics.read',
    'rum.sessions.read',
    'rum.replay.read',
    'rum.applications.read',
    'rum.issues.read',
    'rum.playlists.read',
    'alerts.rules.read',
    'alerts.routing.read',
    'alerts.escalations.read',
    'alerts.calendars.read',
    'alerts.importance.read',
    'alerts.incidents.read',
    'alerts.mutes.read',
    'alerts.inhibitions.read',
    'alerts.remediation.read',
    'alerts.response.read',
    'alerts.semantic_groups.read',
    'alerts.sources.read',
    'alerts.tickets.read',
    'alerts.discovery.read',
    'notify.connectors.read',
    'notify.policies.read',
    'notify.templates.read',
    'notify.defaults.read',
    'notify.deliveries.read',
    'alerts.rules.manage',
    'alerts.rules.delete',
    'alerts.routing.manage',
    'alerts.escalations.manage',
    'alerts.escalations.delete',
    'alerts.calendars.manage',
    'alerts.importance.manage',
    'alerts.incidents.rca',
    'alerts.remediation.manage',
    'alerts.response.manage',
    'alerts.semantic_groups.manage',
    'alerts.sources.manage',
    'alerts.tickets.manage',
    'notify.connectors.manage',
    'notify.connectors.delete',
    'notify.policies.manage',
    'notify.policies.delete',
    'notify.templates.manage',
    'notify.defaults.manage',
    'notify.deliveries.retry',
    'alerts.incidents.create',
    'alerts.incidents.acknowledge',
    'alerts.incidents.update',
    'alerts.incidents.escalate',
    'alerts.incidents.remediate',
    'alerts.incidents.tickets',
    'notify.deliveries.acknowledge',
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
        organizationId: orgId,
        scope: 'organization',
        displayRole: role,
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
        routeCatalogVersion: 1,
        routes: [
          {
            id: 'root',
            pathPattern: '/',
            allowed: true,
          },
          {
            id: 'account.settings',
            pathPattern: '/account/settings/*',
            allowed: true,
          },
          {
            id: 'home',
            pathPattern: '/home',
            allowed: true,
            navigationGroup: 'home',
            navigationPosition: 10,
          },
          {
            id: 'logs',
            pathPattern: '/logs/*',
            allowed: true,
            navigationGroup: 'observe',
            navigationPosition: 20,
          },
          {
            id: 'traces',
            pathPattern: '/traces/*',
            allowed: true,
            navigationGroup: 'observe',
            navigationPosition: 30,
          },
          {
            id: 'alerts',
            pathPattern: '/alerts/*',
            allowed: true,
            navigationGroup: 'reliability',
            navigationPosition: 10,
          },
        ],
      },
    }),
  );
  await page.route('**/api/v1/instance', (route) =>
    route.fulfill({
      json: {
        externalUrl: '',
        signupEnabled: false,
        passkeySignin: false,
        version: '26.0.0.0',
      },
    }),
  );
  await page.route('**/api/v1/version', (route) =>
    route.fulfill({
      json: {
        version: '26.0.0.0',
        commit: 'mock-test',
        branch: 'test',
        buildEpochSecs: 1_785_087_406,
        buildId: 'playwright-session',
        releaseChannel: 'alpha',
        edition: 'community',
      },
    }),
  );
  await page.route('**/api/v1/me/profile', (route) =>
    route.fulfill({
      json: {
        userId: userId,
        email,
        displayName: displayName,
        orgId: orgId,
        orgName: orgName,
        orgSlug: orgName,
        displayRole: role,
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
