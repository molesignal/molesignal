import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const PHONE_TYPES = [
  'aliyun_voice',
  'aliyun_sms',
  'tencent_voice',
  'tencent_sms',
  'huawei_voice',
  'huawei_sms',
  'aws_sms',
  'aws_voice',
  'twilio_sms',
  'twilio_voice',
  'http_sms',
];

const CAPABILITIES = {
  directUser: true,
  group: false,
  richText: false,
  interactive: false,
  acknowledgement: false,
  attachments: false,
};

const aliyun = (service: string) => [
  {
    field: 'apiBaseUrl',
    choices: [{ key: 'default', url: `https://${service}.aliyuncs.com` }],
    byRegion: { service, domains: ['aliyuncs.com'] },
  },
];

const aws = [
  {
    field: 'apiBaseUrl',
    choices: [],
    byRegion: { service: 'sms-voice', domains: ['amazonaws.com', 'amazonaws.com.cn'] },
  },
];

const twilio = [{ field: 'apiBaseUrl', choices: [{ key: 'default', url: 'https://api.twilio.com' }] }];

const tencent = (service: string) => [
  {
    field: 'apiBaseUrl',
    choices: [{ key: 'default', url: `https://${service}.tencentcloudapi.com` }],
    byRegion: { service, domains: ['tencentcloudapi.com'] },
  },
];

/** The provider addresses the backend lists for each type, which the drawer shows and picks from. */
const ADDRESSES: Record<string, unknown[]> = {
  aliyun_voice: aliyun('dyvmsapi'),
  aliyun_sms: aliyun('dysmsapi'),
  tencent_voice: tencent('vms'),
  tencent_sms: tencent('sms'),
  aws_sms: aws,
  aws_voice: aws,
  twilio_voice: twilio,
  twilio_sms: twilio,
  lark_app: [
    {
      field: 'apiBaseUrl',
      picker: 'site',
      choices: [
        { key: 'feishu', url: 'https://open.feishu.cn/open-apis' },
        { key: 'lark', url: 'https://open.larksuite.com/open-apis' },
      ],
    },
  ],
  slack_app: [
    {
      field: 'apiBaseUrl',
      picker: 'site',
      choices: [
        { key: 'slack', url: 'https://slack.com/api' },
        { key: 'govslack', url: 'https://slack-gov.com/api' },
      ],
    },
  ],
  teams_app: [
    {
      field: 'serviceUrl',
      picker: 'cloud',
      choices: [
        { key: 'global', url: 'https://smba.trafficmanager.net/teams/' },
        { key: 'gcc', url: 'https://smba.infra.gcc.teams.microsoft.com/teams' },
        { key: 'gcc_high', url: 'https://smba.infra.gov.teams.microsoft.us/teams' },
        { key: 'dod', url: 'https://smba.infra.dod.teams.microsoft.us/teams' },
      ],
    },
  ],
  wecom_bot: [{ field: 'wsUrl', choices: [{ key: 'default', url: 'wss://openws.work.weixin.qq.com' }] }],
};

/** The connector types as the backend lists them. */
const types = (names: string[]) =>
  names.map((connectorType) => ({
    connectorType: connectorType,
    capabilities: CAPABILITIES,
    addresses: ADDRESSES[connectorType] ?? [],
  }));

/** The regions each provider lists, as the backend passes them on. */
const REGIONS: Record<string, Array<{ id: string; name: string; apiBaseUrl: string }>> = {
  aliyun_sms: [
    { id: 'cn-hangzhou', name: 'China (Hangzhou)', apiBaseUrl: 'https://dysmsapi.aliyuncs.com' },
    { id: 'ap-southeast-1', name: 'Singapore', apiBaseUrl: 'https://dysmsapi.ap-southeast-1.aliyuncs.com' },
  ],
  tencent_sms: [
    { id: 'ap-guangzhou', name: 'South China (Guangzhou)', apiBaseUrl: 'https://sms.ap-guangzhou.tencentcloudapi.com' },
    { id: 'ap-beijing', name: 'North China (Beijing)', apiBaseUrl: 'https://sms.ap-beijing.tencentcloudapi.com' },
  ],
  aws_sms: [
    { id: 'eu-west-1', name: 'eu-west-1', apiBaseUrl: 'https://sms-voice.eu-west-1.amazonaws.com' },
    { id: 'us-east-1', name: 'us-east-1', apiBaseUrl: 'https://sms-voice.us-east-1.amazonaws.com' },
  ],
};

test.describe('Voice and SMS connectors', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.route('**/api/v1/notify/connector-types', (route) =>
      route.fulfill({ json: types(PHONE_TYPES) }),
    );
    await page.route('**/api/v1/notify/aws-external-id', (route) =>
      route.fulfill({ json: { externalId: 'molesignal-org-1' } }),
    );
    await page.route('**/api/v1/notify/connector-regions', (route) => {
      const asked = route.request().postDataJSON() as { connectorType: string };
      return route.fulfill({ json: { regions: REGIONS[asked.connectorType] ?? [] } });
    });
  });

  test('leaves the Alibaba Cloud template to notify templates and calls a phone number', async ({ page }) => {
    await page.goto('/settings/notify/connectors');
    await page.getByRole('button', { name: 'New connector' }).click();

    const drawer = page.getByRole('dialog');
    await drawer.getByRole('combobox').first().click();
    await page.getByRole('option', { name: 'Alibaba Cloud voice' }).click();
    await expect(drawer).toContainText('Calls numbers in mainland China only');
    await expect(drawer).toContainText('write its code and variables into a phone notify template');
    await expect(drawer.getByLabel('Template code')).toHaveCount(0);
    await expect(drawer.getByLabel('Caller ID')).toBeVisible();
    await expect(drawer).toContainText('A phone number with + and the country code');

    await drawer.getByRole('combobox').first().click();
    await page.getByRole('option', { name: 'Twilio SMS' }).click();
    await expect(drawer.getByLabel('Sender')).toBeVisible();
    await expect(drawer).toContainText('A2P 10DLC');
  });

  test('a Tencent Cloud connector asks for its app and lists regions once its keys are in', async ({ page }) => {
    let asked: Record<string, unknown> | null = null;
    await page.route('**/api/v1/notify/connector-regions', (route) => {
      asked = route.request().postDataJSON() as Record<string, unknown>;
      return route.fulfill({ json: { regions: REGIONS.tencent_sms } });
    });

    await page.goto('/settings/notify/connectors');
    await page.getByRole('button', { name: 'New connector' }).click();

    const drawer = page.getByRole('dialog');
    await drawer.getByRole('combobox').first().click();
    await page.getByRole('option', { name: 'Tencent Cloud SMS' }).click();
    await expect(drawer).toContainText('naming the variables 1, 2, 3 by position');
    await expect(drawer).toContainText('6 characters per variable by default');
    await expect(drawer.getByLabel('Template code')).toHaveCount(0);

    // The account lists its regions: nothing to ask with until the keys are in.
    await expect(drawer).toContainText('Fill in the credentials');
    await drawer.getByLabel('SecretId').fill('AKID-1');
    await drawer.getByLabel('SecretKey').fill('secret');
    await expect.poll(() => asked).not.toBeNull();
    await expect(drawer.locator('[data-connector-field="apiBaseUrl"]')).toContainText('sms');
    await expect(drawer.getByLabel('App ID (SdkAppId)')).toBeVisible();
  });

  test('a Huawei Cloud connector takes the address of its app, which no API lists', async ({ page }) => {
    await page.goto('/settings/notify/connectors');
    await page.getByRole('button', { name: 'New connector' }).click();

    const drawer = page.getByRole('dialog');
    await drawer.getByRole('combobox').first().click();
    await page.getByRole('option', { name: 'Huawei Cloud voice' }).click();
    await expect(drawer).toContainText('32 bytes');
    await expect(drawer.getByLabel('AppKey')).toBeVisible();
    await expect(drawer.getByLabel('AppSecret')).toBeVisible();
    await expect(drawer.getByLabel('App address')).toBeEditable();
    await expect(drawer.getByLabel('Caller ID')).toBeVisible();

    await drawer.getByRole('combobox').first().click();
    await page.getByRole('option', { name: 'Huawei Cloud SMS' }).click();
    await expect(drawer.getByLabel('Channel number')).toBeVisible();
    await expect(drawer.getByLabel('Signature name')).toBeVisible();
  });

  test('lists the types under their channels and starts a new connector at the first', async ({ page }) => {
    await page.route('**/api/v1/notify/connector-types', (route) =>
      route.fulfill({
        json: types(['aliyun_sms', 'email_smtp', 'lark_app', 'slack_app', 'twilio_voice', 'webhook']),
      }),
    );
    await page.goto('/settings/notify/connectors');
    await page.getByRole('button', { name: 'New connector' }).click();

    // In English the international platforms come first.
    const type = page.getByRole('dialog').getByRole('combobox').first();
    await expect(type).toContainText('Slack app');
    await type.click();
    await expect(page.getByRole('option')).toHaveText(['Slack app', 'Lark app', 'Twilio voice', 'Alibaba Cloud SMS', 'Email SMTP', 'Webhook']);
    await expect(page.getByRole('group', { name: 'Instant messaging' }).getByRole('option')).toHaveText(['Slack app', 'Lark app']);
    await expect(page.getByRole('group', { name: 'Phone calls' })).toContainText('Twilio voice');
    await expect(page.getByRole('group', { name: 'SMS' })).toContainText('Alibaba Cloud SMS');
  });

  test('asks an AWS connector only for what its way of signing needs', async ({ page }) => {
    await page.goto('/settings/notify/connectors');
    await page.getByRole('button', { name: 'New connector' }).click();

    const drawer = page.getByRole('dialog');
    await drawer.getByRole('combobox').first().click();
    await page.getByRole('option', { name: 'AWS SMS' }).click();
    await expect(drawer.getByLabel('Access key ID')).toBeVisible();
    await expect(drawer.getByLabel('Role ARN')).toHaveCount(0);

    await drawer.getByRole('combobox', { name: 'Authentication' }).click();
    await page.getByRole('option', { name: 'Assume a role' }).click();
    await expect(drawer.getByLabel('Role ARN')).toBeVisible();
    await expect(drawer.getByLabel('Session token')).toHaveCount(0);
    await expect(drawer).toContainText('molesignal-org-1');
    await expect(drawer).toContainText('require the external ID below');

    await drawer.getByRole('combobox', { name: 'Authentication' }).click();
    await page.getByRole('option', { name: 'Node identity' }).click();
    await expect(drawer.getByLabel('Access key ID')).toHaveCount(0);
    await expect(drawer).not.toContainText('molesignal-org-1');
  });

  test('chooses an Alibaba Cloud region from the list the provider gives, which sets the address', async ({ page }) => {
    await page.goto('/settings/notify/connectors');
    await page.getByRole('button', { name: 'New connector' }).click();

    const drawer = page.getByRole('dialog');
    await drawer.getByRole('combobox').first().click();
    await page.getByRole('option', { name: 'Alibaba Cloud SMS' }).click();

    // The address is shown to copy, not to edit.
    const address = drawer.locator('[data-connector-field="apiBaseUrl"]');
    await expect(address).toContainText('https://dysmsapi.aliyuncs.com');
    await expect(address.getByRole('button', { name: 'Copy' })).toBeVisible();
    await expect(address.getByRole('textbox')).toHaveCount(0);

    const region = drawer.getByRole('combobox', { name: 'Region', exact: true });
    await expect(region).toContainText('China (Hangzhou) · cn-hangzhou');
    await region.click();
    await page.getByRole('option', { name: 'Singapore · ap-southeast-1' }).click();
    await expect(address).toContainText('https://dysmsapi.ap-southeast-1.aliyuncs.com');
  });

  test('lists the AWS regions of the account once its key is filled in', async ({ page }) => {
    const asked: Array<Record<string, unknown>> = [];
    await page.route('**/api/v1/notify/connector-regions', (route) => {
      const body = route.request().postDataJSON() as { connectorType: string; config: Record<string, unknown> };
      asked.push(body.config);
      return route.fulfill({ json: { regions: REGIONS[body.connectorType] ?? [] } });
    });

    await page.goto('/settings/notify/connectors');
    await page.getByRole('button', { name: 'New connector' }).click();

    const drawer = page.getByRole('dialog');
    await drawer.getByRole('combobox').first().click();
    await page.getByRole('option', { name: 'AWS SMS' }).click();
    await expect(drawer).toContainText('Fill in the credentials to list the regions this account has enabled.');

    await drawer.getByLabel('Access key ID').fill('AKIAEXAMPLE');
    await drawer.getByLabel('Secret access key').fill('secret');

    const region = drawer.getByRole('combobox', { name: 'Region', exact: true });
    await expect(drawer).not.toContainText('Fill in the credentials');
    await region.click();
    await page.getByRole('option', { name: 'eu-west-1' }).click();
    await expect(drawer.locator('[data-connector-field="apiBaseUrl"]')).toContainText(
      'https://sms-voice.eu-west-1.amazonaws.com',
    );
    expect(asked.at(-1)).toMatchObject({ auth: 'access_key', accessKey: 'AKIAEXAMPLE', secretKey: 'secret' });
  });

  test('keeps the type of a saved connector and shows its region', async ({ page }) => {
    await page.route('**/api/v1/notify/connectors', (route) =>
      route.fulfill({
        json: [
          {
            id: 'conn-sms',
            organizationId: 'org-1',
            name: 'Ops SMS',
            connectorType: 'aliyun_sms',
            config: {
              accessKeyId: 'LTAI-id',
              accessKeySecret: '***',
              signName: '墨信',
              region: 'ap-southeast-1',
              apiBaseUrl: 'https://dysmsapi.ap-southeast-1.aliyuncs.com',
              timeoutSecs: 10,
            },
            capabilities: CAPABILITIES,
            enabled: true,
            status: 'unknown',
            createdAt: 1,
            updatedAt: 1,
          },
        ],
      }),
    );

    await page.goto('/settings/notify/connectors');
    await page.getByText('Ops SMS').click();

    const drawer = page.getByRole('dialog');
    await expect(drawer.locator('[data-connector-field="type"]')).toContainText('Alibaba Cloud SMS');
    await expect(drawer.getByRole('combobox', { name: 'Connector type' })).toHaveCount(0);
    await expect(drawer.getByRole('combobox', { name: 'Region', exact: true })).toContainText('Singapore · ap-southeast-1');
    await expect(drawer.locator('[data-connector-field="apiBaseUrl"]')).toContainText(
      'https://dysmsapi.ap-southeast-1.aliyuncs.com',
    );
  });

  test('acts on replies to a Twilio text and shows where Twilio sends them', async ({ page }) => {
    const connector = {
      id: 'conn-twilio',
      organizationId: 'org-1',
      name: 'Ops texts',
      connectorType: 'twilio_sms',
      config: {
        accountSid: 'AC123',
        authToken: '***',
        sender: '+15550100',
        apiBaseUrl: 'https://api.twilio.com',
        timeoutSecs: 10,
      },
      capabilities: CAPABILITIES,
      enabled: true,
      status: 'unknown',
      createdAt: 1,
      updatedAt: 1,
    };
    let saved: Record<string, unknown> | null = null;
    await page.route('**/api/v1/notify/connectors', (route) => route.fulfill({ json: [connector] }));
    await page.route('**/api/v1/notify/connectors/conn-twilio', (route) => {
      saved = route.request().postDataJSON() as Record<string, unknown>;
      return route.fulfill({ json: { ...connector, ...saved } });
    });
    await page.route('**/api/v1/notify/connectors/conn-twilio/callbacks', (route) =>
      route.fulfill({
        json: {
          externalUrlSet: true,
          repliesUrl: 'https://ms.example.com/api/v1/public/notify/conn-twilio/replies',
          keypressUrl: null,
        },
      }),
    );

    await page.goto('/settings/notify/connectors');
    await page.getByText('Ops texts').click();

    const drawer = page.getByRole('dialog');
    const replies = drawer.getByRole('switch', { name: 'Act on replies' });
    const callbacks = drawer.locator('[data-connector-field="callbacks"]');
    await expect(replies).not.toBeChecked();
    await expect(callbacks).toHaveCount(0);

    // On, it says what a reply does and where Twilio is to send the replies.
    await replies.click();
    await expect(drawer.locator('[data-connector-field="replies"]')).toContainText(
      'such as 31 to acknowledge or 32 to resolve',
    );
    await expect(callbacks).toContainText('https://ms.example.com/api/v1/public/notify/conn-twilio/replies');
    await expect(callbacks.getByRole('button', { name: 'Copy' })).toBeVisible();
    await expect(callbacks).toContainText('A message comes in');

    await drawer.getByRole('button', { name: 'Save connector' }).click();
    await expect.poll(() => saved).toMatchObject({ config: { replies: true, sender: '+15550100' } });
  });

  test('takes keypresses on an Alibaba Cloud call only from a real number', async ({ page }) => {
    await page.goto('/settings/notify/connectors');
    await page.getByRole('button', { name: 'New connector' }).click();

    const drawer = page.getByRole('dialog');
    await drawer.getByRole('combobox').first().click();
    await page.getByRole('option', { name: 'Alibaba Cloud voice' }).click();
    await drawer.getByRole('textbox', { name: 'Name', exact: true }).fill('Ops calls');
    await drawer.getByLabel('AccessKey ID').fill('LTAI-id');
    await drawer.getByLabel('AccessKey secret').fill('secret');

    const save = drawer.getByRole('button', { name: 'Save connector' });
    await expect(save).toBeEnabled();

    await drawer.getByRole('switch', { name: 'Act on keypresses' }).click();
    await expect(drawer.locator('[data-connector-field="replies"]')).toContainText('presses 1 to acknowledge or 2 to resolve');
    await expect(drawer.locator('[data-connector-field="callbacks"]')).toContainText(
      'Shown here once the connector is saved.',
    );
    await expect(save).toBeDisabled();

    await drawer.getByLabel('Caller ID').fill('057188773344');
    await expect(save).toBeEnabled();
  });

  test('picks the Feishu or Lark address by platform', async ({ page }) => {
    await page.route('**/api/v1/notify/connector-types', (route) =>
      route.fulfill({ json: types(['lark_app']) }),
    );

    await page.goto('/settings/notify/connectors');
    await page.getByRole('button', { name: 'New connector' }).click();

    const drawer = page.getByRole('dialog');
    const address = drawer.locator('[data-connector-field="apiBaseUrl"]');
    await expect(address).toContainText('https://open.feishu.cn/open-apis');

    await drawer.getByRole('combobox', { name: 'Platform' }).click();
    await page.getByRole('option', { name: 'Lark', exact: true }).click();
    await expect(address).toContainText('https://open.larksuite.com/open-apis');
  });

  test('picks the Teams cloud, and every IM service address is shown, not edited', async ({ page }) => {
    await page.route('**/api/v1/notify/connector-types', (route) =>
      route.fulfill({ json: types(['teams_app', 'slack_app', 'wecom_bot']) }),
    );

    await page.goto('/settings/notify/connectors');
    await page.getByRole('button', { name: 'New connector' }).click();

    const drawer = page.getByRole('dialog');
    await drawer.getByRole('combobox').first().click();
    await page.getByRole('option', { name: 'Microsoft Teams bot' }).click();

    // The service URL follows from the cloud: it can be copied, not edited.
    const service = drawer.locator('[data-connector-field="serviceUrl"]');
    await expect(service).toContainText('https://smba.trafficmanager.net/teams/');
    await expect(service.getByRole('textbox')).toHaveCount(0);
    await expect(service.getByRole('button', { name: 'Copy' })).toBeVisible();

    await drawer.getByRole('combobox', { name: 'Cloud' }).click();
    await page.getByRole('option', { name: 'GCC High' }).click();
    await expect(service).toContainText('https://smba.infra.gov.teams.microsoft.us/teams');
    await expect(drawer).toContainText('tokens come from login.microsoftonline.us');

    // Slack or GovSlack, and the WeCom bot's long connection address, likewise.
    await drawer.getByRole('combobox').first().click();
    await page.getByRole('option', { name: 'Slack app' }).click();
    const api = drawer.locator('[data-connector-field="apiBaseUrl"]');
    await expect(api).toContainText('https://slack.com/api');
    await drawer.getByRole('combobox', { name: 'Platform' }).click();
    await page.getByRole('option', { name: 'GovSlack' }).click();
    await expect(api).toContainText('https://slack-gov.com/api');

    await drawer.getByRole('combobox').first().click();
    await page.getByRole('option', { name: 'WeCom smart bot' }).click();
    const ws = drawer.locator('[data-connector-field="wsUrl"]');
    await expect(ws).toContainText('wss://openws.work.weixin.qq.com');
    await expect(ws.getByRole('textbox')).toHaveCount(0);
  });

  test('writes a generic HTTP SMS request with placeholders, and a GET carries no body', async ({ page }) => {
    await page.goto('/settings/notify/connectors');
    await page.getByRole('button', { name: 'New connector' }).click();

    const drawer = page.getByRole('dialog');
    await drawer.getByRole('combobox').first().click();
    await page.getByRole('option', { name: 'Generic HTTP SMS' }).click();

    // The hint names the placeholders as they are written.
    await expect(drawer).toContainText('{{phone}} is the number, {{content}} the text');
    await expect(drawer.getByLabel('Gateway URL')).toBeVisible();
    await expect(drawer.getByLabel('Secret', { exact: true })).toHaveAttribute('type', 'password');
    await expect(drawer.getByRole('textbox', { name: 'Body', exact: true })).toHaveValue('{"phone": "{{phone}}", "content": "{{content}}"}');
    await expect(drawer).toContainText('A phone number with + and the country code');

    await drawer.getByRole('combobox', { name: 'HTTP method' }).click();
    await page.getByRole('option', { name: 'GET' }).click();
    await expect(drawer.getByRole('textbox', { name: 'Body', exact: true })).toHaveCount(0);
    await expect(drawer.getByRole('combobox', { name: 'Body format' })).toHaveCount(0);
  });

  test('tests an Alibaba Cloud connector with an SMS template that carries its template', async ({ page }) => {
    const tested: Array<Record<string, unknown>> = [];
    await page.route('**/api/v1/notify/connectors', (route) =>
      route.fulfill({
        json: [
          {
            id: 'conn-sms',
            organizationId: 'org-1',
            name: 'Ops SMS',
            connectorType: 'aliyun_sms',
            config: {
              accessKeyId: 'LTAI-id',
              accessKeySecret: '***',
              signName: '墨信',
              region: 'cn-hangzhou',
              apiBaseUrl: 'https://dysmsapi.aliyuncs.com',
              timeoutSecs: 10,
            },
            capabilities: CAPABILITIES,
            enabled: true,
            status: 'unknown',
            createdAt: 1,
            updatedAt: 1,
          },
        ],
      }),
    );
    const template = (id: string, name: string, channel: string | null, aliyun: boolean) => ({
      id,
      organizationId: 'org-1',
      name,
      body: '{{rule.name}}',
      format: 'text',
      category: 'alert',
      channel,
      providerTemplates: aliyun ? { aliyun: { code: 'SMS_1', params: { name: '{{rule.name}}' } } } : {},
      createdAt: 1,
      updatedAt: 1,
    });
    await page.route('**/api/v1/notify/templates', (route) =>
      route.fulfill({
        json: [
          template('tpl-plain', 'Plain SMS', 'sms', false),
          template('tpl-aliyun', 'Alibaba Cloud SMS', 'sms', true),
          template('tpl-phone', 'Alibaba Cloud call', 'phone', true),
        ],
      }),
    );
    await page.route('**/api/v1/notify/connectors/conn-sms/test', (route) => {
      tested.push(route.request().postDataJSON() as Record<string, unknown>);
      return route.fulfill({ json: { sent: true, testedAt: 1, elapsedMs: 5, error: null } });
    });

    await page.goto('/settings/notify/connectors');
    await page.getByText('Ops SMS').click();

    // Only SMS templates that carry an Alibaba Cloud template can test it.
    const drawer = page.getByRole('dialog');
    const choice = drawer.getByRole('combobox', { name: /^Test template/ });
    await expect(choice).toContainText('Alibaba Cloud SMS');
    await choice.click();
    await expect(page.getByRole('option')).toHaveText(['Alibaba Cloud SMS']);
    await page.keyboard.press('Escape');

    await drawer.getByRole('textbox', { name: /^Target/ }).fill('13800138000');
    await drawer.getByRole('button', { name: 'Test connection' }).click();
    await expect.poll(() => tested.length).toBe(1);
    expect(tested[0]).toMatchObject({ target: '13800138000', templateId: 'tpl-aliyun' });
  });
});
