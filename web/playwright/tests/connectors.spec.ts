import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

interface Stored {
  id: string;
  name: string;
  kind: string;
  config: Record<string, unknown>;
  enabled: boolean;
}

const SECRETS = [
  'accessKey',
  'secretKey',
  'sessionToken',
  'pushToken',
  'saslPassword',
  'tlsClientKey',
  'url',
  'bearerToken',
  'basicPassword',
  'headerValue',
  'hmacSecret',
];

type Check = { ok: boolean; code: string; detail?: string; elapsedMs: number };

/** What the server works out from a configuration: where an HTTP destination points. */
function withDerived(kind: string, config: Record<string, unknown>): Record<string, unknown> {
  if (kind !== 'http' || typeof config.url !== 'string') return config;

  return { ...config, urlOrigin: new URL(config.url).origin };
}

/**
 * The connectors API as the backend answers it: secrets masked on the way
 * out and listed by key, and kept when the mask comes back on an update. A
 * connection test answers with `checks`, in turn, the last one from then on.
 */
async function mountConnectors(page: Page, seed: Stored[] = [], checks: Check[] = []) {
  const stored = [...seed];
  const requests: Array<{ method: string; body: Record<string, unknown> }> = [];
  const tests: Array<Record<string, unknown>> = [];
  const view = (item: Stored) => ({
    id: item.id,
    name: item.name,
    kind: item.kind,
    configJson: Object.fromEntries(
      Object.entries(item.config).map(([key, value]) => [key, SECRETS.includes(key) ? '***' : value]),
    ),
    secretKeys: SECRETS.filter((key) => item.config[key]),
    enabled: item.enabled,
    createdAt: 1,
    updatedAt: 1,
  });

  await page.route('**/api/v1/connectors', (route) => {
    if (route.request().method() === 'GET') return route.fulfill({ json: stored.map(view) });

    const body = route.request().postDataJSON() as { name: string; kind: string; configJson: Record<string, unknown>; enabled?: boolean };
    requests.push({ method: 'POST', body });
    const created = {
      id: `c-${stored.length + 1}`,
      name: body.name,
      kind: body.kind,
      config: withDerived(body.kind, body.configJson),
      enabled: body.enabled ?? true,
    };
    stored.push(created);
    return route.fulfill({ json: view(created) });
  });
  await page.route('**/api/v1/connectors/*', (route) => {
    const id = new URL(route.request().url()).pathname.split('/').pop();
    const index = stored.findIndex((item) => item.id === id);
    const current = stored[index];
    if (!current) return route.fulfill({ status: 404, json: { error: 'not_found', message: 'not found' } });

    if (route.request().method() === 'DELETE') {
      stored.splice(index, 1);
      return route.fulfill({ json: { deleted: true } });
    }
    const body = route.request().postDataJSON() as { name: string; kind: string; configJson: Record<string, unknown>; enabled: boolean };
    requests.push({ method: 'PUT', body });
    const config = Object.fromEntries(
      Object.entries(body.configJson).map(([key, value]) => [key, value === '***' ? current.config[key] : value]),
    );
    stored[index] = { ...current, name: body.name, config: withDerived(current.kind, config), enabled: body.enabled };
    return route.fulfill({ json: view(stored[index]!) });
  });
  // Registered after the route for one connector, so they answer first.
  await page.route('**/api/v1/connectors/test', (route) => {
    tests.push(route.request().postDataJSON() as Record<string, unknown>);
    const check = checks[Math.min(tests.length, checks.length) - 1] ?? { ok: true, code: 'ok', elapsedMs: 1 };
    return route.fulfill({ json: check });
  });
  await page.route('**/api/v1/connectors/aws-external-id', (route) =>
    route.fulfill({ json: { externalId: 'molesignal-org-acme' } }),
  );
  await page.route('**/api/v1/streams', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({
          json: ['default', 'edge'].map((name) => ({
            id: `logs/${name}`,
            name,
            streamType: 'logs',
            schema: { fields: [] },
            createdAt: 1,
            updatedAt: 1,
          })),
        })
      : route.fallback(),
  );

  return { stored, requests, tests };
}

const ARCHIVE: Stored = {
  id: 'c-archive',
  name: 'archive',
  kind: 's3',
  config: { bucket: 'acme-logs', prefix: 'out/', accessKey: 'AKIAEXAMPLE', secretKey: 'kept-secret' },
  enabled: true,
};
const EDGE: Stored = {
  id: 'c-edge',
  name: 'edge-requests',
  kind: 'cloudflare_logpush',
  config: { targetStream: 'edge', pushToken: 'a-token-that-was-saved-earlier' },
  enabled: true,
};

const section = (page: Page, direction: 'source' | 'destination') =>
  page.locator(`[data-connector-section="${direction}"]`);
const drawer = (page: Page) => page.getByRole('dialog');

/** Picks a way of authenticating from the list of them. */
async function chooseAuth(page: Page, name: string) {
  await drawer(page).getByRole('combobox', { name: 'Authentication' }).click();
  await page.getByRole('option', { name, exact: true }).click();
}

test.describe('Connectors', () => {
  test.beforeEach(async ({ page, mockServer }) => {
    await mountMockRoutes(page, mockServer.port);
  });

  test('is the pipelines module’s page of sources and destinations, each told apart', async ({ page }) => {
    await mountConnectors(page, [ARCHIVE, EDGE]);
    await page.goto('/pipelines/connectors');

    const header = page.getByTestId('page-header');
    await expect(header.getByRole('heading', { level: 1 })).toHaveText('Connectors');
    // One step below the pipelines list, which stays lit in the sidebar.
    await expect(header.getByRole('navigation').getByRole('link', { name: 'Pipelines' })).toHaveAttribute(
      'href',
      '/pipelines',
    );
    await expect(header.getByRole('button')).toHaveText(['New connector']);

    await expect(section(page, 'source').getByRole('heading', { level: 2 })).toHaveText('Sources');
    await expect(section(page, 'source').getByRole('columnheader')).toHaveText([
      'Name',
      'Kind',
      'Into stream',
      'Last received',
      'Status',
      'Actions',
    ]);
    await expect(section(page, 'source').getByRole('row').nth(1)).toContainText('edge-requestsCloudflare LogpushPushededge');
    await expect(section(page, 'destination').getByRole('columnheader')).toHaveText([
      'Name',
      'Kind',
      'Where',
      'Authentication',
      'Status',
      'Actions',
    ]);
    await expect(section(page, 'destination').getByRole('row').nth(1)).toContainText(
      'archiveAmazon S3s3://acme-logs/out/Access key',
    );
    // No secret is on the page, masked or not.
    await expect(page.locator('#main')).not.toContainText(/kept-secret|AKIAEXAMPLE|\*\*\*/);

    // The two tables share their first columns, so they read as one list.
    const lefts = (direction: 'source' | 'destination') =>
      section(page, direction)
        .getByRole('columnheader')
        .evaluateAll((cells) => cells.slice(0, 3).map((cell) => Math.round(cell.getBoundingClientRect().left)));
    expect(await lefts('source')).toEqual(await lefts('destination'));
  });

  test('creates an S3 destination with whichever credentials the deployment has', async ({ page }) => {
    const backend = await mountConnectors(page);
    await page.goto('/pipelines/connectors');
    await expect(page.getByText('No destinations yet.')).toBeVisible();

    await page.getByRole('button', { name: 'Add a destination' }).click();
    await drawer(page).getByRole('button', { name: /^Amazon S3/ }).click();
    await drawer(page).getByRole('textbox', { name: 'Name', exact: true }).fill('oss-backup');
    await drawer(page).getByRole('textbox', { name: 'Bucket', exact: true }).fill('acme-backup');
    await drawer(page).getByRole('combobox', { name: 'Authentication' }).click();
    await expect(page.getByRole('option')).toHaveText([
      'Access key',
      'Temporary key',
      'Node identity',
      'Assume a role',
      'Web identity',
    ]);

    // A temporary key is the key, its secret, and the token it was issued with.
    await page.getByRole('option', { name: 'Temporary key' }).click();
    await drawer(page).getByLabel(/^Access key ID/).fill('ASIAEXAMPLE');
    await drawer(page).getByLabel(/^Secret access key/).fill('temporary-secret');
    await expect(drawer(page).getByRole('button', { name: 'Create connector' })).toBeDisabled();
    await drawer(page).getByLabel(/^Session token/).fill('issued-with-the-key');

    await drawer(page).getByRole('checkbox', { name: 'S3-compatible storage' }).click();
    await drawer(page).getByRole('textbox', { name: /^Endpoint/ }).fill('https://oss-cn-hangzhou.aliyuncs.com');
    await drawer(page).getByRole('checkbox', { name: 'Address the bucket as a host name' }).click();
    await drawer(page).getByRole('button', { name: 'Create connector' }).click();

    await expect(drawer(page)).toHaveCount(0);
    expect(backend.requests[0]?.body).toEqual({
      name: 'oss-backup',
      kind: 's3',
      configJson: {
        bucket: 'acme-backup',
        endpoint: 'https://oss-cn-hangzhou.aliyuncs.com',
        virtualHostedStyle: true,
        auth: 'temporary_key',
        accessKey: 'ASIAEXAMPLE',
        secretKey: 'temporary-secret',
        sessionToken: 'issued-with-the-key',
      },
      enabled: true,
    });
    await expect(section(page, 'destination').getByRole('row').nth(1)).toContainText(
      'oss-backupAmazon S3s3://acme-backup · oss-cn-hangzhou.aliyuncs.comTemporary key',
    );
  });

  test('reads a Kafka topic from where it says, over TLS, into a stream', async ({ page }) => {
    const backend = await mountConnectors(page);
    await page.goto('/pipelines/connectors');

    await page.getByRole('button', { name: 'Add a source' }).click();
    await drawer(page).getByRole('button', { name: /^Kafka topic/ }).click();
    await drawer(page).getByRole('textbox', { name: 'Name', exact: true }).fill('orders');
    await drawer(page).getByRole('textbox', { name: /^Brokers/ }).fill('b-1:9093,b-2:9093');
    await drawer(page).getByRole('textbox', { name: /^Topic/ }).fill('orders');
    await drawer(page).getByRole('combobox', { name: 'Start from' }).click();
    await page.getByRole('option', { name: 'Everything that is there' }).click();
    await drawer(page).getByRole('checkbox', { name: 'Encrypt the connection with TLS' }).click();
    await drawer(page).getByRole('button', { name: 'Create connector' }).click();

    await expect(drawer(page)).toHaveCount(0);
    expect(backend.requests[0]?.body).toEqual({
      name: 'orders',
      kind: 'kafka_source',
      configJson: {
        brokers: 'b-1:9093,b-2:9093',
        topic: 'orders',
        startFrom: 'earliest',
        tls: true,
        auth: 'none',
      },
      enabled: true,
    });
    await expect(section(page, 'source').getByRole('row').nth(1)).toContainText('Kafka topic');
  });

  test('reads the new objects of a bucket, and tests it without writing there', async ({ page }) => {
    const backend = await mountConnectors(page, [], [{ ok: true, code: 'ok', elapsedMs: 12 }]);
    await page.goto('/pipelines/connectors');

    await page.getByRole('button', { name: 'Add a source' }).click();
    await drawer(page).getByRole('button', { name: /^S3 objects/ }).click();
    await drawer(page).getByRole('textbox', { name: 'Name', exact: true }).fill('elb-logs');
    await drawer(page).getByRole('textbox', { name: 'Bucket', exact: true }).fill('acme-logs');
    await drawer(page).getByRole('textbox', { name: /^Key prefix/ }).fill('AWSLogs/');
    await drawer(page).getByRole('combobox', { name: 'Authentication' }).click();
    await page.getByRole('option', { name: 'Node identity' }).click();

    await expect(drawer(page)).toContainText('Lists one object under the prefix');
    await drawer(page).getByRole('button', { name: 'Test connection' }).click();
    await expect(drawer(page)).toContainText('Connected in 12 ms');
    expect(backend.tests[0]).toMatchObject({ kind: 's3_source' });

    await drawer(page).getByRole('button', { name: 'Create connector' }).click();
    await expect(drawer(page)).toHaveCount(0);
    expect(backend.requests[0]?.body).toEqual({
      name: 'elb-logs',
      kind: 's3_source',
      configJson: { bucket: 'acme-logs', prefix: 'AWSLogs/', auth: 'node_role' },
      enabled: true,
    });
  });

  test('assumes a role with the organization’s external ID, shown where the trust policy needs it', async ({ page }) => {
    const backend = await mountConnectors(page);
    await page.goto('/pipelines/connectors');

    await page.getByRole('button', { name: 'Add a destination' }).click();
    await drawer(page).getByRole('button', { name: /^Amazon S3/ }).click();
    await drawer(page).getByRole('textbox', { name: 'Name', exact: true }).fill('partner');
    await drawer(page).getByRole('textbox', { name: 'Bucket', exact: true }).fill('acme-partner');
    await chooseAuth(page, 'Assume a role');

    const externalId = drawer(page).locator('[data-connector-external-id]');
    await expect(externalId).toContainText('molesignal-org-acme');
    await expect(externalId).toContainText('sts:ExternalId');
    await drawer(page).getByRole('textbox', { name: /^Role ARN/ }).fill('arn:aws:iam::123456789012:role/molesignal');
    await drawer(page).getByRole('button', { name: 'Create connector' }).click();

    await expect(drawer(page)).toHaveCount(0);
    expect(backend.requests[0]?.body.configJson).toEqual({
      bucket: 'acme-partner',
      auth: 'assume_role',
      roleArn: 'arn:aws:iam::123456789012:role/molesignal',
    });
    await expect(section(page, 'destination').getByRole('row').nth(1)).toContainText('Assume a role');
  });

  test('tests the settings before they are saved, and says what went wrong', async ({ page }) => {
    const backend = await mountConnectors(page, [], [
      { ok: false, code: 'denied', detail: 's3 put: <Code>AccessDenied</Code> Access Denied', elapsedMs: 180 },
      { ok: true, code: 'ok', elapsedMs: 95 },
    ]);
    await page.goto('/pipelines/connectors');

    await page.getByRole('button', { name: 'Add a destination' }).click();
    await drawer(page).getByRole('button', { name: /^Amazon S3/ }).click();
    await chooseAuth(page, 'Node identity');
    const test = drawer(page).getByRole('button', { name: 'Test connection' });
    const outcome = drawer(page).locator('[data-connector-test]');

    // Untested settings that cannot work yet: no bucket.
    await expect(test).toBeDisabled();
    await drawer(page).getByRole('textbox', { name: 'Bucket', exact: true }).fill('acme-logs');
    await test.click();
    await expect(outcome.getByRole('status')).toHaveText('Credentials or permissions refused');
    await expect(outcome).toContainText('Access Denied');
    expect(backend.tests[0]).toEqual({ kind: 's3', configJson: { bucket: 'acme-logs', auth: 'node_role' } });

    // Changed, the settings are untested again.
    await drawer(page).getByRole('textbox', { name: /^Key prefix/ }).fill('out/');
    await expect(outcome.getByRole('status')).toHaveText('');
    await test.click();
    await expect(outcome.getByRole('status')).toHaveText('Connected in 95 ms');
    expect(backend.requests).toHaveLength(0);
  });

  test('keeps an HTTP destination’s address as a secret, and says where it points', async ({ page }) => {
    const backend = await mountConnectors(page);
    await page.goto('/pipelines/connectors');

    await page.getByRole('button', { name: 'Add a destination' }).click();
    await drawer(page).getByRole('button', { name: /^HTTP \/ Webhook/ }).click();
    await drawer(page).getByRole('textbox', { name: 'Name', exact: true }).fill('collector');
    await drawer(page).getByLabel(/^URL/).fill('https://in.example.com/receiver/v1/http/c0llect0r-c0de');
    await chooseAuth(page, 'Custom header');
    await drawer(page).getByRole('textbox', { name: /^Header name/ }).fill('Content-Type');
    await expect(drawer(page).getByRole('button', { name: 'Create connector' })).toBeDisabled();
    await drawer(page).getByRole('textbox', { name: /^Header name/ }).fill('X-Api-Key');
    await drawer(page).getByLabel(/^Header value/).fill('a-key');
    await drawer(page).getByRole('button', { name: 'Create connector' }).click();

    await expect(drawer(page)).toHaveCount(0);
    expect(backend.requests[0]?.body.configJson).toEqual({
      url: 'https://in.example.com/receiver/v1/http/c0llect0r-c0de',
      auth: 'header',
      headerName: 'X-Api-Key',
      headerValue: 'a-key',
    });
    const row = section(page, 'destination').getByRole('row').nth(1);
    await expect(row).toContainText('collectorHTTP / Webhookhttps://in.example.comCustom header');
    await expect(page.locator('#main')).not.toContainText('c0llect0r-c0de');

    // Opened again, the address is kept unless another is typed.
    await section(page, 'destination').getByRole('button', { name: 'Edit' }).click();
    await expect(drawer(page).getByLabel(/^URL/)).toHaveValue('');
    await drawer(page).getByRole('button', { name: 'Save changes' }).click();
    await expect(drawer(page)).toHaveCount(0);
    expect(backend.requests[1]?.body.configJson).toMatchObject({ url: '***', headerValue: '***' });
    expect(backend.stored[0]?.config).toMatchObject({ url: 'https://in.example.com/receiver/v1/http/c0llect0r-c0de' });
  });

  test('hands a push connector’s token over once, with where to point the service', async ({ page }) => {
    const backend = await mountConnectors(page);
    await page.goto('/pipelines/connectors');

    await page.getByRole('button', { name: 'Add a source' }).click();
    await drawer(page).getByRole('button', { name: /^Cloudflare Logpush/ }).click();
    await drawer(page).getByRole('textbox', { name: 'Name', exact: true }).fill('edge');
    // Nobody types the token, and it is nowhere to be seen before the connector exists.
    await expect(drawer(page).getByLabel(/^Token/)).toHaveCount(0);
    await drawer(page).getByRole('button', { name: 'Create connector' }).click();

    await expect(drawer(page).getByRole('heading', { name: 'Point Cloudflare Logpush here' })).toBeVisible();
    const token = String((backend.requests[0]?.body.configJson as Record<string, unknown>).pushToken);
    expect(token).toMatch(/^[A-Za-z0-9_-]{43}$/);
    const setup = drawer(page).locator('[data-connector-push-setup]');
    await expect(setup.locator('code')).toHaveText([
      /\/api\/v1\/_cloudflare$/,
      token,
      new RegExp(`/api/v1/_cloudflare\\?header_X-Connector-Token=${token}$`),
    ]);
    await expect(setup).toContainText('The token is shown this once.');

    await drawer(page).getByRole('button', { name: 'Done' }).click();
    await expect(section(page, 'source').getByRole('row').nth(1)).toContainText('edgeCloudflare LogpushPushedcloudflare');

    // Opened again, the token is not there to read: only to replace.
    await section(page, 'source').getByRole('button', { name: 'Edit' }).click();
    await expect(drawer(page).locator('[data-connector-token-state]')).toHaveText(
      'A token is saved. It cannot be shown again.',
    );
    await expect(drawer(page)).not.toContainText(token);
  });

  test('changes a saved connector and switches it off without its secrets being typed again', async ({
    page,
  }) => {
    const backend = await mountConnectors(page, [ARCHIVE]);
    await page.goto('/pipelines/connectors');

    await section(page, 'destination').getByRole('button', { name: 'Edit' }).click();
    await expect(drawer(page).getByLabel(/^Secret access key/)).toHaveValue('');
    await drawer(page).getByRole('textbox', { name: /^Key prefix/ }).fill('archive/');
    await drawer(page).getByRole('button', { name: 'Save changes' }).click();
    await expect(drawer(page)).toHaveCount(0);

    // What went over the wire was the mask; what is stored is still the secret.
    expect(backend.requests[0]?.body.configJson).toMatchObject({ prefix: 'archive/', secretKey: '***' });
    expect(backend.stored[0]?.config).toMatchObject({ prefix: 'archive/', secretKey: 'kept-secret' });

    await section(page, 'destination').getByRole('switch', { name: 'Turn archive on or off' }).click();
    await expect(section(page, 'destination').getByRole('switch')).toHaveAttribute('aria-checked', 'false');
    expect(backend.stored[0]).toMatchObject({ enabled: false, config: { secretKey: 'kept-secret' } });
  });

  test('deletes a connector after saying what that does', async ({ page }) => {
    const backend = await mountConnectors(page, [ARCHIVE]);
    await page.goto('/pipelines/connectors');

    await section(page, 'destination').getByRole('button', { name: 'Delete' }).click();
    const confirm = page.getByRole('dialog');
    await expect(confirm).toContainText('Delete archive?');
    await expect(confirm).toContainText('records an error on every run');
    await confirm.getByRole('button', { name: 'Delete connector' }).click();

    await expect(page.getByText('No destinations yet.')).toBeVisible();
    expect(backend.stored).toHaveLength(0);
  });

  test('reads in Chinese when the interface does', async ({ page }) => {
    await mountConnectors(page, [ARCHIVE, EDGE]);
    await page.route('**/api/v1/me/preferences', (route) =>
      route.request().method() === 'GET'
        ? route.fulfill({
            json: {
              theme: 'system',
              density: 'normal',
              language: 'zh-cn',
              defaultHomeRoute: '/home',
              timeFormat: 'iso_24h',
              dateFormat: 'yyyy_mm_dd_dash',
              timezone: '',
              keyboardShortcutsEnabled: true,
            },
          })
        : route.fallback(),
    );
    await page.goto('/pipelines/connectors');

    await expect(page.getByTestId('page-header').getByRole('heading', { level: 1 })).toHaveText('连接器');
    await expect(section(page, 'destination').getByRole('row').nth(1)).toContainText('访问密钥');
    await page.getByRole('button', { name: '新建连接器' }).click();
    await drawer(page).getByRole('button', { name: /^Apache Kafka/ }).click();
    await expect(drawer(page).locator('[data-connector-auth-hint]')).toHaveText('用于不要求凭据的 Broker。');
    await expect(drawer(page).getByRole('checkbox', { name: 'TLS 加密连接' })).toBeVisible();
    await expect(drawer(page).getByRole('button', { name: '测试连接' })).toBeVisible();

    const text = `${await page.locator('#main').innerText()}\n${await drawer(page).innerText()}`;
    expect(text).not.toMatch(/\bconnectors\.[a-z_.]+\b/);
    expect(text).not.toMatch(/New connector|Authentication|Deliver to|Destinations|Sources/);
  });
});
