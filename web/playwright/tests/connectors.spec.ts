import type { Page } from '@playwright/test';

import { expect, mountMockRoutes, test } from '../fixtures/mockBackend';

interface Stored {
  id: string;
  name: string;
  kind: string;
  config: Record<string, unknown>;
  enabled: boolean;
}

const SECRETS = ['access_key', 'secret_key', 'session_token', 'push_token', 'sasl_password'];

/**
 * The connectors API as the backend answers it: secrets masked on the way
 * out and listed by key, and kept when the mask comes back on an update.
 */
async function mountConnectors(page: Page, seed: Stored[] = []) {
  const stored = [...seed];
  const requests: Array<{ method: string; body: Record<string, unknown> }> = [];
  const view = (item: Stored) => ({
    id: item.id,
    name: item.name,
    kind: item.kind,
    config_json: Object.fromEntries(
      Object.entries(item.config).map(([key, value]) => [key, SECRETS.includes(key) ? '***' : value]),
    ),
    secret_keys: SECRETS.filter((key) => item.config[key]),
    enabled: item.enabled,
    created_at_micros: 1,
    updated_at_micros: 1,
  });

  await page.route('**/api/v1/connectors', (route) => {
    if (route.request().method() === 'GET') return route.fulfill({ json: stored.map(view) });

    const body = route.request().postDataJSON() as { name: string; kind: string; config_json: Record<string, unknown>; enabled?: boolean };
    requests.push({ method: 'POST', body });
    const created = { id: `c-${stored.length + 1}`, name: body.name, kind: body.kind, config: body.config_json, enabled: body.enabled ?? true };
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
    const body = route.request().postDataJSON() as { name: string; kind: string; config_json: Record<string, unknown>; enabled: boolean };
    requests.push({ method: 'PUT', body });
    const config = Object.fromEntries(
      Object.entries(body.config_json).map(([key, value]) => [key, value === '***' ? current.config[key] : value]),
    );
    stored[index] = { ...current, name: body.name, config, enabled: body.enabled };
    return route.fulfill({ json: view(stored[index]!) });
  });
  await page.route('**/api/v1/streams', (route) =>
    route.request().method() === 'GET'
      ? route.fulfill({
          json: ['default', 'edge'].map((name) => ({
            id: `logs/${name}`,
            name,
            stream_type: 'logs',
            schema: { fields: [] },
            created_at_micros: 1,
            updated_at_micros: 1,
          })),
        })
      : route.fallback(),
  );

  return { stored, requests };
}

const ARCHIVE: Stored = {
  id: 'c-archive',
  name: 'archive',
  kind: 's3',
  config: { bucket: 'acme-logs', prefix: 'out/', access_key: 'AKIAEXAMPLE', secret_key: 'kept-secret' },
  enabled: true,
};
const EDGE: Stored = {
  id: 'c-edge',
  name: 'edge-requests',
  kind: 'cloudflare_logpush',
  config: { target_stream: 'edge', push_token: 'a-token-that-was-saved-earlier' },
  enabled: true,
};

const section = (page: Page, direction: 'source' | 'destination') =>
  page.locator(`[data-connector-section="${direction}"]`);
const drawer = (page: Page) => page.getByRole('dialog');
const authModes = (page: Page) => drawer(page).getByRole('group', { name: 'Authentication' });

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
    await expect(authModes(page).getByRole('button')).toHaveText(['Access key', 'Temporary key', 'Node role']);

    // A temporary key is the key, its secret, and the token it was issued with.
    await authModes(page).getByRole('button', { name: 'Temporary key' }).click();
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
      config_json: {
        bucket: 'acme-backup',
        endpoint: 'https://oss-cn-hangzhou.aliyuncs.com',
        virtual_hosted_style: true,
        access_key: 'ASIAEXAMPLE',
        secret_key: 'temporary-secret',
        session_token: 'issued-with-the-key',
      },
      enabled: true,
    });
    await expect(section(page, 'destination').getByRole('row').nth(1)).toContainText(
      'oss-backupAmazon S3s3://acme-backup · oss-cn-hangzhou.aliyuncs.comTemporary key',
    );
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
    const token = String((backend.requests[0]?.body.config_json as Record<string, unknown>).push_token);
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
    expect(backend.requests[0]?.body.config_json).toMatchObject({ prefix: 'archive/', secret_key: '***' });
    expect(backend.stored[0]?.config).toMatchObject({ prefix: 'archive/', secret_key: 'kept-secret' });

    await section(page, 'destination').getByRole('switch', { name: 'Turn archive on or off' }).click();
    await expect(section(page, 'destination').getByRole('switch')).toHaveAttribute('aria-checked', 'false');
    expect(backend.stored[0]).toMatchObject({ enabled: false, config: { secret_key: 'kept-secret' } });
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
              default_home_route: '/home',
              time_format: 'iso_24h',
              date_format: 'yyyy_mm_dd_dash',
              timezone: '',
              keyboard_shortcuts_enabled: true,
            },
          })
        : route.fallback(),
    );
    await page.goto('/pipelines/connectors');

    await expect(page.getByTestId('page-header').getByRole('heading', { level: 1 })).toHaveText('连接器');
    await expect(section(page, 'destination').getByRole('row').nth(1)).toContainText('访问密钥');
    await page.getByRole('button', { name: '新建连接器' }).click();
    await drawer(page).getByRole('button', { name: /^Apache Kafka/ }).click();
    await expect(drawer(page).locator('[data-connector-auth-hint]')).toContainText('连接不加密');

    const text = `${await page.locator('#main').innerText()}\n${await drawer(page).innerText()}`;
    expect(text).not.toMatch(/\bconnectors\.[a-z_.]+\b/);
    expect(text).not.toMatch(/New connector|Authentication|Deliver to|Destinations|Sources/);
  });
});
