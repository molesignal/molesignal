import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as connectorsApi from '@/api/connectors';
import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { Connectors } from './Connectors';

vi.mock('@/api/connectors', () => ({
  list: vi.fn(),
  create: vi.fn(),
  update: vi.fn(),
  remove: vi.fn(),
  testConnection: vi.fn(),
  awsExternalId: vi.fn(),
}));
vi.mock('@/api/streams', () => ({
  list: async () => [{ name: 'edge', streamType: 'logs' }],
}));
vi.mock('@/api/instance', () => ({ get: async () => ({ externalUrl: 'https://obs.example.com/' }) }));
vi.mock('@/product/actionAccess', () => ({
  useActionAccess: () => ({ allowed: true, disabled: false, reason: undefined }),
}));
vi.mock('@/shell/ui/sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
// The stream picker is a field like any other here; what it offers is its own test.
vi.mock('../editor/StreamCombobox', () => ({
  StreamCombobox: ({ value, onChange }: { value: string; onChange: (stream: string) => void }) => (
    <input value={value} onChange={(event) => onChange(event.target.value)} />
  ),
}));

const api = vi.mocked(connectorsApi);

function saved(
  id: string,
  name: string,
  kind: string,
  config: Record<string, unknown>,
  extra: Partial<connectorsApi.Connector> = {},
): connectorsApi.Connector {
  return {
    id,
    name,
    kind,
    configJson: config,
    secretKeys: Object.keys(config).filter((key) => config[key] === '***'),
    enabled: true,
    createdAt: 1,
    updatedAt: 1,
    ...extra,
  };
}

const ARCHIVE = saved('c-s3', 'archive', 's3', { bucket: 'acme-logs', prefix: 'out/', accessKey: '***', secretKey: '***' });
const EDGE = saved('c-cf', 'edge-requests', 'cloudflare_logpush', { targetStream: 'edge', pushToken: '***' });
const BUS = saved('c-kafka', 'events-bus', 'kafka', { brokers: 'b1:9092', topic: 'logs' });

function renderPage(connectors: connectorsApi.Connector[] = []) {
  api.list.mockResolvedValue(connectors);
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <TooltipProvider delayDuration={0}>
        <MemoryRouter initialEntries={['/pipelines/connectors']}>
          <Connectors />
        </MemoryRouter>
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

const section = (direction: 'source' | 'destination') =>
  within(document.querySelector<HTMLElement>(`[data-connector-section="${direction}"]`)!);
const rows = (direction: 'source' | 'destination') =>
  section(direction)
    .queryAllByRole('row')
    .slice(1)
    .map((row) =>
      within(row)
        .getAllByRole('cell')
        .slice(0, 4)
        .map((cell) => cell.textContent),
    );
const drawer = () => within(screen.getByRole('dialog'));
const type = (field: HTMLElement, value: string) => fireEvent.change(field, { target: { value } });
const submit = () => drawer().getByRole('button', { name: /Create connector|Save changes/ });
const sent = () => api.create.mock.calls.at(-1)?.[0] ?? api.update.mock.calls.at(-1)?.[1];

async function newConnector(kind: string) {
  fireEvent.click(await screen.findByRole('button', { name: 'New connector' }));
  fireEvent.click(drawer().getByRole('button', { name: new RegExp(`^${kind}`) }));
}

/** The ways on offer, as the list of them reads. */
function authModes(): string[] {
  fireEvent.keyDown(drawer().getByRole('combobox', { name: 'Authentication' }), { key: 'Enter' });
  const names = screen.getAllByRole('option').map((option) => option.textContent ?? '');
  fireEvent.keyDown(screen.getByRole('listbox'), { key: 'Escape' });

  return names;
}

function chooseAuth(name: string) {
  fireEvent.keyDown(drawer().getByRole('combobox', { name: 'Authentication' }), { key: 'Enter' });
  fireEvent.click(screen.getByRole('option', { name }));
}

const hint = () => document.querySelector('[data-connector-auth-hint]')?.textContent;

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  Element.prototype.scrollIntoView = vi.fn();
  vi.clearAllMocks();
  api.create.mockImplementation(async (payload) => saved('new', payload.name, payload.kind, payload.configJson));
  api.update.mockImplementation(async (id, payload) => saved(id, payload.name, payload.kind, payload.configJson));
  api.remove.mockResolvedValue(undefined);
  api.awsExternalId.mockResolvedValue('molesignal-org-1');
});

afterEach(cleanup);

describe('connectors', () => {
  it('keeps what brings logs in apart from where pipelines deliver, each with its own columns', async () => {
    renderPage([ARCHIVE, EDGE, BUS, saved('c-x', 'mystery', 'webhook', {})]);

    await waitFor(() => expect(rows('destination')).toHaveLength(2));
    expect(rows('source')).toEqual([
      ['edge-requests', 'Cloudflare LogpushPushed', 'edge', 'Never'],
      // A kind this version does not know is shown for what it is, among the sources.
      ['mystery', 'A kind this version does not know: webhook', 'default', 'Never'],
    ]);
    expect(rows('destination')).toEqual([
      ['archive', 'Amazon S3', 's3://acme-logs/out/', 'Access key'],
      ['events-bus', 'Apache Kafka', 'logs · b1:9092', 'None'],
    ]);
    expect(screen.getByRole('heading', { level: 1 }).textContent).toBe('Connectors');
  });

  it('says when a source last failed to read, and why', async () => {
    renderPage([
      {
        ...saved('c-topic', 'orders-topic', 'kafka_source', { brokers: 'b1:9092', topic: 'orders' }),
        readState: { lastError: { atTime: 1, text: 'kafka metadata: broker down' } },
      },
    ]);

    const sources = await screen.findByRole('region', { name: 'Sources' });

    expect(within(sources).getByText('orders-topic')).toBeTruthy();
    expect(within(sources).getByRole('img', { name: 'The last read failed' })).toBeTruthy();
  });

  it('says so in each section when it has none', async () => {
    renderPage();

    expect(await screen.findByText('No sources yet.')).not.toBeNull();
    expect(screen.getByText('No destinations yet.')).not.toBeNull();
  });

  it('starts a new connector by what it connects to, the asked-for direction first', async () => {
    renderPage();

    fireEvent.click(await screen.findByRole('button', { name: 'Add a destination' }));

    expect(drawer().getAllByRole('heading', { level: 3 }).map((heading) => heading.textContent)).toEqual([
      'Destinations',
      'Sources',
    ]);
    fireEvent.click(drawer().getByRole('button', { name: /^Apache Kafka/ }));
    expect(drawer().getByRole('textbox', { name: /^Brokers/ })).not.toBeNull();

    // Before it is saved it can still be another kind.
    fireEvent.click(drawer().getByRole('button', { name: 'Choose another' }));
    expect(drawer().getByRole('button', { name: /^Amazon S3/ })).not.toBeNull();
  });

  it('creates an S3 destination with an access key, and names what is missing until it can', async () => {
    renderPage();
    await newConnector('Amazon S3');

    expect(submit()).toBeDisabled();
    type(drawer().getByRole('textbox', { name: 'Name' }), 'archive');
    type(drawer().getByRole('textbox', { name: 'Bucket' }), 'acme-logs');
    type(drawer().getByLabelText(/^Access key ID/), 'AKIAEXAMPLE');
    expect(submit()).toBeDisabled();
    type(drawer().getByLabelText(/^Secret access key/), 's3cr3t');
    expect(submit()).toBeEnabled();
    fireEvent.click(submit());

    await waitFor(() => expect(api.create).toHaveBeenCalledTimes(1));
    expect(sent()).toEqual({
      name: 'archive',
      kind: 's3',
      configJson: { bucket: 'acme-logs', auth: 'access_key', accessKey: 'AKIAEXAMPLE', secretKey: 's3cr3t' },
      enabled: true,
    });
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  });

  it('offers every way an S3 connector can authenticate, and stores no key for the node identity', async () => {
    renderPage();
    await newConnector('Amazon S3');
    type(drawer().getByRole('textbox', { name: 'Name' }), 'audit');
    type(drawer().getByRole('textbox', { name: 'Bucket' }), 'acme-audit');
    type(drawer().getByLabelText(/^Access key ID/), 'AKIAEXAMPLE');

    expect(authModes()).toEqual(['Access key', 'Temporary key', 'Node identity', 'Assume a role', 'Web identity']);
    chooseAuth('Temporary key');
    expect(drawer().getByLabelText(/^Session token/)).not.toBeNull();
    expect(hint()).toContain('valid only together with its session token');

    chooseAuth('Node identity');
    expect(drawer().queryByLabelText(/^Access key ID/)).toBeNull();
    expect(hint()).toContain('s3:PutObject on the bucket');
    expect(hint()).toContain('every organization in this deployment can choose it');
    fireEvent.click(submit());

    await waitFor(() => expect(api.create).toHaveBeenCalledTimes(1));
    // The key typed for another way of authenticating is not sent.
    expect(sent()?.configJson).toEqual({ bucket: 'acme-audit', auth: 'node_role' });
  });

  it('assumes a role with the organization\'s external ID, as the node unless a key is asked for', async () => {
    renderPage();
    await newConnector('Amazon S3');
    type(drawer().getByRole('textbox', { name: 'Name' }), 'cross-account');
    type(drawer().getByRole('textbox', { name: 'Bucket' }), 'acme-partner');
    chooseAuth('Assume a role');

    expect(await drawer().findByText('molesignal-org-1')).not.toBeNull();
    expect(hint()).toContain('require the external ID below');
    type(drawer().getByRole('textbox', { name: /^Role ARN/ }), 'molesignal-delivery');
    expect(submit()).toBeDisabled();
    type(drawer().getByRole('textbox', { name: /^Role ARN/ }), 'arn:aws:iam::123456789012:role/molesignal-delivery');

    fireEvent.click(drawer().getByRole('checkbox', { name: /with an access key/ }));
    expect(submit()).toBeDisabled();
    type(drawer().getByLabelText(/^Access key ID/), 'AKIAEXAMPLE');
    type(drawer().getByLabelText(/^Secret access key/), 's3cr3t');
    fireEvent.click(submit());

    await waitFor(() => expect(api.create).toHaveBeenCalledTimes(1));
    expect(sent()?.configJson).toEqual({
      bucket: 'acme-partner',
      auth: 'assume_role',
      roleArn: 'arn:aws:iam::123456789012:role/molesignal-delivery',
      accessKey: 'AKIAEXAMPLE',
      secretKey: 's3cr3t',
    });
  });

  it('reaches an S3-compatible store by its endpoint, addressed the way it asks', async () => {
    renderPage();
    await newConnector('Amazon S3');
    type(drawer().getByRole('textbox', { name: 'Name' }), 'oss');
    type(drawer().getByRole('textbox', { name: 'Bucket' }), 'acme-backup');
    chooseAuth('Node identity');

    expect(drawer().queryByRole('textbox', { name: /^Endpoint/ })).toBeNull();
    fireEvent.click(drawer().getByRole('checkbox', { name: 'S3-compatible storage' }));
    type(drawer().getByRole('textbox', { name: /^Endpoint/ }), 'oss-cn-hangzhou.aliyuncs.com');
    expect(submit()).toBeDisabled();
    type(drawer().getByRole('textbox', { name: /^Endpoint/ }), 'https://oss-cn-hangzhou.aliyuncs.com');
    fireEvent.click(drawer().getByRole('checkbox', { name: 'Address the bucket as a host name' }));
    fireEvent.click(submit());

    await waitFor(() => expect(api.create).toHaveBeenCalledTimes(1));
    expect(sent()?.configJson).toEqual({
      bucket: 'acme-backup',
      endpoint: 'https://oss-cn-hangzhou.aliyuncs.com',
      virtualHostedStyle: true,
      auth: 'node_role',
    });
  });

  it('connects to Kafka over TLS with SCRAM, and is plain about PLAIN without it', async () => {
    renderPage();
    await newConnector('Apache Kafka');

    expect(authModes()).toEqual(['None', 'SASL/PLAIN', 'SCRAM-SHA-256', 'SCRAM-SHA-512', 'Client certificate']);
    expect(hint()).toBe('For a broker that asks for no credentials.');
    chooseAuth('SASL/PLAIN');
    expect(hint()).toContain('without TLS, use it on a private network only');

    type(drawer().getByRole('textbox', { name: 'Name' }), 'bus');
    type(drawer().getByRole('textbox', { name: /^Brokers/ }), 'b1:9096');
    type(drawer().getByRole('textbox', { name: 'Topic' }), 'logs');
    expect(drawer().queryByRole('textbox', { name: /^CA certificate/ })).toBeNull();
    fireEvent.click(drawer().getByRole('checkbox', { name: 'Encrypt the connection with TLS' }));
    expect(drawer().getByRole('textbox', { name: /^CA certificate/ })).not.toBeNull();

    chooseAuth('SCRAM-SHA-512');
    type(drawer().getByRole('textbox', { name: 'User name' }), 'svc');
    expect(submit()).toBeDisabled();
    type(drawer().getByLabelText(/^Password/), 'pw');
    fireEvent.click(submit());

    await waitFor(() => expect(api.create).toHaveBeenCalledTimes(1));
    expect(sent()?.configJson).toEqual({
      brokers: 'b1:9096',
      topic: 'logs',
      tls: true,
      auth: 'sasl_scram_sha_512',
      saslUsername: 'svc',
      saslPassword: 'pw',
    });
  });

  it('presents a client certificate to Kafka, which turns TLS on', async () => {
    const certificate = '-----BEGIN CERTIFICATE-----\nMIIB\n-----END CERTIFICATE-----';
    const key = '-----BEGIN PRIVATE KEY-----\nMIIE\n-----END PRIVATE KEY-----';

    renderPage();
    await newConnector('Apache Kafka');
    type(drawer().getByRole('textbox', { name: 'Name' }), 'msk');
    type(drawer().getByRole('textbox', { name: /^Brokers/ }), 'b-1.msk:9094');
    type(drawer().getByRole('textbox', { name: 'Topic' }), 'logs');
    chooseAuth('Client certificate');

    expect(drawer().getByRole('checkbox', { name: 'Encrypt the connection with TLS' })).toBeChecked();
    type(drawer().getByRole('textbox', { name: /^Client certificate/ }), certificate);
    type(drawer().getByRole('textbox', { name: /^Client private key/ }), certificate);
    // The certificate pasted where the key goes is told apart.
    expect(submit()).toBeDisabled();
    type(drawer().getByRole('textbox', { name: /^Client private key/ }), key);
    fireEvent.click(submit());

    await waitFor(() => expect(api.create).toHaveBeenCalledTimes(1));
    expect(sent()?.configJson).toEqual({
      brokers: 'b-1.msk:9094',
      topic: 'logs',
      tls: true,
      auth: 'mtls',
      tlsClientCert: certificate,
      tlsClientKey: key,
    });
  });

  it('delivers to an HTTP address, signed with a shared secret', async () => {
    renderPage();
    await newConnector('HTTP / Webhook');
    type(drawer().getByRole('textbox', { name: 'Name' }), 'collector');
    type(drawer().getByLabelText(/^URL/), 'https://in.example.com/v1/logs');

    expect(authModes()).toEqual(['None', 'Bearer token', 'Basic auth', 'Custom header', 'HMAC signature']);
    chooseAuth('HMAC signature');
    expect(hint()).toContain('X-MoleSignal-Sign: sha256=<hex>');
    type(drawer().getByLabelText(/^Shared secret/), 'short');
    expect(submit()).toBeDisabled();
    type(drawer().getByLabelText(/^Shared secret/), '0123456789abcdef');
    fireEvent.click(submit());

    await waitFor(() => expect(api.create).toHaveBeenCalledTimes(1));
    expect(sent()?.configJson).toEqual({
      url: 'https://in.example.com/v1/logs',
      auth: 'hmac',
      hmacSecret: '0123456789abcdef',
    });
  });

  it('tests the settings as they stand, and forgets the answer once they change', async () => {
    api.testConnection.mockResolvedValue({
      ok: false,
      code: 'denied',
      detail: 's3 put: <Code>AccessDenied</Code>',
      elapsedMs: 42,
    });

    renderPage();
    await newConnector('Amazon S3');
    chooseAuth('Node identity');
    const test = () => drawer().getByRole('button', { name: 'Test connection' });
    const status = () => document.querySelector('[data-connector-test]')?.querySelector('[role="status"]')?.textContent;

    // Nothing to test until it says where to deliver; a name is not needed.
    expect(test()).toBeDisabled();
    type(drawer().getByRole('textbox', { name: 'Bucket' }), 'acme-logs');
    expect(test()).toBeEnabled();
    fireEvent.click(test());

    await waitFor(() => expect(status()).toBe('Credentials or permissions refused'));
    expect(api.testConnection).toHaveBeenCalledWith({
      kind: 's3',
      configJson: { bucket: 'acme-logs', auth: 'node_role' },
    });
    expect(drawer().getByText('s3 put: <Code>AccessDenied</Code>')).not.toBeNull();

    type(drawer().getByRole('textbox', { name: 'Bucket' }), 'acme-logs-2');
    expect(status()).toBe('');
    expect(drawer().queryByText('s3 put: <Code>AccessDenied</Code>')).toBeNull();

    api.testConnection.mockResolvedValue({ ok: true, code: 'ok', elapsedMs: 87 });
    fireEvent.click(test());
    await waitFor(() => expect(status()).toBe('Connected in 87 ms'));
  });

  it('tests a saved connector with the secrets it holds', async () => {
    api.testConnection.mockResolvedValue({ ok: true, code: 'ok', elapsedMs: 5 });
    renderPage([ARCHIVE]);

    fireEvent.click(await screen.findByRole('button', { name: 'Edit' }));
    fireEvent.click(drawer().getByRole('button', { name: 'Test connection' }));

    await waitFor(() => expect(api.testConnection).toHaveBeenCalledTimes(1));
    expect(api.testConnection).toHaveBeenCalledWith({
      kind: 's3',
      configJson: { bucket: 'acme-logs', prefix: 'out/', auth: 'access_key', accessKey: '***', secretKey: '***' },
      id: 'c-s3',
    });
  });

  it('offers no test for a push connector, which is sent to', async () => {
    renderPage();
    await newConnector('Cloudflare Logpush');

    expect(drawer().queryByRole('button', { name: 'Test connection' })).toBeNull();
  });

  it('gives a push connector a token, and hands it over once with where to point the service', async () => {
    renderPage();
    await newConnector('Cloudflare Logpush');

    // Where to push is known before the connector exists; the token is not typed by anyone.
    expect(await drawer().findByText('https://obs.example.com/api/v1/_cloudflare')).not.toBeNull();
    expect(drawer().queryByLabelText(/^Token/)).toBeNull();
    type(drawer().getByRole('textbox', { name: 'Name' }), 'edge');
    fireEvent.click(submit());

    await waitFor(() => expect(api.create).toHaveBeenCalledTimes(1));
    const token = String(sent()?.configJson.pushToken);
    expect(token).toMatch(/^[A-Za-z0-9_-]{43}$/);

    expect(await drawer().findByRole('heading', { name: 'Point Cloudflare Logpush here' })).not.toBeNull();
    expect(drawer().getByText(token)).not.toBeNull();
    expect(
      drawer().getByText(`https://obs.example.com/api/v1/_cloudflare?header_X-Connector-Token=${token}`),
    ).not.toBeNull();
    expect(drawer().getByText(/The token is shown this once/)).not.toBeNull();

    fireEvent.click(drawer().getByRole('button', { name: 'Done' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  });

  it('carries a Heroku token in the address, since Heroku sends no header', async () => {
    renderPage();
    await newConnector('Heroku log drain');
    type(drawer().getByRole('textbox', { name: 'Name' }), 'legacy');
    fireEvent.click(submit());

    await waitFor(() => expect(api.create).toHaveBeenCalledTimes(1));
    const token = String(sent()?.configJson.pushToken);
    expect(
      await drawer().findByText(`heroku drains:add "https://obs.example.com/api/v1/_heroku?token=${token}"`),
    ).not.toBeNull();
  });

  it('changes a saved connector without its secrets being typed again', async () => {
    renderPage([ARCHIVE]);

    fireEvent.click(await screen.findByRole('button', { name: 'Edit' }));
    expect(drawer().getByRole('heading', { name: 'Edit archive' })).not.toBeNull();
    // Saved, a connector is what it is: its kind is not on offer.
    expect(drawer().queryByRole('button', { name: 'Choose another' })).toBeNull();
    expect(drawer().getByLabelText<HTMLInputElement>(/^Secret access key/).value).toBe('');
    expect(drawer().getAllByText('Saved. Leave empty to keep it.')).toHaveLength(2);

    type(drawer().getByRole('textbox', { name: /^Key prefix/ }), 'archive/');
    fireEvent.click(submit());

    await waitFor(() => expect(api.update).toHaveBeenCalledTimes(1));
    expect(api.update.mock.calls[0]?.[0]).toBe('c-s3');
    // The mask goes back for what was left alone; the backend keeps what it has.
    expect(sent()?.configJson).toEqual({
      bucket: 'acme-logs',
      prefix: 'archive/',
      auth: 'access_key',
      accessKey: '***',
      secretKey: '***',
    });
  });

  it('replaces only the secret that was typed', async () => {
    renderPage([ARCHIVE]);

    fireEvent.click(await screen.findByRole('button', { name: 'Edit' }));
    type(drawer().getByLabelText(/^Secret access key/), 'rotated');
    fireEvent.click(submit());

    await waitFor(() => expect(api.update).toHaveBeenCalledTimes(1));
    expect(sent()?.configJson).toMatchObject({ accessKey: '***', secretKey: 'rotated' });
  });

  it('asks for the secrets again, and sends no mask, where the backend would save the mask itself', async () => {
    const { secretKeys: _unlisted, ...older } = ARCHIVE;
    renderPage([older]);

    // Switching it would send its configuration back, masks and all.
    const toggle = await screen.findByRole('switch', { name: 'Turn archive on or off' });
    expect(toggle).toBeDisabled();

    fireEvent.click(screen.getByRole('button', { name: 'Edit' }));
    expect(drawer().getAllByText(/does not keep a saved secret/)).toHaveLength(2);
    expect(submit()).toBeDisabled();
    type(drawer().getByLabelText(/^Access key ID/), 'AKIAEXAMPLE');
    type(drawer().getByLabelText(/^Secret access key/), 's3cr3t');
    fireEvent.click(submit());

    await waitFor(() => expect(api.update).toHaveBeenCalledTimes(1));
    expect(sent()?.configJson).toEqual({
      bucket: 'acme-logs',
      prefix: 'out/',
      auth: 'access_key',
      accessKey: 'AKIAEXAMPLE',
      secretKey: 's3cr3t',
    });
  });

  it('keeps a push token until a new one is asked for, then shows the new one once', async () => {
    renderPage([EDGE]);

    fireEvent.click(await screen.findByRole('button', { name: 'Edit' }));
    const state = () => document.querySelector('[data-connector-token-state]')?.textContent;
    expect(state()).toBe('A token is saved. It cannot be shown again.');

    fireEvent.click(drawer().getByRole('button', { name: 'Generate a new token' }));
    expect(state()).toContain('A new token is ready');
    fireEvent.click(drawer().getByRole('button', { name: 'Keep the current token' }));
    expect(state()).toBe('A token is saved. It cannot be shown again.');
    fireEvent.click(drawer().getByRole('button', { name: 'Generate a new token' }));
    fireEvent.click(submit());

    await waitFor(() => expect(api.update).toHaveBeenCalledTimes(1));
    const token = String(sent()?.configJson.pushToken);
    expect(token).toMatch(/^[A-Za-z0-9_-]{43}$/);
    expect(await drawer().findByText(token)).not.toBeNull();
  });

  it('switches a connector off and on, sending its configuration back as it was read', async () => {
    renderPage([ARCHIVE]);

    fireEvent.click(await screen.findByRole('switch', { name: 'Turn archive on or off' }));

    await waitFor(() => expect(api.update).toHaveBeenCalledTimes(1));
    expect(api.update.mock.calls[0]).toEqual([
      'c-s3',
      { name: 'archive', kind: 's3', configJson: ARCHIVE.configJson, enabled: false },
    ]);
  });

  it('says what deleting a connector does before it does it', async () => {
    renderPage([ARCHIVE, EDGE]);

    await screen.findByText('archive');
    fireEvent.click(section('destination').getByRole('button', { name: 'Delete' }));
    const confirm = within(screen.getByRole('dialog'));
    expect(confirm.getByText('Delete archive?')).not.toBeNull();
    expect(confirm.getByText(/records an error on every run/)).not.toBeNull();
    fireEvent.click(confirm.getByRole('button', { name: 'Delete connector' }));

    await waitFor(() => expect(api.remove).toHaveBeenCalledWith('c-s3'));
  });
});
