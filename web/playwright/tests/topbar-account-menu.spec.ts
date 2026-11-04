import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('topbar account menu', () => {
  test('keeps identity and personal actions in the user menu, and the organization in the top bar', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/account/settings/profile');

    await expect(page.getByTestId('command-palette-trigger')).toBeVisible();
    // The organization stands by the avatar, its light the status of the core API.
    const org = page.getByTestId('org-status-trigger');
    await expect(org).toHaveText('acme-prod');
    await expect(org).toHaveAccessibleName('acme-prod, Core API connected');
    await expect(org.getByTestId('system-status-indicator')).toHaveAttribute(
      'data-state',
      'healthy',
    );

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
    await expect(menu.getByText('Current workspace', { exact: true })).toHaveCount(0);
    await expect(menu.getByText('acme-prod', { exact: true })).toHaveCount(0);

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

  test('shows a single organization without the arrows that switch', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.route('**/api/v1/orgs', async (route) => {
      await route.fulfill({
        json: [
          {
            id: 'acme-prod',
            name: 'acme-prod',
            slug: 'acme-prod',
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
        ],
      });
    });
    await page.goto('/account/settings/profile');

    const org = page.getByTestId('org-status-trigger');
    await expect(org).toHaveText('acme-prod');
    await expect(org.locator('svg.lucide-chevrons-up-down')).toHaveCount(0);
    await org.click();

    const menu = page.locator("[data-ui='org-status-menu']");
    await expect(menu.getByText('Current organization', { exact: true })).toBeVisible();
    await expect(menu.getByRole('menuitemradio')).toHaveCount(1);
    await expect(menu.getByRole('menuitemradio', { name: /^acme-prod/ })).toHaveAttribute(
      'aria-checked',
      'true',
    );
  });

  test('opens on the organizations with the current one marked, the status left to the light', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/account/settings/profile');

    const org = page.getByTestId('org-status-trigger');
    await expect(org.locator('svg.lucide-chevrons-up-down')).toHaveCount(1);
    await org.click();

    const menu = page.locator("[data-ui='org-status-menu']");
    await expect(menu).toBeVisible();
    // The status is the light and the button's name; the menu does not say it again.
    await expect(menu.getByText('Core API connected', { exact: true })).toHaveCount(0);
    await expect(menu.getByText('Switch organization', { exact: true })).toBeVisible();
    const current = menu.locator("[data-ui='workspace-option'][data-state='checked']");
    await expect(current).toContainText('acme-prod');
    await expect(menu.getByTestId('current-org-role')).toHaveText('Owner');
    // Administering organizations is a platform right: an organization's owner is not offered it.
    await expect(menu.getByRole('menuitem', { name: 'Manage organizations…' })).toHaveCount(0);
    // The organization the reader is in takes the selection fill, a step off the menu.
    await expect
      .poll(async () => {
        const [optionFill, menuFill] = await Promise.all([
          current.evaluate((element) => getComputedStyle(element).backgroundColor),
          menu.evaluate((element) => getComputedStyle(element).backgroundColor),
        ]);
        return optionFill !== menuFill && optionFill !== 'rgba(0, 0, 0, 0)';
      })
      .toBe(true);
  });

  test('offers the way to the organizations to whoever may administer them', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, {
      orgId: 'system-org-id',
      orgName: '_sys',
      role: 'Owner',
      scope: 'system',
      platformPermissions: ['sys.organizations.read', 'sys.telemetry.read'],
    });
    await page.goto('/home');

    await page.getByTestId('org-status-trigger').click();
    const menu = page.locator("[data-ui='org-status-menu']");
    await menu.getByRole('menuitem', { name: 'Manage organizations…' }).click();

    await expect(page).toHaveURL(/\/iam\/organizations(?:[?#]|$)/);
  });

  test('can switch from the system workspace back to the only tenant workspace', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port, {
      orgId: 'system-org-id',
      orgName: '_sys',
      role: 'Owner',
      scope: 'system',
      platformPermissions: [
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
        ],
      });
    });
    await page.goto('/home');

    const org = page.getByTestId('org-status-trigger');
    await expect(org).toHaveText('_sys');
    await org.click();
    await expect(page.getByTestId('current-org-role')).toHaveText('Owner');
    await page.getByRole('menuitemradio', { name: /^default/ }).click();

    await expect(page).toHaveURL(/\/home(?:[?#]|$)/);
    await expect(page.getByRole('link', { name: 'License' })).toHaveCount(0);
  });
});

test('draws the avatar as a 28px disc inside the 32px topbar target', async ({
  page,
  mockServer,
}) => {
  await mountMockRoutes(page, mockServer.port);
  await page.goto('/home');

  const target = (await page.getByTestId('user-menu-trigger').boundingBox())!;
  const disc = (await page.getByTestId('user-avatar').boundingBox())!;

  // Quieter than the 32px disc it replaced, and no harder to hit.
  expect(disc.width).toBe(28);
  expect(disc.height).toBe(28);
  expect(target.width).toBeGreaterThanOrEqual(32);
  expect(target.height).toBeGreaterThanOrEqual(32);
  // Centred in its target.
  expect(Math.abs(disc.x + disc.width / 2 - (target.x + target.width / 2))).toBeLessThanOrEqual(0.5);
  expect(Math.abs(disc.y + disc.height / 2 - (target.y + target.height / 2))).toBeLessThanOrEqual(0.5);
});

test.describe('language in the user menu', () => {
  test('is chosen from the menu, takes effect at once and is kept', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/account/settings/profile');
    await page.getByRole('button', { name: 'User menu' }).click();

    const menu = page.locator("[data-ui='user-menu']");
    const language = menu.locator("[data-ui='language-trigger']");
    // A row of the menu, after the pages of the account and before signing out, saying what is in use.
    await expect(language).toContainText('Language');
    await expect(language.locator("[data-ui='language-current']")).toHaveText('English');
    await expect(menu.getByRole('menuitem')).toHaveText([
      /^Account settings$/,
      /^Preferences$/,
      /^Notify$/,
      /^Language\s*English$/,
      /^Sign out$/,
    ]);

    await language.click();
    const options = page.locator("[data-ui='language-menu']").getByRole('menuitemradio');
    // Each language is named in itself, whatever language is on screen.
    await expect(options).toHaveText(['简体中文', 'English']);
    await expect(options.nth(1)).toHaveAttribute('aria-checked', 'true');

    const saved = page.waitForRequest(
      (request) => request.method() === 'PUT' && request.url().includes('/api/v1/me/preferences'),
    );
    await options.first().click();
    expect((await saved).postDataJSON()).toMatchObject({ language: 'zh-cn' });
    await expect(page.locator('html')).toHaveAttribute('lang', 'zh-cn');

    // Still Chinese after a reload, and the menu says so.
    await page.reload();
    await page.getByRole('button', { name: '用户菜单' }).click();
    await expect(page.locator("[data-ui='language-trigger']")).toContainText('语言');
    await expect(page.locator("[data-ui='language-current']")).toHaveText('简体中文');
  });

  test('what the backend names in the reader\'s language is read again once the language is kept', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    // Built-in functions are described by the backend, in the language it has for the reader.
    const order: string[] = [];
    await page.route('**/api/v1/functions', (route) => {
      order.push('functions read');
      return route.fulfill({ json: [] });
    });
    page.on('response', (response) => {
      if (response.request().method() === 'PUT' && response.url().includes('/api/v1/me/preferences')) {
        order.push('language kept');
      }
    });

    await page.goto('/functions');
    await expect.poll(() => order).toEqual(['functions read']);

    await page.getByRole('button', { name: 'User menu' }).click();
    await page.locator("[data-ui='language-trigger']").click();
    await page.getByRole('menuitemradio', { name: '简体中文' }).click();

    // After the save, not before it: the backend goes by the language it has kept.
    await expect.poll(() => order).toEqual(['functions read', 'language kept', 'functions read']);
  });

  test('a language that cannot be kept is put back', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.route('**/api/v1/me/preferences', (route) =>
      route.request().method() === 'PUT'
        ? route.fulfill({ status: 500, json: { error: 'preferences unavailable' } })
        : route.fallback(),
    );
    await page.goto('/account/settings/profile');
    await page.getByRole('button', { name: 'User menu' }).click();
    await page.locator("[data-ui='language-trigger']").click();
    await page.getByRole('menuitemradio', { name: '简体中文' }).click();

    await expect(page.getByText('preferences unavailable')).toBeVisible();
    await expect(page.locator('html')).toHaveAttribute('lang', 'en-us');
  });
});

