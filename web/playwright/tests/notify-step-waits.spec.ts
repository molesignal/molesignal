import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const capabilities = {
  directUser: true,
  group: false,
  richText: false,
  interactive: false,
  acknowledgement: false,
  attachments: false,
};

const connector = (id: string, name: string, connectorType: string) => ({
  id,
  organizationId: 'org-1',
  name,
  connectorType: connectorType,
  config: {},
  capabilities,
  enabled: true,
  status: 'connected',
  lastTestedAt: null,
  lastTestStatus: null,
  lastTestError: null,
  createdAt: 1,
  updatedAt: 1,
});

const endpoint = (id: string, connectorId: string, externalIdentity: string) => ({
  id,
  organizationId: 'org-1',
  userId: 'user-1',
  connectorId: connectorId,
  providerType: 'fake',
  externalIdentity: externalIdentity,
  displayName: null,
  metadata: {},
  verified: true,
  enabled: true,
  createdAt: 1,
  updatedAt: 1,
});

test.describe('Personal notify order', () => {
  test('a later alert step waits the minutes it is given', async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.route('**/api/v1/notify/connectors', (route) =>
      route.fulfill({
        json: [connector('c-im', 'Lark', 'lark_app'), connector('c-voice', 'Duty phone', 'aliyun_voice')],
      }),
    );
    await page.route('**/api/v1/users/*/notify-endpoints', (route) =>
      route.fulfill({
        json: [endpoint('e-im', 'c-im', 'ou_alice'), endpoint('e-voice', 'c-voice', '+8613800138000')],
      }),
    );
    await page.route('**/api/v1/users/*/notify-preferences', (route) =>
      route.fulfill({
        json: [
          {
            id: 'p-alert',
            organizationId: 'org-1',
            userId: 'user-1',
            category: 'alert',
            enabled: true,
            quietHours: null,
            allowHighUrgencyBypass: true,
            steps: [
              { id: 's1', preferenceId: 'p-alert', endpointId: 'e-im', stepOrder: 1, delaySecs: 0, createdAt: 1 },
              { id: 's2', preferenceId: 'p-alert', endpointId: 'e-voice', stepOrder: 2, delaySecs: 300, createdAt: 1 },
            ],
            createdAt: 1,
            updatedAt: 1,
          },
        ],
      }),
    );
    let saved: unknown = null;
    await page.route('**/api/v1/users/*/notify-preferences/alert', async (route) => {
      saved = route.request().postDataJSON();
      await route.fulfill({ json: {} });
    });

    await page.goto('/account/settings/notify');

    const wait = page.getByRole('textbox', { name: 'Minutes before step 2' });
    await expect(wait).toHaveValue('5');
    // Only alerts wait for someone to acknowledge them: one minutes field, the alert order's.
    await expect(page.getByRole('textbox', { name: /Minutes before step/ })).toHaveCount(1);

    await wait.fill('3');
    // Low urgency has its own list: the phone alone, with no wait before it.
    await page.locator('[data-preference="alert"] [data-step-list="low"] button', { hasText: '+8613800138000' }).click();
    await page.getByRole('button', { name: 'Save Alert events' }).click();
    await expect.poll(() => saved).not.toBeNull();
    expect(saved).toMatchObject({
      endpointIds: ['e-im', 'e-voice'],
      stepDelaysSecs: [0, 180],
      lowEndpointIds: ['e-voice'],
      lowStepDelaysSecs: [0],
    });
  });
});
