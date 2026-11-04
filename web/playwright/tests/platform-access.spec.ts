import type { Page } from '@playwright/test';

import { expect, MOCK_IAM_PLATFORM_PERMISSIONS, mountMockRoutes, test } from '../fixtures/mockBackend';

const SYSTEM_ORG = 'system-org-id';
const ACME = 'acme-id';
const BETA = 'beta-id';

const PLATFORM_ROLES = [
  {
    id: 'role-platform-admin',
    key: 'platform_admin',
    name: 'Platform administrator',
    description: 'All platform permissions.',
    builtin: true,
    roleType: 'platform',
    scope: 'platform',
    permissions: [...MOCK_IAM_PLATFORM_PERMISSIONS],
    policyIds: [],
    effectivePermissions: [...MOCK_IAM_PLATFORM_PERMISSIONS],
    usage: { memberships: 1, serviceAccounts: 0, invitations: 0, bindings: 1, total: 2 },
    createdAt: 1,
    updatedAt: 1,
  },
  {
    id: 'role-retention-keeper',
    key: 'retention_keeper',
    name: 'Retention keeper',
    description: '',
    builtin: false,
    roleType: 'platform',
    scope: 'platform',
    permissions: ['sys.settings.retention.read', 'sys.settings.retention.manage'],
    policyIds: [],
    effectivePermissions: ['sys.settings.retention.read', 'sys.settings.retention.manage'],
    usage: { memberships: 0, serviceAccounts: 0, invitations: 0, bindings: 0, total: 0 },
    createdAt: 1,
    updatedAt: 1,
  },
];

const ORGANIZATIONS = [
  {
    id: SYSTEM_ORG,
    name: '_sys',
    slug: '_sys',
    system: true,
    disabled: false,
    accessible: true,
    displayRole: 'Super administrator',
    roles: [{ id: 'super_administrator', key: 'super_administrator', name: 'Super administrator', builtin: true }],
  },
  {
    id: ACME,
    name: 'Acme',
    slug: 'acme',
    system: false,
    disabled: false,
    accessible: false,
    displayRole: null,
    roles: [],
  },
  {
    id: BETA,
    name: 'Beta',
    slug: 'beta',
    system: false,
    disabled: false,
    accessible: false,
    displayRole: null,
    roles: [],
  },
];

const GRANTED = {
  id: 'binding-1',
  organizationId: SYSTEM_ORG,
  roleId: 'role-platform-admin',
  principalType: 'user',
  principalId: 'user-grace',
  conditions: {},
  createdBy: 'root',
  createdAt: 1,
};

const GRACE = { id: 'user-grace', email: 'grace@example.com', displayName: 'Grace', disabled: false };

async function servePlatform(page: Page, bindings: unknown[] = [GRANTED]) {
  await page.route('**/api/v1/roles', (route) => route.fulfill({ json: PLATFORM_ROLES }));
  await page.route('**/api/v1/orgs', (route) => route.fulfill({ json: ORGANIZATIONS }));
  await page.route('**/api/v1/iam/role-bindings', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({ json: bindings })
      : route.fulfill({ json: { value: GRANTED, version: 2 } }),
  );
  await page.route('**/api/v1/iam/platform/accounts**', (route) => route.fulfill({ json: [GRACE] }));
}

const mountPlatform = (
  page: Page,
  port: number,
  permissions: readonly string[],
  organizationPermissions: Record<string, string[]> = {},
) =>
  mountMockRoutes(page, port, {
    orgId: SYSTEM_ORG,
    orgName: '_sys',
    role: 'Super administrator',
    scope: 'system',
    capabilityPermissions: [...permissions],
    organizationPermissions,
  });

test.describe('platform access', () => {
  test('lists who holds what on the platform', async ({ page, mockServer }) => {
    await mountPlatform(page, mockServer.port, MOCK_IAM_PLATFORM_PERMISSIONS);
    await servePlatform(page);

    await page.goto('/iam/groups');

    await expect(page.getByRole('heading', { name: 'Platform access' })).toBeVisible();
    await expect(page.getByText('grace@example.com')).toBeVisible();
    await expect(page.getByRole('cell', { name: 'Platform administrator' })).toBeVisible();
    await expect(page.getByRole('cell', { name: 'Whole platform' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Share across organizations' })).toHaveCount(0);
  });

  test('grants a role to an account found by its email', async ({ page, mockServer }) => {
    await mountPlatform(page, mockServer.port, MOCK_IAM_PLATFORM_PERMISSIONS);
    await servePlatform(page, []);

    await page.goto('/iam/groups');
    await page.getByRole('button', { name: 'New access grant' }).first().click();

    const drawer = page.getByRole('dialog');
    await drawer.getByRole('combobox', { name: 'Account' }).click();
    await page.getByPlaceholder('Email or name').fill('grace');
    await page.getByRole('option', { name: /grace@example.com/ }).click();

    await drawer.getByRole('combobox', { name: 'Role' }).click();
    await page.getByRole('option', { name: 'Retention keeper' }).click();

    // This role holds nothing about organizations, so there is nothing to narrow it to.
    await expect(drawer.getByText('Scope', { exact: true })).toHaveCount(0);

    const created = page.waitForRequest(
      (request) => request.method() === 'POST' && request.url().endsWith('/iam/role-bindings'),
    );
    await drawer.getByRole('button', { name: 'Create grant' }).click();

    expect((await created).postDataJSON()).toEqual({
      roleId: 'role-retention-keeper',
      principalType: 'user',
      principalId: 'user-grace',
    });
  });

  test('narrows a role that holds what organizations are managed with', async ({ page, mockServer }) => {
    await mountPlatform(page, mockServer.port, MOCK_IAM_PLATFORM_PERMISSIONS);
    await servePlatform(page, []);

    await page.goto('/iam/groups');
    await page.getByRole('button', { name: 'New access grant' }).first().click();

    const drawer = page.getByRole('dialog');
    await drawer.getByRole('combobox', { name: 'Account' }).click();
    await page.getByPlaceholder('Email or name').fill('grace');
    await page.getByRole('option', { name: /grace@example.com/ }).click();

    await drawer.getByRole('combobox', { name: 'Role' }).click();
    await page.getByRole('option', { name: 'Platform administrator' }).click();

    await drawer.getByRole('combobox', { name: 'Scope' }).click();
    await expect(page.getByRole('option', { name: '_sys' })).toHaveCount(0);
    await page.getByRole('option', { name: 'Beta' }).click();

    const created = page.waitForRequest(
      (request) => request.method() === 'POST' && request.url().endsWith('/iam/role-bindings'),
    );
    await drawer.getByRole('button', { name: 'Create grant' }).click();

    expect((await created).postDataJSON()).toEqual({
      roleId: 'role-platform-admin',
      principalType: 'user',
      principalId: 'user-grace',
      resourceType: 'organization',
      resourceId: BETA,
    });
  });

  test('reading it does not offer changing it', async ({ page, mockServer }) => {
    await mountPlatform(page, mockServer.port, ['sys.administrators.read']);
    await servePlatform(page);

    await page.goto('/iam/groups');

    await expect(page.getByRole('button', { name: 'New access grant' }).first()).toBeDisabled();
    await expect(page.getByRole('button', { name: 'Delete access grant' })).toBeDisabled();
  });

  test('the roles of the platform offer the permissions of the platform', async ({ page, mockServer }) => {
    await mountPlatform(page, mockServer.port, MOCK_IAM_PLATFORM_PERMISSIONS);
    await servePlatform(page);

    await page.goto('/iam/roles');

    await expect(
      page.getByText(`All ${MOCK_IAM_PLATFORM_PERMISSIONS.length} platform permissions`),
    ).toBeVisible();

    await page.getByRole('button', { name: 'New role' }).click();

    const drawer = page.getByRole('dialog');
    await expect(drawer.getByText('Manage retention settings')).toBeVisible();
    await expect(drawer.getByText('View dashboards', { exact: true })).toHaveCount(0);
  });
});

test.describe('organizations someone reaches by name', () => {
  test('are the ones that can be acted on', async ({ page, mockServer }) => {
    await mountPlatform(page, mockServer.port, [], {
      'sys.organizations.read': [ACME],
      'sys.organizations.update': [ACME],
    });
    await servePlatform(page);

    await page.goto('/settings/organization_management');

    const acme = page.getByRole('row', { name: /Acme/ });
    await expect(acme.getByRole('button', { name: 'Edit' })).toBeEnabled();
    await expect(acme.getByRole('button', { name: 'Delete' })).toBeDisabled();
    await expect(acme.getByRole('button', { name: 'Disable' })).toBeDisabled();

    const beta = page.getByRole('row', { name: /Beta/ });
    await expect(beta.getByRole('button', { name: 'Edit' })).toBeDisabled();

    await expect(page.getByRole('button', { name: 'Create organization' })).toBeDisabled();
  });
});

test.describe('the sidebar of the platform organization', () => {
  test('reaches a settings page from a permission over that page alone', async ({ page, mockServer }) => {
    await mountPlatform(page, mockServer.port, ['sys.settings.retention.read']);

    // The home page is out of reach, so this is where the app lands.
    await page.goto('/home');
    await expect(page).toHaveURL(/\/settings\/stream_retention$/);

    await page.getByRole('link', { name: 'Settings', exact: true }).click();

    await expect(page).toHaveURL(/\/settings\/stream_retention$/);
    await expect(page.getByRole('link', { name: 'Identity & Access', exact: true })).toHaveCount(0);
  });

  test('opens organization management for whoever manages organizations', async ({ page, mockServer }) => {
    await mountPlatform(page, mockServer.port, MOCK_IAM_PLATFORM_PERMISSIONS);
    await servePlatform(page);

    await page.goto('/home');
    await page.getByRole('link', { name: 'Settings', exact: true }).click();

    await expect(page).toHaveURL(/\/settings\/organization_management$/);
  });
});

test.describe('root', () => {
  test('reads as the super administrator in the language of the page', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port, {
      orgId: SYSTEM_ORG,
      orgName: '_sys',
      role: 'Super administrator',
      scope: 'system',
      capabilityPermissions: [...MOCK_IAM_PLATFORM_PERMISSIONS],
      preferences: { language: 'zh-cn' },
    });
    await servePlatform(page);

    await page.goto('/home');
    await page.getByTestId('org-status-trigger').click();

    await expect(page.getByTestId('current-org-role')).toHaveText('超级管理员');
  });
});
