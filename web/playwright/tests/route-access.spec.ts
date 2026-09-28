import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('dynamic route access', () => {
  for (const path of [
    '/alerts/notify/connectors',
    '/alerts/channels',
    '/alerts/templates',
    '/settings/license',
    '/account/billing',
    '/account/support',
    '/iam/quota',
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
