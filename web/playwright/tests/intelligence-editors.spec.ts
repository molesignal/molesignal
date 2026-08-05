import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('Mole Intelligence editors', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('edits a model provider Base URL and an agent profile', async ({ page }) => {
    await page.goto('/intelligence/settings');

    await page.getByRole('tab', { name: 'Models & reasoning' }).click();
    await page.getByRole('button', { name: 'Edit model provider' }).last().click();
    const baseUrl = page.getByLabel('Base URL');
    await expect(baseUrl).toHaveValue('https://llm.internal.example/v1');
    await baseUrl.fill('https://gateway.internal.example/v1');
    await page.getByRole('button', { name: 'Save' }).click();
    await expect(page.getByText('https://gateway.internal.example/v1')).toBeVisible();

    await page.getByRole('tab', { name: 'Agent profiles' }).click();
    await page.getByRole('button', { name: 'Edit profile' }).click();
    await expect(
      page.getByRole('checkbox', { name: /query_logs/ }),
    ).toHaveCount(0);
    await expect(page.getByLabel('Allowed environments')).toHaveCount(0);
    await expect(
      page.getByRole('combobox', { name: /L0 ·/ }),
    ).toHaveCount(0);
    await page.getByLabel('Max tool calls').fill('48');
    await page.getByRole('button', { name: 'Save' }).click();
    await expect(page.getByText('48')).toBeVisible();
  });

  test('keeps profile editing scoped to the selected settings section', async ({
    page,
  }) => {
    await page.goto('/intelligence/settings/tools');

    await expect(
      page.getByRole('heading', { name: 'Tools & capabilities', level: 2 }),
    ).toBeVisible();
    await expect(page.getByText('Enabled tools')).toBeVisible();
    await expect(page.getByText('MCP Servers', { exact: true })).toBeVisible();

    await page.getByRole('button', { name: 'Configure tool policy' }).click();
    await expect(
      page.getByRole('dialog', { name: 'Configure tool policy' }),
    ).toBeVisible();
    await expect(page.getByLabel('Allowed environments')).toHaveCount(0);
    await expect(page.getByLabel('Max tool calls')).toHaveCount(0);
    await page.getByRole('button', { name: 'Cancel' }).click();

    await page.getByText('query_logs', { exact: true }).click();
    await expect(page.getByRole('dialog', { name: 'query_logs' })).toBeVisible();
    await expect(page.getByRole('tab', { name: 'Input & output' })).toBeVisible();
    await expect(page.getByRole('tab', { name: 'Call records' })).toBeVisible();
    await expect(page.getByRole('tab', { name: 'Dependencies' })).toBeVisible();
    await page.getByRole('button', { name: 'Close' }).click();

    await page.getByRole('tab', { name: 'Data access' }).click();
    await page.getByRole('button', { name: 'Edit data access' }).click();
    await expect(
      page.getByRole('dialog', { name: 'Edit data access' }),
    ).toBeVisible();
    await expect(page.getByLabel('Allowed environments')).toBeVisible();
    await expect(
      page.getByRole('checkbox', { name: /query_logs/ }),
    ).toHaveCount(0);
    await expect(page.getByLabel('Max tool calls')).toHaveCount(0);
    await page.getByRole('button', { name: 'Cancel' }).click();

    await page.getByRole('tab', { name: 'Network policy' }).click();
    await page.getByRole('button', { name: 'Edit network policy' }).click();
    await expect(
      page.getByRole('dialog', { name: 'Edit network policy' }),
    ).toBeVisible();
    const networkSwitch = page.getByRole('switch', {
      name: 'Allow network access',
    });
    await expect(networkSwitch).not.toBeChecked();
    await expect(page.getByLabel('Allowed environments')).toHaveCount(0);
    await expect(page.getByLabel('Max tool calls')).toHaveCount(0);
    await networkSwitch.click();
    await page.getByRole('button', { name: 'Save' }).click();
    await expect(page.getByText('Allowed', { exact: true }).first()).toBeVisible();
    await page.getByRole('button', { name: 'Edit network policy' }).click();
    await expect(
      page.getByRole('switch', { name: 'Allow network access' }),
    ).toBeChecked();
    await page.getByRole('button', { name: 'Cancel' }).click();

    await page.getByRole('tab', { name: 'Approval policy' }).click();
    await page.getByRole('button', { name: 'Edit approval policy' }).click();
    await expect(
      page.getByRole('dialog', { name: 'Edit approval policy' }),
    ).toBeVisible();
    await expect(
      page.getByRole('combobox', { name: /L0 ·/ }),
    ).toBeVisible();
    await expect(page.getByLabel('Allowed environments')).toHaveCount(0);
    await expect(page.getByLabel('Max tool calls')).toHaveCount(0);
  });

  test('creates and activates a scoped prompt override', async ({ page }) => {
    await page.goto('/intelligence/settings/prompts');

    await expect(
      page.getByRole('heading', { name: 'Prompt management', level: 2 }),
    ).toBeVisible();
    const builtinRow = page
      .locator('article')
      .filter({ hasText: 'Root-cause investigation' });
    await builtinRow.getByRole('button', { name: 'Create override' }).click();

    const drawer = page.getByRole('dialog', {
      name: 'Create prompt override',
    });
    await expect(drawer).toBeVisible();
    await drawer
      .getByLabel('Name')
      .fill('Production root-cause investigation');
    await drawer
      .getByLabel('Prompt body')
      .fill(
        'Investigate {{ streams }} over {{ time_range }} and prioritize production evidence.',
      );
    await drawer.getByRole('button', { name: 'Save' }).click();

    const customRow = page
      .locator('article')
      .filter({ hasText: 'Production root-cause investigation' });
    await expect(customRow).toBeVisible();
    await expect(customRow.getByText('Organization')).toBeVisible();
    await expect(customRow.getByText('Effective')).toBeVisible();
  });

  test('edits an investigation and an automation', async ({ page }) => {
    await page.goto('/intelligence/investigations');
    await page.getByRole('button', { name: 'Edit investigation' }).click();
    await page.getByLabel('Current step').fill('Verify recovery');
    await page.getByRole('button', { name: 'Save' }).click();
    await expect(page.getByText('Verify recovery')).toBeVisible();

    await page.goto('/intelligence/automations');
    await page.getByRole('button', { name: 'Edit' }).click();
    await page
      .getByLabel('Description')
      .fill('Editable critical-alert investigation workflow');
    await page.getByRole('button', { name: 'Save' }).click();
    await expect(
      page.getByText('Editable critical-alert investigation workflow'),
    ).toBeVisible();
  });
});
