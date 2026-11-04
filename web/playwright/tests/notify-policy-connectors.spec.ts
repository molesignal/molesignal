/**
 * The connectors of a notify policy are a group of boxes, not one label: a box is ticked where it is
 * clicked, and the title of the list ticks none.
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const connector = (id: string, name: string, connectorType: string) => ({
  id,
  organizationId: 'org-1',
  name,
  connectorType: connectorType,
  config: {},
  capabilities: {
    directUser: true,
    group: false,
    richText: false,
    interactive: false,
    acknowledgement: false,
    attachments: false,
  },
  enabled: true,
  status: 'connected',
  createdAt: 1,
  updatedAt: 1,
});

const POLICY = {
  id: 'own-1',
  organizationId: 'org-1',
  name: 'Payments on call',
  builtinKey: null,
  eventType: 'alert.triggered',
  category: 'alert',
  matchers: {},
  recipientResolver: 'alert_owner',
  resolverConfig: {},
  deliveryMode: 'multi_connector',
  deliveryConfig: { connectorIds: [] },
  templateId: null,
  channelTemplates: {},
  fallbackConfig: { useUserFallbacks: true, useTeamDefaults: true, useOrganizationDefaults: true },
  ackTimeoutSeconds: null,
  escalationConfig: null,
  mergeWindowSecs: null,
  enabled: true,
  priority: 50,
  createdAt: 1,
  updatedAt: 1,
};

test('a connector is ticked where it is clicked, and the title of the list ticks none', async ({
  page,
  mockServer,
}) => {
  await mountMockRoutes(page, mockServer.port);

  await page.route(/\/api\/v1\/notify\/connectors(\?.*)?$/, (route) =>
    route.fulfill({ json: [connector('c-mail', 'Ops mail', 'email'), connector('c-hook', 'Ops hook', 'webhook')] }),
  );
  await page.route('**/api/v1/notify/policies', (route) => route.fulfill({ json: [POLICY] }));
  await page.route('**/api/v1/notify/policies/preview', (route) =>
    route.fulfill({ json: { policyId: 'own-1', matched: true, recipients: [] } }),
  );
  let saved: Record<string, unknown> | null = null;
  await page.route('**/api/v1/notify/policies/own-1', (route) => {
    if (route.request().method() !== 'PUT') return route.fallback();
    saved = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({ json: { ...POLICY, ...saved } });
  });

  await page.goto('/settings/notify/policies');
  await page.getByRole('row', { name: /Payments on call/ }).getByRole('button', { name: 'Edit' }).click();
  const drawer = page.getByRole('dialog', { name: /Payments on call/ });
  const connectors = drawer.getByRole('group', { name: 'Selected connectors' });
  const mail = connectors.getByRole('checkbox', { name: 'Ops mail' });
  const hook = connectors.getByRole('checkbox', { name: 'Ops hook' });

  await connectors.getByText('Ops hook', { exact: true }).click();
  await expect(hook).toBeChecked();
  await expect(mail).not.toBeChecked();

  await connectors.getByText('Selected connectors', { exact: true }).click();
  await expect(hook).toBeChecked();
  await expect(mail).not.toBeChecked();

  await drawer.getByRole('button', { name: 'Save policy' }).click();
  await expect.poll(() => saved).toMatchObject({
    deliveryMode: 'multi_connector',
    deliveryConfig: { connectorIds: ['c-hook'] },
  });
});
