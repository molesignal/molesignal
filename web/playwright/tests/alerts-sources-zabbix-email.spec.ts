/**
 * The senders added later: Zabbix, set up by importing a media type; email, read from an IMAP mailbox
 * or posted as a raw message, whose mailbox is read only and whose password is never shown again; and
 * two cloud monitors that carry the token in the address, Tencent Cloud and CloudWatch through SNS.
 */
import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

const NOW = Date.now() * 1000;
const MINUTE = 60_000_000;

const base = {
  enabled: true,
  escalationPolicyId: 'default',
  severity: 'warning',
  labels: {},
  tokenPrefix: 'mspat-c16jb8m5zr',
  homeCluster: '',
  intakePath: '/api/v1/intake/alerts',
  createdBy: 'u1',
  createdAt: NOW - 60 * MINUTE,
  updatedAt: NOW - 60 * MINUTE,
  lastIntakeAt: null,
};

const TOKEN = 'mspat-c16jb8m5zrzf174p8g8b7m5p4jz0kp9y1hxr';

const email = {
  ...base,
  id: 'src-mail',
  name: 'Monitoring mailbox',
  kind: 'email',
  email: { allowedSenders: ['@monitoring.example.com'], resolvePattern: '', ignorePattern: '', dedupPattern: '' },
};

const zabbix = { ...base, id: 'src-zbx', name: 'Zabbix', kind: 'zabbix', email: { allowedSenders: [] } };

test('a Zabbix source gives a media type to import, with the token written in', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  let created: Record<string, unknown> | null = null;
  await page.route(/\/api\/v1\/alerts\/sources$/, (route) => {
    if (route.request().method() === 'POST') {
      created = route.request().postDataJSON() as Record<string, unknown>;
      return route.fulfill({ json: { ...zabbix, token: TOKEN } });
    }
    return route.fulfill({ json: [] });
  });

  await page.route(/\/api\/v1\/alerts\/sources\/src-zbx$/, (route) => route.fulfill({ json: zabbix }));
  await page.route(/\/api\/v1\/alerts\/sources\/src-zbx\/requests(\?.*)?$/, (route) => route.fulfill({ json: [] }));

  await page.goto('/alerts/sources');
  await page.getByRole('button', { name: /New/ }).first().click();
  await page.getByRole('radio', { name: /Zabbix/ }).click();
  await page.getByRole('textbox', { name: /^Name/ }).fill('Zabbix');
  await page.locator('button[type="submit"][form="alert-source-form"]').click();

  await expect.poll(() => created?.kind).toBe('zabbix');

  const dialog = page.locator('[data-source-token]');
  await expect(dialog).toContainText('zabbix_export');
  await expect(dialog).toContainText('{EVENT.NSEVERITY}');
  await expect(dialog).toContainText('{EVENT.RECOVERY.TIMESTAMP}');
  await expect(dialog).toContainText(TOKEN);
});

test('a CloudWatch source subscribes an SNS topic, with the token in the address', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  const cloudwatch = { ...base, id: 'src-cw', name: 'CloudWatch', kind: 'aws_cloudwatch', email: { allowedSenders: [] } };

  let created: Record<string, unknown> | null = null;
  await page.route(/\/api\/v1\/alerts\/sources$/, (route) => {
    if (route.request().method() === 'POST') {
      created = route.request().postDataJSON() as Record<string, unknown>;
      return route.fulfill({ json: { ...cloudwatch, token: TOKEN } });
    }
    return route.fulfill({ json: [] });
  });
  await page.route(/\/api\/v1\/alerts\/sources\/src-cw$/, (route) => route.fulfill({ json: cloudwatch }));
  await page.route(/\/api\/v1\/alerts\/sources\/src-cw\/requests(\?.*)?$/, (route) => route.fulfill({ json: [] }));

  await page.goto('/alerts/sources');
  await page.getByRole('button', { name: /New/ }).first().click();
  await page.getByRole('radio', { name: /Amazon CloudWatch/ }).click();
  await page.getByRole('textbox', { name: /^Name/ }).fill('CloudWatch');
  await page.locator('button[type="submit"][form="alert-source-form"]').click();

  await expect.poll(() => created?.kind).toBe('aws_cloudwatch');

  const dialog = page.locator('[data-source-token]');
  await expect(dialog).toContainText('aws sns subscribe');
  await expect(dialog).toContainText(`/api/v1/intake/alerts?token=${TOKEN}`);

  await page.goto('/alerts/sources/src-cw');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('CloudWatch');
  await expect(page.locator('[data-source-snippet]')).toContainText('--notification-endpoint');
  await expect(page.locator('a[href*="/alerting/sources#amazon-cloudwatch"]')).toBeVisible();
});

test('a Tencent Cloud source is set up by its address alone', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  const tencent = { ...base, id: 'src-tc', name: 'Tencent production', kind: 'tencent_monitor', email: { allowedSenders: [] } };

  await page.route(/\/api\/v1\/alerts\/sources\/src-tc$/, (route) => route.fulfill({ json: tencent }));
  await page.route(/\/api\/v1\/alerts\/sources\/src-tc\/requests(\?.*)?$/, (route) => route.fulfill({ json: [] }));

  await page.goto('/alerts/sources/src-tc');
  await expect(page.getByRole('heading', { level: 1 })).toHaveText('Tencent production');
  await expect(page.locator('code', { hasText: '/api/v1/intake/alerts?token=<TOKEN>' })).toBeVisible();
  await expect(page.locator('[data-source-snippet]')).toHaveCount(0);
  await expect(page.getByText(/API callback of a notification template/)).toBeVisible();
  await expect(page.locator('a[href*="/alerting/sources#tencent-cloud-observability-platform"]')).toBeVisible();
});

test('an email source names its senders and reads a mailbox', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  let savedSource: Record<string, unknown> | null = null;
  let savedMailbox: Record<string, unknown> | null = null;
  let tested: Record<string, unknown> | null = null;
  let mailbox: Record<string, unknown> | null = null;

  await page.route(/\/api\/v1\/alerts\/sources\/src-mail$/, (route) => {
    if (route.request().method() === 'PUT') {
      savedSource = route.request().postDataJSON() as Record<string, unknown>;
    }
    return route.fulfill({ json: email });
  });
  await page.route(/\/api\/v1\/alerts\/sources\/src-mail\/requests(\?.*)?$/, (route) => route.fulfill({ json: [] }));
  await page.route(/\/api\/v1\/alerts\/sources\/src-mail\/mailbox$/, (route) => {
    if (route.request().method() === 'PUT') {
      savedMailbox = route.request().postDataJSON() as Record<string, unknown>;

      const { secret: _secret, ...settings } = savedMailbox;

      mailbox = {
        settings,
        secretSet: true,
        reading: false,
        lastPolledAt: null,
        lastSuccessAt: null,
        lastError: 'the mail server answered: Invalid credentials',
        updatedAt: NOW,
      };
    }

    return route.fulfill({ json: mailbox });
  });
  await page.route(/\/api\/v1\/alerts\/sources\/src-mail\/mailbox\/test$/, (route) => {
    tested = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({ json: { ok: true, messages: 3 } });
  });

  await page.goto('/alerts/sources/src-mail');

  const form = page.locator('[data-mailbox-form]');
  await form.getByRole('textbox', { name: /IMAP server/ }).fill('imap.example.com');
  await form.getByRole('textbox', { name: /User name/ }).fill('alerts@example.com');
  const password = form.getByRole('textbox', { name: /^Password/ });
  await password.fill('app-password');

  // STARTTLS moves a port still at TLS's default to its own.
  await form.locator('button[role="combobox"]').first().click();
  await page.getByRole('option', { name: 'STARTTLS' }).click();
  await expect(form.getByRole('spinbutton', { name: /Port/ })).toHaveValue('143');

  // Without encryption the password crosses the network in the clear: the form says so.
  await form.locator('button[role="combobox"]').first().click();
  await page.getByRole('option', { name: 'None' }).click();
  await expect(form.locator('[data-mailbox-plaintext]')).toBeVisible();
  await form.locator('button[role="combobox"]').first().click();
  await page.getByRole('option', { name: 'TLS', exact: true }).click();
  await expect(form.getByRole('spinbutton', { name: /Port/ })).toHaveValue('993');

  await form.locator('[data-mailbox-test]').click();
  await expect.poll(() => tested?.host).toBe('imap.example.com');
  await expect(page.getByText('Connected; this folder has 3 mails')).toBeVisible();

  await form.locator('[data-mailbox-save]').click();
  await expect.poll(() => savedMailbox).toMatchObject({
    host: 'imap.example.com',
    port: 993,
    security: 'tls',
    username: 'alerts@example.com',
    secret: 'app-password',
    folder: 'INBOX',
    pollSecs: 60,
    auth: { kind: 'password' },
  });

  // The saved password is not shown again, and the last failed read says why.
  await expect(password).toHaveValue('');
  await expect(password).toHaveAttribute('placeholder', 'Leave empty to keep it');
  await expect(form.locator('[data-mailbox-standing="failed"]')).toContainText('Invalid credentials');

  // How a mail is read is on the source.
  await page.getByRole('button', { name: 'Edit' }).click();

  const drawer = page.locator('#alert-source-form');
  await drawer.getByRole('textbox', { name: /Accept mail from/ }).fill('nagios@example.com, @monitoring.example.com');
  await drawer.getByRole('textbox', { name: /Subjects that recover/ }).fill('\\bUP\\b');
  await page.locator('button[type="submit"][form="alert-source-form"]').click();

  await expect.poll(() => savedSource?.email).toMatchObject({
    allowedSenders: ['nagios@example.com', '@monitoring.example.com'],
    resolvePattern: '\\bUP\\b',
  });
});

test('a mailbox for Microsoft 365 signs in with a token', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  let saved: Record<string, unknown> | null = null;

  await page.route(/\/api\/v1\/alerts\/sources\/src-mail$/, (route) => route.fulfill({ json: email }));
  await page.route(/\/api\/v1\/alerts\/sources\/src-mail\/requests(\?.*)?$/, (route) => route.fulfill({ json: [] }));
  await page.route(/\/api\/v1\/alerts\/sources\/src-mail\/mailbox$/, (route) => {
    if (route.request().method() === 'PUT') saved = route.request().postDataJSON() as Record<string, unknown>;
    return route.fulfill({ json: null });
  });

  await page.goto('/alerts/sources/src-mail');

  const form = page.locator('[data-mailbox-form]');
  await form.getByRole('textbox', { name: /IMAP server/ }).fill('outlook.office365.com');
  await form.getByRole('textbox', { name: /User name/ }).fill('alerts@example.com');
  await form.getByRole('radio', { name: /OAuth 2.0/ }).click();
  await form.getByRole('textbox', { name: /^Client secret/ }).fill('shh');
  await form.getByRole('textbox', { name: /Token address/ }).fill('https://login.microsoftonline.com/t/oauth2/v2.0/token');
  await form.getByRole('textbox', { name: /Client ID/ }).fill('client');
  await form.getByRole('textbox', { name: /Scope/ }).fill('https://outlook.office365.com/.default');
  await form.locator('[data-mailbox-save]').click();

  await expect.poll(() => saved?.auth).toEqual({
    kind: 'oauth2',
    tokenUrl: 'https://login.microsoftonline.com/t/oauth2/v2.0/token',
    clientId: 'client',
    scope: 'https://outlook.office365.com/.default',
  });
});

test('another cluster reads the mailbox of a source it receives', async ({ page, mockServer }) => {
  await mountMockRoutes(page, mockServer.port);

  await page.route(/\/api\/v1\/alerts\/sources\/clusters$/, (route) =>
    route.fulfill({ json: { local: 'east', clusters: [{ id: 'east', name: 'east', local: true }, { id: 'west', name: 'west', local: false }] } }),
  );
  await page.route(/\/api\/v1\/alerts\/sources\/src-mail$/, (route) =>
    route.fulfill({ json: { ...email, homeCluster: 'west' } }),
  );
  await page.route(/\/api\/v1\/alerts\/sources\/src-mail\/requests(\?.*)?$/, (route) => route.fulfill({ json: [] }));
  await page.route(/\/api\/v1\/alerts\/sources\/src-mail\/mailbox$/, (route) => route.fulfill({ json: null }));

  await page.goto('/alerts/sources/src-mail');

  await expect(page.locator('[data-mailbox-elsewhere]')).toContainText('west');
  await expect(page.locator('[data-mailbox-form]')).toHaveCount(0);
});
