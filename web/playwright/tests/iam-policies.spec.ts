import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

/** Replaces what the policy editor holds with `text`, as typed. */
async function writeDocument(page: Page, text: string) {
  const editor = page
    .locator('.code-editor-shell')
    .filter({ has: page.getByRole('textbox', { name: 'Policy document', exact: true }) });
  await expect(editor.locator('.view-line').first()).toBeVisible();
  await editor.locator('.view-lines').click();
  await expect(page.getByRole('textbox', { name: 'Policy document', exact: true })).toBeFocused();

  // Monaco takes its keys from the platform the user agent names, not the machine's.
  const mac = await page.evaluate(() => navigator.userAgent.includes('Macintosh'));
  await page.keyboard.press(mac ? 'Meta+a' : 'Control+a');
  await page.keyboard.press('Backspace');
  await expect(editor.locator('.view-line')).toHaveCount(1);
  await page.keyboard.insertText(text);
}

test.describe('permission policies', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('reads a built-in policy and copies it into a custom one', async ({ page }) => {
    await page.goto('/iam/policies');
    await expect(page.getByRole('heading', { name: 'Permission policies', level: 2 })).toBeVisible();

    const row = page.getByRole('row').filter({ hasText: 'readonly_observer' });
    await expect(row).toContainText('Read-only observer');
    await expect(row).toContainText('Built-in');

    // Its permissions read as the role list shows them: three pills and how many more.
    const pills = row.locator('[data-permission-summary="some"] > span');
    await expect(pills).toHaveCount(4);
    await expect(pills.last()).toHaveText(/^\+\d+$/);
    await row.click();

    const drawer = page.getByRole('dialog', { name: 'View policy' });
    await expect(drawer.getByText(/^Built-in policies are read-only/)).toBeVisible();
    await expect(drawer.locator('[data-permission-list] li').first()).toBeVisible();
    await drawer.getByRole('button', { name: 'Copy as custom policy' }).click();

    const copy = page.getByRole('dialog', { name: 'New policy' });
    await expect(copy.getByRole('textbox', { name: 'Key' })).toHaveValue('readonly_observer_copy');
    await expect(copy.getByRole('textbox', { name: 'Name' })).toHaveValue('Read-only observer copy');
    await copy.getByRole('button', { name: 'Create policy' }).click();

    await expect(page.getByRole('row').filter({ hasText: 'readonly_observer_copy' })).toContainText(
      'Custom',
    );
  });

  test('a new policy starts with nothing ticked, and saves once something is', async ({ page }) => {
    await page.goto('/iam/policies');
    await page.getByRole('button', { name: 'New policy' }).click();
    const drawer = page.getByRole('dialog', { name: 'New policy' });
    await drawer.getByRole('textbox', { name: 'Name' }).fill('Dashboard viewers');

    // An empty document is a start, not a mistake: the boxes are there to tick.
    const picker = drawer.locator('[data-permission-picker]');
    const view = picker.getByRole('checkbox', { name: 'View dashboards' });
    const create = drawer.getByRole('button', { name: 'Create policy' });
    await expect(view).not.toBeChecked();
    await expect(drawer.getByRole('alert')).toHaveCount(0);
    await expect(create).toBeDisabled();

    await view.check();
    await expect(create).toBeEnabled();
    await view.uncheck();
    await expect(view).toBeVisible();
    await expect(create).toBeDisabled();

    await drawer.getByRole('button', { name: 'JSON', exact: true }).click();
    await expect(drawer.locator('[data-policy-allowed]')).toHaveText('Allows no permissions yet');
    await expect(drawer.getByRole('alert')).toHaveCount(0);

    await drawer.getByRole('button', { name: 'Visual', exact: true }).click();
    await view.check();
    await create.click();
    await expect(page.getByRole('row').filter({ hasText: 'dashboard_viewers' })).toContainText('Custom');
  });

  test('writes a policy as JSON, naming what is wrong before it is saved', async ({ page }) => {
    await page.goto('/iam/policies');
    await page.getByRole('button', { name: 'New policy' }).click();
    const drawer = page.getByRole('dialog', { name: 'New policy' });
    await drawer.getByRole('textbox', { name: 'Name' }).fill('Dashboard editors');
    await expect(drawer.getByRole('textbox', { name: 'Key' })).toHaveValue('dashboard_editors');

    await drawer.getByRole('button', { name: 'JSON', exact: true }).click();
    await writeDocument(
      page,
      '{"Version":"1","Statement":[{"Effect":"Allow","Action":["dashboards.*","alerts.reed"]}]}',
    );
    await expect(drawer.locator('[data-policy-problem]')).toHaveText(
      'Statement[0].Action[1]: no permission is called alerts.reed.',
    );
    await expect(drawer.getByRole('button', { name: 'Create policy' })).toBeDisabled();

    await writeDocument(
      page,
      '{"Version":"1","Statement":[{"Sid":"Dashboards","Effect":"Allow","Action":"dashboards.*"}]}',
    );
    await expect(drawer.locator('[data-policy-allowed]')).toHaveText(/^Allows \d+ permissions?$/);
    await expect(drawer.locator('[data-permission-list] li').first()).toBeVisible();

    // Ticking would write the pattern out, so the visual editor says so first.
    await drawer.getByRole('button', { name: 'Visual', exact: true }).click();
    await expect(drawer.getByText(/^This document uses wildcards/)).toBeVisible();

    await drawer.getByRole('button', { name: 'Create policy' }).click();
    const row = page.getByRole('row').filter({ hasText: 'dashboard_editors' });
    await expect(row).toContainText('Dashboard editors');
    await expect(row).toContainText('Custom');
  });

  test('a role attaches a policy and shows what it grants', async ({ page }) => {
    await page.goto('/iam/roles');
    await page.getByRole('button', { name: 'New role' }).click();
    const drawer = page.getByRole('dialog', { name: 'Create role' });
    await drawer.getByRole('textbox', { name: 'Name' }).fill('Collectors');

    await drawer
      .locator('[data-policy-checklist]')
      .getByRole('checkbox', { name: 'Data intake' })
      .check();

    // What the policy grants shows ticked in the role's own list, and stays ticked.
    const inherited = drawer.locator('[data-permission-picker] [data-permission-inherited]');
    await expect(inherited).toHaveCount(5);
    await expect(inherited.first()).toContainText('From Data intake');
    await expect(inherited.first().getByRole('checkbox')).toBeChecked();
    await expect(inherited.first().getByRole('checkbox')).toBeDisabled();

    await drawer.getByRole('button', { name: 'Create role' }).click();
    const row = page.getByRole('row').filter({ hasText: 'collectors' });
    await expect(row.locator('[data-role-policies]')).toHaveText('Data intake');
  });
});
