import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('dynamic route access', () => {
  for (const path of [
    '/alerts/notify/connectors',
    '/alerts/channels',
    '/alerts/templates',
  ]) {
    test(`${path} is deleted without a compatibility redirect`, async ({
      page,
      mockServer,
    }) => {
      await mountMockRoutes(page, mockServer.port);

      await page.goto(path);

      await expect(page).toHaveURL(new RegExp(`${path.replaceAll('/', '\\/')}(?:[?#]|$)`));
      await expect(
        page
          .getByRole('status')
          .getByText('Page not found', { exact: true }),
      ).toBeVisible();
    });
  }

  test('legacy organization settings route redirects to General', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);

    await page.goto('/settings/organization');

    await expect(page).toHaveURL(/\/settings\/general(?:[?#]|$)/);
    await expect(page.getByLabel('Organization name')).toBeVisible();
    await expect(
      page.getByRole('link', { name: 'Organization', exact: true }),
    ).toHaveCount(0);
  });

  test('tenant sessions neither discover nor mount the system License route', async ({
    page,
    mockServer,
  }) => {
    let licenseRequests = 0;
    page.on('request', (request) => {
      if (new URL(request.url()).pathname === '/api/v1/system/license') {
        licenseRequests += 1;
      }
    });
    await mountMockRoutes(page, mockServer.port, {
      token: 'tenant-token',
      role: 'Owner',
      scope: 'organization',
    });

    await page.goto('/settings/license');

    await expect(page).toHaveURL(/\/home(?:[?#]|$)/);
    await expect(page.getByRole('link', { name: 'License' })).toHaveCount(0);
    expect(licenseRequests).toBe(0);
  });

  test('system LicenseRead exposes the route and its navigation entry', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, {
      token: 'system-token',
      orgId: 'system-org-id',
      orgName: '_sys',
      role: 'Viewer',
      scope: 'system',
      platformPermissions: ['license_read'],
    });

    await page.goto('/settings/license');

    await expect(page).toHaveURL(/\/settings\/license(?:[?#]|$)/);
    await expect(
      page.getByText('Active plan and entitlements.', { exact: true }),
    ).toBeVisible();
    await expect(
      page.getByRole('link', { name: 'License' }).first(),
    ).toBeVisible();
    await expect(
      page.getByRole('link', { name: 'General' }),
    ).toHaveCount(0);
    await expect(page.locator('a[href="/traces"]')).toHaveCount(0);
    await expect(page.locator('a[href="/logs"]')).toHaveCount(0);
    await expect(page.getByTestId('mole-agent-trigger')).toHaveCount(0);
    await expect(
      page.getByRole('button', { name: 'Upload', exact: true }),
    ).toBeDisabled();
  });

  test('system scope without LicenseRead cannot discover or mount License', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, {
      token: 'system-token-without-license',
      orgId: 'system-org-id',
      orgName: '_sys',
      role: 'Viewer',
      scope: 'system',
      platformPermissions: ['system_telemetry_read'],
    });

    await page.goto('/settings/license');

    await expect(page).toHaveURL(/\/home(?:[?#]|$)/);
    await expect(page.getByRole('link', { name: 'License' })).toHaveCount(0);
  });

  test('role controls follow IAM capabilities instead of the display role', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, {
      token: 'custom-role-reader',
      role: 'Viewer',
      scope: 'organization',
      capabilityPermissions: ['iam.roles.read'],
    });

    await page.goto('/iam/roles');

    await expect(page).toHaveURL(/\/iam\/roles(?:[?#]|$)/);
    await expect(page.getByRole('button', { name: 'New role' })).toBeDisabled();
    await expect(page.getByRole('button', { name: /Edit Owner/ })).toBeDisabled();
  });

  test('a custom IAM capability can create a role and submits catalog keys', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, {
      token: 'custom-role-manager',
      role: 'Viewer',
      scope: 'organization',
      capabilityPermissions: ['iam.roles.read', 'iam.roles.manage'],
    });
    await page.goto('/iam/roles');

    await page.getByRole('button', { name: 'New role' }).click();
    await page.getByLabel('Name').fill('Incident Commander');
    await expect(page.getByLabel('Key')).toHaveValue('incident_commander');
    await page.getByText('Edit dashboards', { exact: true }).click();

    const requestPromise = page.waitForRequest(
      (request) =>
        new URL(request.url()).pathname === '/api/v1/roles' &&
        request.method() === 'POST',
    );
    await page.getByRole('button', { name: 'Create role' }).click();
    const request = await requestPromise;
    expect(request.postDataJSON()).toMatchObject({
      key: 'incident_commander',
      name: 'Incident Commander',
      permissions: expect.arrayContaining(['dashboards.edit']),
    });
    await expect(page.getByText('Role created', { exact: true })).toBeVisible();
  });
});
