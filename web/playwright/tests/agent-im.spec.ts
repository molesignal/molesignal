import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

test.describe('Mole Agent in IM', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('lists the app connectors that answer in IM and how each is doing', async ({ page }) => {
    await page.goto('/agent/settings/im');

    const feishu = page.locator('[data-im-bot="conn-feishu"]').locator('xpath=ancestor::tr');
    await expect(feishu).toContainText('Ops Feishu');
    await expect(feishu.locator('[data-im-state="connected"]')).toContainText('Connected');
    await expect(feishu.locator('[data-im-state="connected"]')).toContainText('Mole · node-1');

    const slack = page.locator('[data-im-bot="conn-slack"]').locator('xpath=ancestor::tr');
    await expect(slack.locator('[data-im-state="missing"]')).toHaveText('Needs an app-level token');
    await expect(slack.getByRole('switch', { name: 'Answer in IM with Ops Slack' })).toBeDisabled();

    const dingtalk = page.locator('[data-im-bot="conn-dingtalk"]').locator('xpath=ancestor::tr');
    await expect(dingtalk.locator('[data-im-state="error"]')).toContainText('invalid clientSecret');

    const wecom = page.locator('[data-im-bot="conn-wecom"]').locator('xpath=ancestor::tr');
    await expect(wecom).toContainText('WeCom');
    await expect(wecom.locator('[data-im-state="connected"]')).toContainText('aibOps · node-2');

    const chat = page.locator('[data-im-bot="conn-google-chat"]').locator('xpath=ancestor::tr');
    await expect(chat.locator('[data-im-state="missing"]')).toHaveText('Needs a Pub/Sub subscription');

    // Teams posts to a public callback: the row says where, ready to copy.
    const teams = page.locator('[data-im-bot="conn-teams"]').locator('xpath=ancestor::tr');
    await expect(teams.locator('[data-im-callback]')).toContainText(
      'https://molesignal.example.com/api/v1/public/im/teams/conn-teams/messages',
    );
    await expect(teams.getByRole('button', { name: 'Copy the callback address' })).toBeVisible();
    await expect(page.locator('[data-im-callback]')).toHaveCount(1);
  });

  test('turns a bot off and sends new group threads to a team', async ({ page }) => {
    await page.goto('/agent/settings/im');

    const dingtalk = page.locator('[data-im-bot="conn-dingtalk"]').locator('xpath=ancestor::tr');
    const off = page.waitForRequest(
      (request) => request.method() === 'PUT' && request.url().endsWith('/api/v1/agent/im/bots/conn-dingtalk'),
    );
    await dingtalk.getByRole('switch', { name: 'Answer in IM with Ops DingTalk' }).click();
    expect((await off).postDataJSON()).toEqual({ enabled: false, defaultTeamId: null });
    await expect(dingtalk.locator('[data-im-state="off"]')).toHaveText('Off');

    const feishu = page.locator('[data-im-bot="conn-feishu"]').locator('xpath=ancestor::tr');
    const moved = page.waitForRequest(
      (request) => request.method() === 'PUT' && request.url().endsWith('/api/v1/agent/im/bots/conn-feishu'),
    );
    await feishu.getByRole('combobox', { name: 'Where new group threads of Ops Feishu go' }).click();
    await page.getByRole('option', { name: 'Payments' }).click();
    expect((await moved).postDataJSON()).toEqual({ enabled: true, defaultTeamId: 'team-payments' });
  });

  test('links an IM account from a one-time link, once', async ({ page }) => {
    await page.goto('/agent/im/link?token=valid-token');

    const offer = page.locator('[data-im-link]');
    await expect(offer).toContainText('Link the Feishu account Alice (ou_alice) to your MoleSignal account?');
    await offer.getByRole('button', { name: 'Link' }).click();
    await expect(page.getByText('Linked', { exact: true })).toBeVisible();
    await expect(page.getByText('Go back to Feishu and ask again.')).toBeVisible();

    await page.goto('/agent/im/link?token=valid-token');
    await expect(page.getByText('This link was used or has expired')).toBeVisible();
  });

  test('offers the new providers in the connector editor', async ({ page }) => {
    await page.route('**/api/v1/notify/connector-types', (route) =>
      route.fulfill({
        json: ['dingtalk_app', 'wecom_app', 'wecom_bot', 'teams_app', 'google_chat_app', 'google_chat_webhook'].map(
          (connectorType) => ({
            connectorType: connectorType,
            capabilities: {
              directUser: !connectorType.endsWith('_webhook'),
              group: true,
              richText: true,
              interactive: false,
              acknowledgement: false,
              attachments: false,
            },
          }),
        ),
      }),
    );
    await page.goto('/settings/notify/connectors');
    await page.getByRole('button', { name: 'New connector' }).click();

    const drawer = page.getByRole('dialog');
    // The picker lists the types by channel in the interface's order, so DingTalk is chosen, not counted on.
    await drawer.getByRole('combobox').first().click();
    await page.getByRole('option', { name: 'DingTalk app' }).click();
    await expect(drawer).toContainText('Client secret');
    await expect(drawer).toContainText('openConversationId');
    await expect(drawer).toContainText('Card template ID');

    // A WeCom app takes the presses on its alert cards at a callback URL, shown once it is saved.
    await drawer.getByRole('combobox').first().click();
    await page.getByRole('option', { name: 'WeCom app' }).click();
    await expect(drawer).toContainText('Callback token');
    await expect(drawer).toContainText('EncodingAESKey');
    await expect(drawer.locator('[data-connector-field="callbacks"]')).toContainText(
      'Shown here once the connector is saved.',
    );

    await drawer.getByRole('combobox').first().click();
    await page.getByRole('option', { name: 'Google Chat app' }).click();
    await expect(drawer).toContainText('Service account key (JSON)');
    await expect(drawer).toContainText('Pub/Sub subscription');
    await expect(drawer).toContainText('spaces/');

    // A WeCom smart bot answers in IM and delivers notifications over the same connection.
    await drawer.getByRole('combobox').first().click();
    await page.getByRole('option', { name: 'WeCom smart bot' }).click();
    await expect(drawer).toContainText('Bot ID');
    await expect(drawer).toContainText('Long connection secret');
    await expect(drawer).toContainText('the bot delivers notifications too');
    await expect(drawer.getByRole('button', { name: 'Test connection' })).toBeVisible();
    await expect(drawer).toContainText('Test target');
  });
});
