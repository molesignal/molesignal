import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('personal preferences page', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/account/settings/preferences');
  });

  test('presents product settings with grouped controls and searchable timezone', async ({
    page,
  }) => {
    const preferences = page.locator('main');
    await expect(
      page.getByRole('heading', { name: 'Preferences', level: 2 }),
    ).toBeVisible();
    await expect(page.getByTestId('settings-trigger')).toHaveCount(0);

    await expect(
      preferences.getByText('Appearance', { exact: true }),
    ).toBeVisible();
    await expect(
      preferences.getByText('Language & region', { exact: true }),
    ).toBeVisible();
    await expect(
      preferences.getByText('Startup & interaction', { exact: true }),
    ).toBeVisible();
    await expect(
      preferences.getByRole('button', { name: 'View shortcuts' }),
    ).toBeVisible();
    await preferences
      .getByRole('button', { name: 'View shortcuts' })
      .click();
    const shortcutDialog = page.getByRole('dialog', {
      name: 'Keyboard shortcuts',
    });
    await expect(shortcutDialog).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(shortcutDialog).not.toBeVisible();

    await expect(
      preferences.getByRole('radio', { name: 'System' }),
    ).toHaveAttribute('aria-checked', 'true');
    await expect(
      preferences.getByRole('radio', { name: 'Comfortable' }),
    ).toHaveAttribute('aria-checked', 'true');
    await preferences
      .getByRole('button', { name: 'Date & time format' })
      .click();
    await expect(
      page.getByRole('radio', { name: '24 hour' }),
    ).toHaveAttribute('aria-checked', 'true');
    await page.keyboard.press('Escape');
    await expect(
      preferences.getByText('24h · ISO 8601'),
    ).toHaveCount(0);

    await expect(
      preferences.getByRole('combobox', {
        name: 'Open after entering workspace',
      }),
    ).toContainText('Home');
    await expect(
      preferences.getByText('/home', { exact: true }),
    ).toHaveCount(0);

    await preferences
      .getByRole('combobox', { name: 'Default timezone' })
      .click();
    const timezoneSearch = page.getByPlaceholder('Search timezones…');
    await expect(timezoneSearch).toBeVisible();
    await timezoneSearch.fill('Asia/Shanghai');
    const shanghaiOption = page.getByText(/Asia\/Shanghai · UTC\+8/);
    await expect(shanghaiOption).toBeVisible();
    await shanghaiOption.click();

    await page.setViewportSize({ width: 1024, height: 768 });
    const hasHorizontalOverflow = await preferences
      .locator('form')
      .evaluate((element) => element.scrollWidth > element.clientWidth);
    expect(hasHorizontalOverflow).toBe(false);
  });

  test('previews theme and restores the saved value on cancel', async ({
    page,
  }) => {
    const resolvedSystemTheme = await page.evaluate(() =>
      window.matchMedia('(prefers-color-scheme: dark)').matches
        ? 'dark'
        : 'light',
    );
    const preferences = page.locator('main');
    await preferences.getByRole('radio', { name: 'Dark' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await expect(
      preferences.getByText('You have unsaved changes'),
    ).toBeVisible();

    await preferences.getByRole('button', { name: 'Cancel' }).click();
    await expect(page.locator('html')).toHaveAttribute(
      'data-theme',
      resolvedSystemTheme,
    );
    await expect(
      preferences.getByText('You have unsaved changes'),
    ).toHaveCount(0);
  });

  test('saves product choices while persisting internal route values', async ({
    page,
  }) => {
    const preferences = page.locator('main');
    await preferences
      .getByRole('button', { name: 'Date & time format' })
      .click();
    await page.getByRole('radio', { name: '12 hour' }).click();
    await page.keyboard.press('Escape');

    const defaultHome = preferences.getByRole('combobox', {
      name: 'Open after entering workspace',
    });
    await defaultHome.click();
    await page.getByRole('option', { name: 'Specific dashboard…' }).click();
    await expect(
      preferences.getByRole('combobox', { name: 'Dashboard' }),
    ).toBeVisible();

    const saveResponse = page.waitForResponse(
      (response) =>
        response.url().includes('/api/v1/me/preferences') &&
        response.request().method() === 'PUT',
    );
    const saveButton = preferences.getByRole('button', {
      name: 'Save settings',
    });
    await saveButton.click();
    const response = await saveResponse;
    const request = response.request().postDataJSON() as {
      theme: string;
      time_format: string;
      date_format: string;
      default_home_route: string;
    };
    expect(request.theme).toBe('system');
    expect(request.time_format).toBe('local_12h');
    expect(request.date_format).toBe('yyyy_mm_dd_dash');
    expect(request.default_home_route).toMatch(/^\/dashboards\/.+/);
    await expect(saveButton).toBeDisabled();
  });

  test('uses localized Chinese product copy after saving the language', async ({
    page,
  }) => {
    const preferences = page.locator('main');
    await preferences.getByRole('combobox', { name: 'Language' }).click();
    await page.getByRole('option', { name: 'Simplified Chinese' }).click();
    await preferences.getByRole('button', { name: 'Save settings' }).click();

    await expect(
      page.getByRole('heading', { name: '偏好设置', level: 2 }),
    ).toBeVisible();
    await expect(
      preferences.getByText(
        '保存到个人账号，优先于工作区默认值；查询页面可以临时覆盖时间显示。',
      ),
    ).toBeVisible();
    await expect(
      preferences.getByRole('button', { name: '保存设置' }),
    ).toBeVisible();
  });
});

test.describe('preference entry-point synchronization', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('top theme shortcut updates the same preference shown on the account page', async ({
    page,
  }) => {
    await page.goto('/account/settings/profile');
    const initialTheme = await page.locator('html').getAttribute('data-theme');
    await page.getByTestId('theme-toggle').click();
    const expectedTheme = initialTheme === 'dark' ? 'light' : 'dark';
    await expect(page.locator('html')).toHaveAttribute(
      'data-theme',
      expectedTheme,
    );

    await page.getByRole('link', { name: 'Preferences' }).click();
    const preferences = page.locator('main');
    await expect(
      preferences.getByRole('radio', {
        name: expectedTheme === 'dark' ? 'Dark' : 'Light',
      }),
    ).toHaveAttribute('aria-checked', 'true');
  });

  test('page timezone override can be promoted to the personal default', async ({
    page,
  }) => {
    await page.goto('/logs');
    const timezone = page.getByRole('combobox', { name: 'Page timezone' });
    await expect(timezone).toContainText('Timezone: use default');

    await timezone.click();
    await page.getByPlaceholder('Search timezones…').fill('Europe/Paris');
    await page.getByText(/Europe\/Paris · UTC[+-]\d+/).click();
    await expect(timezone).toContainText('this page only');

    await timezone.click();
    const responsePromise = page.waitForResponse(
      (response) =>
        response.url().includes('/api/v1/me/preferences') &&
        response.request().method() === 'PUT',
    );
    await page
      .getByRole('button', { name: 'Set as my default timezone' })
      .click();
    const response = await responsePromise;
    expect(response.request().postDataJSON()).toMatchObject({
      timezone: 'Europe/Paris',
    });
    await expect(timezone).toContainText('Timezone: use default');
  });
});

test.describe('workspace preference defaults', () => {
  test('persists administrator defaults separately from personal preferences', async ({
    page,
    mockServer,
  }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.goto('/settings/general');

    const section = page
      .getByText('Organization preference defaults', { exact: true })
      .locator('xpath=ancestor::section[1]');
    await expect(
      section.getByText(
        'Personal preferences and current-page overrides take priority.',
        { exact: false },
      ),
    ).toBeVisible();
    const responsePromise = page.waitForResponse(
      (response) =>
        response.url().includes('/api/v1/workspace/preferences') &&
        response.request().method() === 'PUT',
    );
    await section.getByRole('radio', { name: 'Dark' }).click();
    const response = await responsePromise;
    expect(response.request().postDataJSON()).toMatchObject({
      theme: 'dark',
      timezone: '',
    });
  });
});
