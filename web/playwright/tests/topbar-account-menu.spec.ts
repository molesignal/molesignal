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

    await expect(page.getByTestId('org-switcher')).toHaveCount(0);
    await expect(page.getByTestId('command-palette-trigger')).toBeVisible();

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
      menu.getByRole('menuitem', { name: 'Notify' }),
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

  test('uses the same flat selected fill for the workspace trigger and current option', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, {
      token: 'fake-jwt',
    });
    await page.goto('/account/settings/profile');
    await page.getByTestId('user-menu-trigger').click();

    const menu = page.locator("[data-ui='user-menu']");
    const trigger = page.locator("[data-ui='workspace-switcher-trigger']");
    await expect(menu).toBeVisible();
    await expect(menu).toHaveCSS('box-shadow', 'none');
    await expect(trigger).toBeVisible();
    await expect(trigger).toHaveAttribute('data-state', 'closed');
    await expect(trigger).not.toHaveAttribute('data-selected', 'true');
    await expect(trigger).not.toHaveAttribute('style', /background-color/);
    await expect
      .poll(() =>
        trigger.evaluate(
          (element) => getComputedStyle(element).backgroundColor,
        ),
      )
      .toBe('rgba(0, 0, 0, 0)');
    await expect(trigger).toHaveCSS('box-shadow', 'none');

    await page.getByText('Switch', { exact: true }).click();
    await expect(trigger).toHaveAttribute('data-state', 'open');
    await expect(trigger).toHaveAttribute('data-selected', 'true');
    await expect(trigger).toHaveAttribute(
      'style',
      /background-color: var\(--workspace-selection-fill\)/,
    );
    const currentOption = page.locator(
      "[data-ui='workspace-option'][data-state='checked']",
    );
    await expect(currentOption).toBeVisible();
    await page.waitForTimeout(400);
    await expect(trigger).toHaveAttribute('data-state', 'open');
    await expect(currentOption).toBeVisible();
    await expect
      .poll(async () => {
        const [triggerFill, optionFill, menuFill] = await Promise.all([
          trigger.evaluate((element) =>
            getComputedStyle(element).backgroundColor,
          ),
          currentOption.evaluate((element) =>
            getComputedStyle(element).backgroundColor,
          ),
          menu.evaluate((element) =>
            getComputedStyle(element).backgroundColor,
          ),
        ]);
        return (
          triggerFill === optionFill &&
          triggerFill !== menuFill &&
          triggerFill !== 'rgba(0, 0, 0, 0)'
        );
      })
      .toBe(true);
    await expect(currentOption).toHaveCSS('box-shadow', 'none');
    await expect
      .poll(() =>
        currentOption.evaluate((element) => {
          const submenu = element.closest('[role="menu"]');
          return submenu ? getComputedStyle(submenu).boxShadow : null;
        }),
      )
      .toBe('none');
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

    await expect(page.getByTestId('org-switcher')).toHaveCount(0);
    await page.getByTestId('user-menu-trigger').click();
    await expect(page.getByTestId('current-workspace-role')).toHaveText(
      'Owner',
    );
    await page.getByText('Switch', { exact: true }).click();
    await page.getByRole('menuitemradio', { name: 'default' }).click();

    await expect(page).toHaveURL(/\/home(?:[?#]|$)/);
    await expect(page.getByRole('link', { name: 'License' })).toHaveCount(0);
  });
});
