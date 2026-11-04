import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const POLICY = {
  organizationId: 'org-1',
  eventType: 'alert.triggered',
  category: 'alert',
  matchers: {},
  recipientResolver: 'alert_owner',
  resolverConfig: {},
  deliveryMode: 'prefer_user',
  deliveryConfig: { connectorIds: [] },
  templateId: null,
  channelTemplates: {},
  fallbackConfig: { useUserFallbacks: true, useTeamDefaults: true, useOrganizationDefaults: true },
  ackTimeoutSeconds: null,
  escalationConfig: null,
  mergeWindowSecs: null,
  enabled: true,
  priority: 100,
  createdAt: 1,
  updatedAt: 1,
};

const BUILT_IN = {
  ...POLICY,
  id: 'builtin-1',
  name: 'Alert fired: notify the owner',
  builtinKey: 'alert_triggered',
  mergeWindowSecs: 60,
};

test.describe('Built-in notify policies', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('keep their name, are turned off rather than deleted, and go back to their default', async ({ page }) => {
    const restored: string[] = [];
    await page.route('**/api/v1/notify/policies', (route) =>
      route.fulfill({
        json: [BUILT_IN, { ...POLICY, id: 'own-1', name: 'Payments on call', builtinKey: null, priority: 50 }],
      }),
    );
    // An existing policy is previewed as soon as its editor opens.
    await page.route('**/api/v1/notify/policies/preview', (route) =>
      route.fulfill({ json: { policyId: 'builtin-1', matched: true, recipients: [] } }),
    );
    await page.route('**/api/v1/notify/policies/builtin-1/restore', (route) => {
      restored.push(route.request().method());
      return route.fulfill({ json: BUILT_IN });
    });

    await page.goto('/settings/notify/policies');

    const builtIn = page.getByRole('row', { name: /Alert fired: notify the owner/ });
    await expect(builtIn).toContainText('Built-in');
    await expect(builtIn.getByRole('button', { name: 'Delete' })).toBeDisabled();
    const own = page.getByRole('row', { name: /Payments on call/ });
    await expect(own).not.toContainText('Built-in');
    await expect(own.getByRole('button', { name: 'Delete' })).toBeEnabled();

    // The editor shows the name, not a box for it, and puts the policy back on request.
    await builtIn.getByRole('button', { name: 'Edit' }).click();
    const drawer = page.getByRole('dialog', { name: /Alert fired: notify the owner/ });
    await expect(drawer).toContainText('A built-in policy keeps its name.');
    await expect(drawer.getByRole('textbox', { name: 'Name', exact: true })).toHaveCount(0);

    await drawer.getByRole('button', { name: 'Restore default' }).click();
    await page
      .getByRole('dialog', { name: 'Restore the default settings?' })
      .getByRole('button', { name: 'Restore default' })
      .click();
    await expect.poll(() => restored).toEqual(['POST']);
    await expect(page.getByText('Default restored')).toBeVisible();
  });

  test('merge alert notifications within the window their policy sets', async ({ page }) => {
    let saved: Record<string, unknown> | null = null;
    await page.route('**/api/v1/notify/policies', (route) => route.fulfill({ json: [BUILT_IN] }));
    await page.route('**/api/v1/notify/policies/preview', (route) =>
      route.fulfill({ json: { policyId: 'builtin-1', matched: true, recipients: [] } }),
    );
    await page.route('**/api/v1/notify/policies/builtin-1', (route) => {
      saved = route.request().postDataJSON() as Record<string, unknown>;
      return route.fulfill({ json: { ...BUILT_IN, ...saved } });
    });

    await page.goto('/settings/notify/policies');
    await page.getByRole('row', { name: /Alert fired: notify the owner/ }).getByRole('button', { name: 'Edit' }).click();

    const drawer = page.getByRole('dialog', { name: /Alert fired: notify the owner/ });
    const merge = drawer.getByRole('combobox', { name: 'Merge notifications' });
    await expect(merge).toContainText('1 minute');
    await expect(drawer).toContainText('webhooks are never merged');

    await merge.click();
    await page.getByRole('option', { name: '5 minutes' }).click();
    await drawer.getByRole('button', { name: 'Save policy' }).click();
    await expect.poll(() => saved).toMatchObject({ mergeWindowSecs: 300 });
  });
});
