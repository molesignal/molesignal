import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('topbar account menu', () => {
  test('separates identity, workspace, and personal actions', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, {
      token: 'fake-jwt',
    });
    await page.goto('/account/settings/profile');

    await expect(
      page.getByTestId('workspace-settings-trigger'),
    ).toHaveAttribute('aria-label', 'Workspace settings');

    const userMenuTrigger = page.getByTestId('user-menu-trigger');
    await userMenuTrigger.click();
    await expect(userMenuTrigger).toHaveCSS('box-shadow', 'none');
    await expect(userMenuTrigger).toHaveCSS('outline-style', 'none');
    const menu = page.getByRole('menu');
    await expect(menu).toBeVisible();
    const menuBox = await menu.boundingBox();
    expect(menuBox!.width).toBeGreaterThanOrEqual(280);
    expect(menuBox!.width).toBeLessThanOrEqual(320);

    await expect(menu.getByText('Dev User', { exact: true })).toBeVisible();
    await expect(
      menu.getByText('dev@molesignal.local', { exact: true }),
    ).toBeVisible();
    await expect(
      menu.getByText('Current workspace', { exact: true }),
    ).toBeVisible();
    await expect(menu.getByText('acme-prod', { exact: true })).toBeVisible();
    const roleBadge = menu.getByTestId('current-workspace-role');
    await expect(roleBadge).toHaveText('Owner');
    expect(await roleBadge.evaluate((element) => element.tagName)).toBe('SPAN');
    await expect(menu.getByText('Switch', { exact: true })).toBeVisible();

    await expect(
      menu.getByRole('menuitem', { name: 'Account settings' }),
    ).toBeVisible();
    await expect(
      menu.getByRole('menuitem', { name: 'Preferences' }),
    ).toBeVisible();
    await expect(
      menu.getByRole('menuitem', { name: 'Notifications' }),
    ).toBeVisible();
    await expect(
      menu.getByRole('menuitem', { name: 'Sign out' }),
    ).toBeVisible();
    await expect(
      menu.getByRole('menuitem', { name: 'Settings', exact: true }),
    ).toHaveCount(0);

    await menu.getByRole('menuitem', { name: 'Preferences' }).click();
    await expect(page).toHaveURL(/\/account\/settings\/preferences/);
  });

  test('opens workspace settings only for administrators', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, {
      token: 'fake-jwt',
    });
    await page.goto('/account/settings/profile');
    await page.getByTestId('workspace-settings-trigger').click();
    await expect(page).toHaveURL(/\/settings\/general/);
  });

  test('hides workspace settings from non-administrators', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, { role: 'Viewer' });
    await page.goto('/account/settings/profile');
    await expect(page.getByTestId('workspace-settings-trigger')).toHaveCount(
      0,
    );
  });

  test('does not show a switch action for a single workspace', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, {
      token: 'fake-jwt',
    });
    await page.route('**/api/v1/orgs', async (route) => {
      await route.fulfill({
        json: [
          {
            id: 'acme-prod',
            name: 'acme-prod',
            slug: 'acme-prod',
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
        ],
      });
    });
    await page.goto('/account/settings/profile');
    await page.getByRole('button', { name: 'User menu' }).click();

    const menu = page.getByRole('menu');
    await expect(menu.getByText('acme-prod', { exact: true })).toBeVisible();
    await expect(menu.getByText('Switch', { exact: true })).toHaveCount(0);
  });

  test('can switch from the system workspace back to the only tenant workspace', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, {
      token: 'system-token',
      orgId: 'system-org-id',
      orgName: '_sys',
      role: 'Owner',
      scope: 'system',
      platformPermissions: [
        'license_read',
        'license_write',
        'system_telemetry_read',
      ],
    });
    await page.route('**/api/v1/orgs', async (route) => {
      await route.fulfill({
        json: [
          {
            id: 'default-org-id',
            name: 'default',
            slug: 'default',
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
        ],
      });
    });
    await page.goto('/settings/license');

    const switcher = page.getByTestId('org-switcher');
    await expect(switcher).toHaveText('_sys');
    await expect(switcher).toHaveCSS('box-shadow', 'none');
    await expect(switcher).toHaveCSS('outline-style', 'none');
    await page.getByTestId('user-menu-trigger').click();
    await expect(page.getByTestId('current-workspace-role')).toHaveText(
      'Owner',
    );
    await page.keyboard.press('Escape');
    await switcher.click();
    await page.getByRole('menuitem', { name: 'default' }).click();

    await expect(page).toHaveURL(/\/home(?:[?#]|$)/);
    await expect(switcher).toHaveText('default');
    await expect(page.getByRole('link', { name: 'License' })).toHaveCount(0);
  });
});
