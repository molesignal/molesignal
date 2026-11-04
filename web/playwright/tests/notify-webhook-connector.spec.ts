/**
 * A webhook connector: what its requests carry, which body it sends, and the secret it signs them with.
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const CAPABILITIES = {
  directUser: false,
  group: true,
  richText: false,
  interactive: false,
  acknowledgement: false,
  attachments: false,
};

test('a webhook sends the event for machines and signs it with its secret', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  let created: Record<string, unknown> | null = null;
  await page.route('**/api/v1/notify/connector-types', (route) =>
    route.fulfill({ json: [{ connectorType: 'webhook', capabilities: CAPABILITIES, addresses: [] }] }),
  );
  await page.route('**/api/v1/notify/connectors', (route) => {
    if (route.request().method() !== 'POST') return route.fulfill({ json: [] });

    created = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({
      json: {
        id: 'conn-hook',
        organizationId: 'org-1',
        capabilities: CAPABILITIES,
        status: 'unknown',
        createdAt: 1,
        updatedAt: 1,
        ...created,
      },
    });
  });

  await page.goto('/settings/notify/connectors');
  await page.getByRole('button', { name: 'New connector' }).click();

  const drawer = page.getByRole('dialog', { name: 'New connector' });
  await drawer.getByRole('combobox').first().click();
  await page.getByRole('option', { name: 'Webhook', exact: true }).click();

  // Every request names the notification and the delivery; a secret adds the signature.
  await expect(drawer).toContainText('X-MoleSignal-Delivery');
  await expect(drawer).toContainText('X-MoleSignal-Sign: sha256=<hex>');

  await drawer.getByRole('textbox', { name: 'Name', exact: true }).fill('Incident hook');
  await drawer.getByLabel('Default URL').fill('https://hooks.example.com/molesignal');

  // The event for machines to read unless the policy's template is chosen.
  const body = drawer.getByRole('combobox', { name: /^Body/ });
  await expect(body).toContainText('MoleSignal event');
  await expect(drawer).toContainText('The body is the event for machines to read');
  await body.click();
  await page.getByRole('option', { name: 'Notify template body' }).click();
  await expect(drawer).toContainText('a JSON template is sent as JSON with its values escaped');
  await body.click();
  await page.getByRole('option', { name: 'MoleSignal event' }).click();

  await drawer.getByLabel('Signing secret').fill('0123456789abcdef0123');
  await drawer.getByRole('button', { name: 'Save connector' }).click();

  await expect.poll(() => created).toMatchObject({
    name: 'Incident hook',
    connectorType: 'webhook',
    config: {
      url: 'https://hooks.example.com/molesignal',
      method: 'post',
      body: 'event',
      signingSecret: '0123456789abcdef0123',
      headers: {},
      timeoutSecs: 10,
    },
  });
});
