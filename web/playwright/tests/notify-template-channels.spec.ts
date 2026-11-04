import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

/** A template as the backend lists it. */
function template(
  id: string,
  name: string,
  channel: string | null,
  extra: Record<string, unknown> = {},
) {
  return {
    id,
    organizationId: 'org-1',
    name,
    body: channel === 'sms' ? '[{{severity}}] {{rule.name}}' : '**{{rule.name}}**',
    format: channel === 'sms' || channel === 'phone' ? 'text' : 'markdown',
    category: 'alert',
    channel,
    providerTemplates: {},
    createdAt: 1,
    updatedAt: 1,
    ...extra,
  };
}

const TEMPLATES = [
  template('tpl-im', 'IM card', 'im'),
  template('tpl-sms', 'Alert SMS', 'sms', {
    providerTemplates: { aliyun: { code: 'SMS_1', params: { name: '{{rule.name}}' } } },
  }),
  template('tpl-general', 'Alert markdown', null),
];

const FIELDS = {
  fields: [
    {
      key: 'rule_name',
      token: '{{rule.name}}',
      group: 'rule',
      categories: ['alert'],
      example: 'High error rate · api',
      eventTypes: [],
    },
  ],
  presets: [
    {
      key: 'alert_sms',
      category: 'alert',
      channel: 'sms',
      eventType: 'alert.triggered',
      format: 'text',
      body: '[{{severity}}] {{rule.name}}: {{incident.summary}}',
    },
    {
      key: 'alert_markdown',
      category: 'alert',
      channel: null,
      eventType: 'alert.triggered',
      format: 'markdown',
      body: '**{{rule.name}}**',
    },
    {
      key: 'alert_resolved_markdown',
      category: 'alert',
      channel: null,
      eventType: 'alert.resolved',
      format: 'markdown',
      body: '**[RESOLVED] {{rule.name}}**\n\n- Resolved: {{incident.resolved_time}}\n- Duration: {{incident.duration}}',
    },
  ],
  labelKeys: [],
  annotationKeys: [],
};

test.describe('Notify templates by channel', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
    await page.route('**/api/v1/notify/template-fields', (route) => route.fulfill({ json: FIELDS }));
  });

  test('lists the templates by channel and shows one channel at a time', async ({ page }) => {
    await page.route('**/api/v1/notify/templates', (route) => route.fulfill({ json: TEMPLATES }));
    await page.goto('/settings/notify/templates');

    // General first, then the channels in the order the connector types list them.
    const names = page.locator('tbody tr td:first-child');
    await expect(names).toHaveText(['Alert markdown', 'IM card', 'Alert SMS']);

    const filter = page.getByRole('group', { name: 'Filter by channel' });
    await expect(filter.getByRole('button')).toHaveText([/All\s*3/, /General\s*1/, /Instant messaging\s*1/, /SMS\s*1/]);

    await filter.getByRole('button', { name: /SMS/ }).click();
    await expect(names).toHaveText(['Alert SMS']);
    await expect(page.locator('tbody tr').first()).toContainText('SMS');
  });

  test('folds the variables, presets and preview attributes until they are opened', async ({ page }) => {
    await page.route('**/api/v1/notify/templates', (route) => route.fulfill({ json: [] }));
    await page.goto('/settings/notify/templates');
    await page.getByRole('button', { name: 'New template' }).click();

    const drawer = page.getByRole('dialog');
    const heads = ['Variables', 'Preset templates', 'Preview event attributes (JSON)'].map((name) =>
      drawer.getByRole('button', { name, exact: true }),
    );
    for (const head of heads) await expect(head).toHaveAttribute('aria-expanded', 'false');
    await expect(drawer.getByRole('tab')).toHaveCount(0);
    await expect(drawer.locator('.code-editor-shell')).toHaveCount(0);

    // The message preview is not folded.
    const preview = drawer.locator('section', {
      has: page.getByRole('heading', { name: 'Message preview', exact: true }),
    });
    await expect(preview).toContainText('Start typing a message body.');

    await heads[0]!.click();
    await expect(heads[0]!).toHaveAttribute('aria-expanded', 'true');
    await expect(drawer.getByRole('tab', { name: 'Rule', exact: true })).toBeVisible();
    await heads[2]!.click();
    await expect(drawer.locator('.code-editor-shell')).toHaveCount(1);

    // A preset written for a resolved alert is previewed on one.
    await heads[1]!.click();
    await drawer.getByRole('button', { name: /Alert resolved Markdown/ }).click();
    await expect(preview).toContainText('Resolved: 2026-07-29 01:00:00 UTC');
    await expect(preview).toContainText('Duration: 1h 5m');
    await heads[0]!.click();
    await expect(drawer.getByRole('tab')).toHaveCount(0);
  });

  test('writes an SMS template as plain text with an Alibaba Cloud template', async ({ page }) => {
    const saved: Array<Record<string, unknown>> = [];
    await page.route('**/api/v1/notify/templates', async (route) => {
      if (route.request().method() === 'POST') {
        const body = route.request().postDataJSON() as Record<string, unknown>;
        saved.push(body);
        return route.fulfill({ json: { ...template('tpl-new', 'x', 'sms'), ...body } });
      }
      return route.fulfill({ json: [] });
    });

    await page.goto('/settings/notify/templates');
    await page.getByRole('button', { name: 'New template' }).click();

    const drawer = page.getByRole('dialog');
    await drawer.getByRole('textbox', { name: 'Name', exact: true }).fill('Alert SMS');
    await drawer.getByRole('combobox', { name: /^Channel/ }).click();
    await page.getByRole('option', { name: 'SMS', exact: true }).click();

    // An SMS is one plain text: no format to choose, and its length is counted.
    await expect(drawer.getByRole('combobox', { name: /^Format/ })).toHaveCount(0);
    await expect(drawer).toContainText('The text message is this body alone');
    await drawer.getByRole('button', { name: 'Preset templates', exact: true }).click();
    await drawer.getByRole('button', { name: /Alert SMS/ }).click();
    await expect(drawer.locator('[data-template-length]')).toContainText('/ 320 characters');

    const approved = drawer.locator('[data-provider-template="aliyun"]');
    await approved.getByLabel('Template code').fill('SMS_123');
    await approved.getByRole('button', { name: 'Add variable' }).click();
    await approved.getByRole('textbox', { name: 'Variable', exact: true }).fill('name');

    // A placeholder picked from the panel goes into the variable last typed in.
    const value = approved.getByRole('textbox', { name: 'Value', exact: true });
    await value.click();
    await drawer.getByRole('button', { name: 'Variables', exact: true }).click();
    await drawer.getByRole('tab', { name: 'Rule', exact: true }).click();
    await drawer.getByRole('button', { name: /\{\{rule\.name\}\}/ }).click();
    await expect(value).toHaveValue('{{rule.name}}');
    await expect(drawer.locator('[data-provider-preview]')).toContainText('Alibaba Cloud receives SMS_123');
    await expect(drawer.locator('[data-provider-preview]')).toContainText('API unavailable');

    // Tencent Cloud fills its variables by position; each provider keeps what was written for it.
    await drawer.getByRole('button', { name: 'Tencent Cloud', exact: true }).click();
    const tencent = drawer.locator('[data-provider-template="tencent"]');
    await expect(tencent).toContainText('name them 1, 2, 3');
    await tencent.getByLabel('Template code').fill('1110');
    await tencent.getByRole('button', { name: 'Add variable' }).click();
    await tencent.getByRole('textbox', { name: 'Variable', exact: true }).fill('1');
    await tencent.getByRole('textbox', { name: 'Value', exact: true }).fill('{{rule.name}}');
    await expect(drawer.locator('[data-provider-preview]')).toContainText('Tencent Cloud receives 1110');

    await drawer.getByRole('button', { name: 'Alibaba Cloud', exact: true }).click();
    await expect(drawer.locator('[data-provider-template="aliyun"]').getByLabel('Template code')).toHaveValue('SMS_123');

    await drawer.getByRole('button', { name: 'Save', exact: true }).click();
    await expect.poll(() => saved.length).toBe(1);
    expect(saved[0]).toMatchObject({
      name: 'Alert SMS',
      channel: 'sms',
      format: 'text',
      body: '[{{severity}}] {{rule.name}}: {{incident.summary}}',
      providerTemplates: {
        aliyun: { code: 'SMS_123', params: { name: '{{rule.name}}' } },
        tencent: { code: '1110', params: { '1': '{{rule.name}}' } },
      },
    });
  });

  test('writes a webhook body as JSON, with conditions', async ({ page }) => {
    const saved: Array<Record<string, unknown>> = [];
    await page.route('**/api/v1/notify/templates', async (route) => {
      if (route.request().method() === 'POST') {
        const body = route.request().postDataJSON() as Record<string, unknown>;
        saved.push(body);
        return route.fulfill({ json: { ...template('tpl-new', 'x', 'webhook'), ...body } });
      }
      return route.fulfill({ json: [] });
    });

    await page.goto('/settings/notify/templates');
    await page.getByRole('button', { name: 'New template' }).click();

    const drawer = page.getByRole('dialog');
    await drawer.getByRole('textbox', { name: 'Name', exact: true }).fill('Incident webhook');
    await drawer.getByRole('combobox', { name: /^Format/ }).click();
    await expect(page.getByRole('option', { name: 'json', exact: true })).toHaveCount(0);
    await page.keyboard.press('Escape');

    // JSON is a webhook's format only.
    await drawer.getByRole('combobox', { name: /^Channel/ }).click();
    await page.getByRole('option', { name: 'Webhook', exact: true }).click();
    await drawer.getByRole('combobox', { name: /^Format/ }).click();
    await page.getByRole('option', { name: 'json', exact: true }).click();

    // A block left open is pointed out; closed, the preview keeps the branch that holds.
    const body = drawer.getByRole('textbox', { name: /^Message body/ });
    const problem = drawer.locator('[data-template-block-problem]');
    await body.fill('{"text": "{{#if severity == "critical"}}PAGE{{else}}FYI');
    await expect(problem).toBeVisible();
    await body.fill('{"text": "{{#if severity == "critical"}}PAGE{{else}}FYI{{/if}} {{rule.name}}"}');
    await expect(problem).toHaveCount(0);

    const preview = drawer.locator('section', {
      has: page.getByRole('heading', { name: 'Message preview', exact: true }),
    });
    await expect(preview).toContainText('PAGE API unavailable');
    await expect(preview).not.toContainText('FYI');

    await drawer.getByRole('button', { name: 'Save', exact: true }).click();
    await expect.poll(() => saved.length).toBe(1);
    expect(saved[0]).toMatchObject({
      name: 'Incident webhook',
      channel: 'webhook',
      format: 'json',
      body: '{"text": "{{#if severity == "critical"}}PAGE{{else}}FYI{{/if}} {{rule.name}}"}',
    });
  });

  test('chooses a template of its own for a channel in a policy', async ({ page }) => {
    const saved: Array<Record<string, unknown>> = [];
    await page.route('**/api/v1/notify/templates', (route) => route.fulfill({ json: TEMPLATES }));
    await page.route('**/api/v1/notify/policies/preview', (route) =>
      route.fulfill({
        json: {
          policyId: 'draft',
          matched: true,
          recipients: [
            {
              userId: 'user-1',
              resolvedBy: 'fixed_users',
              deliveryPlan: [
                {
                  stage: 'user_primary',
                  connectorId: 'conn-sms',
                  connectorName: 'Ops SMS',
                  targetType: 'direct_user',
                  targetValueMasked: '138****8000',
                  templateId: 'tpl-sms',
                },
              ],
            },
          ],
        },
      }),
    );
    await page.route('**/api/v1/notify/policies', async (route) => {
      if (route.request().method() === 'POST') {
        saved.push(route.request().postDataJSON() as Record<string, unknown>);
        return route.fulfill({ json: { id: 'policy-1' } });
      }
      return route.fulfill({ json: [] });
    });

    await page.goto('/settings/notify/policies');
    await page.getByRole('button', { name: 'New policy' }).first().click();

    const drawer = page.getByRole('dialog');
    await drawer.getByRole('textbox', { name: 'Name', exact: true }).fill('Critical alerts');

    // The general slot offers general templates; a channel with templates gets a slot of its own.
    await drawer.getByRole('combobox', { name: /^General template/ }).click();
    await expect(page.getByRole('option')).toHaveText(['Use generated event message', 'Alert markdown · markdown']);
    await page.getByRole('option', { name: 'Alert markdown · markdown' }).click();

    const slots = drawer.locator('[data-channel-templates]');
    await expect(slots).toContainText('Templates by channel');
    await expect(slots.getByRole('combobox')).toHaveCount(2);
    await slots.getByRole('combobox', { name: /^SMS/ }).click();
    await page.getByRole('option', { name: 'Alert SMS' }).click();

    // The preview names the template each step is sent with.
    await expect(drawer.locator('[data-step-template]')).toContainText('Template: Alert SMS');

    await drawer.getByRole('button', { name: 'Save policy' }).click();
    await expect.poll(() => saved.length).toBe(1);
    expect(saved[0]).toMatchObject({
      templateId: 'tpl-general',
      channelTemplates: { sms: 'tpl-sms' },
    });
  });

  test('sends an instant messaging template as a card', async ({ page }) => {
    const saved: Array<Record<string, unknown>> = [];
    await page.route('**/api/v1/notify/templates', async (route) => {
      if (route.request().method() === 'POST') {
        const body = route.request().postDataJSON() as Record<string, unknown>;
        saved.push(body);
        return route.fulfill({ json: { ...template('tpl-card', 'x', 'im'), ...body } });
      }
      return route.fulfill({ json: [] });
    });

    await page.goto('/settings/notify/templates');
    await page.getByRole('button', { name: 'New template' }).click();

    const drawer = page.getByRole('dialog');
    await drawer.getByRole('textbox', { name: 'Name', exact: true }).fill('Alert card');
    await drawer.getByRole('textbox', { name: /^Message body/ }).fill('{{incident.summary}} since {{evaluated_at}}');

    // Only instant messaging templates are sent as cards.
    const fields = drawer.locator('[data-card-fields]');
    await expect(fields).toHaveCount(0);
    await drawer.getByRole('combobox', { name: /^Channel/ }).click();
    await page.getByRole('option', { name: 'Instant messaging', exact: true }).click();
    await expect(fields).toContainText('Lark, DingTalk, WeCom');

    // Turned on, the card starts from what an alert is about, filled with the preview event.
    await fields.getByRole('switch', { name: 'Send as a card' }).click();
    const preview = drawer.locator('[data-card-preview]');
    await expect(preview).toContainText('CRITICAL · API unavailable');
    await expect(preview.locator('dt')).toHaveText(['Severity', 'Status', 'Service']);
    await expect(preview.locator('dd')).toHaveText(['critical', 'open', 'api']);
    await expect(preview.getByRole('link', { name: 'Runbook' })).toHaveAttribute(
      'href',
      'https://runbooks.example.com/api-unavailable',
    );

    // A field the preview event cannot fill is left out of the card.
    await fields.getByRole('button', { name: 'Add field' }).click();
    await fields.getByRole('textbox', { name: 'Field name', exact: true }).last().fill('Host');
    const host = fields.getByRole('textbox', { name: 'Field value', exact: true }).last();
    await host.fill('{{labels.host}}');
    await expect(preview.locator('dt')).toHaveText(['Severity', 'Status', 'Service']);

    // A placeholder picked from the panel goes into the card input last typed in.
    const title = fields.getByRole('textbox', { name: /^Card title/ });
    await title.click();
    await drawer.getByRole('button', { name: 'Variables', exact: true }).click();
    await drawer.getByRole('tab', { name: 'Rule', exact: true }).click();
    await drawer.getByRole('button', { name: /\{\{rule\.name\}\}/ }).click();
    await expect(title).toHaveValue('{{rule.name}}');
    await expect(preview.locator('h4')).toHaveText('API unavailable');

    await fields.getByRole('combobox', { name: /^Title color/ }).click();
    await page.getByRole('option', { name: 'Orange', exact: true }).click();

    await drawer.getByRole('button', { name: 'Save', exact: true }).click();
    await expect.poll(() => saved.length).toBe(1);
    expect(saved[0]).toMatchObject({
      channel: 'im',
      card: {
        title: '{{rule.name}}',
        color: 'orange',
        fields: [
          { label: 'Severity', value: '{{severity}}' },
          { label: 'Status', value: '{{incident.status}}' },
          { label: 'Service', value: '{{labels.service}}' },
          { label: 'Host', value: '{{labels.host}}' },
        ],
        links: [{ label: 'Runbook', url: '{{annotations.runbook_url}}' }],
        note: '',
      },
    });
  });

  test('tests an IM app connector with the card of an instant messaging template', async ({ page }) => {
    const tested: Array<Record<string, unknown>> = [];
    await page.route('**/api/v1/notify/connector-types', (route) =>
      route.fulfill({
        json: [
          {
            connectorType: 'lark_app',
            capabilities: {
              directUser: true,
              group: true,
              richText: true,
              interactive: true,
              acknowledgement: false,
              attachments: false,
            },
          },
        ],
      }),
    );
    await page.route('**/api/v1/notify/connectors', (route) =>
      route.fulfill({
        json: [
          {
            id: 'conn-lark',
            organizationId: 'org-1',
            name: 'Ops Lark',
            connectorType: 'lark_app',
            config: { appId: 'cli_a', appSecret: '***', apiBaseUrl: 'https://open.feishu.cn/open-apis' },
            capabilities: { directUser: true, group: true },
            enabled: true,
            status: 'unknown',
            createdAt: 1,
            updatedAt: 1,
          },
        ],
      }),
    );
    await page.route('**/api/v1/notify/templates', (route) =>
      route.fulfill({
        json: [
          template('tpl-card', 'Alert card', 'im', {
            card: { title: '', color: 'severity', fields: [], links: [], note: '' },
          }),
          template('tpl-sms', 'Alert SMS', 'sms'),
        ],
      }),
    );
    await page.route('**/api/v1/notify/connectors/conn-lark/test', (route) => {
      tested.push(route.request().postDataJSON() as Record<string, unknown>);
      return route.fulfill({ json: { sent: true, testedAt: 1, elapsedMs: 5, error: null } });
    });

    await page.goto('/settings/notify/connectors');
    await page.getByText('Ops Lark').click();

    // Without a template the test is a plain message; an instant messaging template sends its card.
    const drawer = page.getByRole('dialog');
    const choice = drawer.getByRole('combobox', { name: /^Test template/ });
    await expect(choice).toContainText('No template');
    await choice.click();
    await expect(page.getByRole('option')).toHaveText(['No template', 'Alert card']);
    await page.getByRole('option', { name: 'Alert card' }).click();

    await drawer.getByRole('textbox', { name: /^Target/ }).fill('ou_123');
    await drawer.getByRole('button', { name: 'Test connection' }).click();
    await expect.poll(() => tested.length).toBe(1);
    expect(tested[0]).toMatchObject({ target: 'ou_123', templateId: 'tpl-card' });
  });
});

