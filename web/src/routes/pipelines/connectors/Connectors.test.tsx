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
}));
vi.mock('@/api/streams', () => ({
  list: async () => [{ name: 'edge', stream_type: 'logs' }],
}));
vi.mock('@/api/instance', () => ({ get: async () => ({ external_url: 'https://obs.example.com/' }) }));
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
    config_json: config,
    secret_keys: Object.keys(config).filter((key) => config[key] === '***'),
    enabled: true,
    created_at_micros: 1,
    updated_at_micros: 1,
    ...extra,
  };
}

const ARCHIVE = saved('c-s3', 'archive', 's3', { bucket: 'acme-logs', prefix: 'out/', access_key: '***', secret_key: '***' });
const EDGE = saved('c-cf', 'edge-requests', 'cloudflare_logpush', { target_stream: 'edge', push_token: '***' });
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

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  Element.prototype.scrollIntoView = vi.fn();
  vi.clearAllMocks();
  api.create.mockImplementation(async (payload) => saved('new', payload.name, payload.kind, payload.config_json));
  api.update.mockImplementation(async (id, payload) => saved(id, payload.name, payload.kind, payload.config_json));
  api.remove.mockResolvedValue(undefined);
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
      config_json: { bucket: 'acme-logs', access_key: 'AKIAEXAMPLE', secret_key: 's3cr3t' },
      enabled: true,
    });
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
  });

  it('offers every way an S3 connector can authenticate, and stores no key for the node role', async () => {
    renderPage();
    await newConnector('Amazon S3');
    type(drawer().getByRole('textbox', { name: 'Name' }), 'audit');
    type(drawer().getByRole('textbox', { name: 'Bucket' }), 'acme-audit');
    type(drawer().getByLabelText(/^Access key ID/), 'AKIAEXAMPLE');

    const modes = within(drawer().getByRole('group', { name: 'Authentication' }));
    expect(modes.getAllByRole('button').map((mode) => mode.textContent)).toEqual([
      'Access key',
      'Temporary key',
      'Node role',
    ]);
    fireEvent.click(modes.getByRole('button', { name: 'Temporary key' }));
    expect(drawer().getByLabelText(/^Session token/)).not.toBeNull();
    expect(document.querySelector('[data-connector-auth-hint]')?.textContent).toContain('valid only together with its session token');

    fireEvent.click(modes.getByRole('button', { name: 'Node role' }));
    expect(drawer().queryByLabelText(/^Access key ID/)).toBeNull();
    expect(document.querySelector('[data-connector-auth-hint]')?.textContent).toContain('No key is stored');
    fireEvent.click(submit());

    await waitFor(() => expect(api.create).toHaveBeenCalledTimes(1));
    // The key typed for another way of authenticating is not sent.
    expect(sent()?.config_json).toEqual({ bucket: 'acme-audit' });
  });

  it('reaches an S3-compatible store by its endpoint, addressed the way it asks', async () => {
    renderPage();
    await newConnector('Amazon S3');
    type(drawer().getByRole('textbox', { name: 'Name' }), 'oss');
    type(drawer().getByRole('textbox', { name: 'Bucket' }), 'acme-backup');
    fireEvent.click(within(drawer().getByRole('group', { name: 'Authentication' })).getByRole('button', { name: 'Node role' }));

    expect(drawer().queryByRole('textbox', { name: /^Endpoint/ })).toBeNull();
    fireEvent.click(drawer().getByRole('checkbox', { name: 'S3-compatible storage' }));
    type(drawer().getByRole('textbox', { name: /^Endpoint/ }), 'oss-cn-hangzhou.aliyuncs.com');
    expect(submit()).toBeDisabled();
    type(drawer().getByRole('textbox', { name: /^Endpoint/ }), 'https://oss-cn-hangzhou.aliyuncs.com');
    fireEvent.click(drawer().getByRole('checkbox', { name: 'Address the bucket as a host name' }));
    fireEvent.click(submit());

    await waitFor(() => expect(api.create).toHaveBeenCalledTimes(1));
    expect(sent()?.config_json).toEqual({
      bucket: 'acme-backup',
      endpoint: 'https://oss-cn-hangzhou.aliyuncs.com',
      virtual_hosted_style: true,
    });
  });

  it('is plain about what a Kafka connection does not protect', async () => {
    renderPage();
    await newConnector('Apache Kafka');
    const hint = () => document.querySelector('[data-connector-auth-hint]')?.textContent;

    expect(hint()).toContain('The connection is not encrypted');
    fireEvent.click(within(drawer().getByRole('group', { name: 'Authentication' })).getByRole('button', { name: 'User name and password' }));
    expect(hint()).toContain('TLS and SCRAM are not supported yet');

    type(drawer().getByRole('textbox', { name: 'Name' }), 'bus');
    type(drawer().getByRole('textbox', { name: /^Brokers/ }), 'b1:9092');
    type(drawer().getByRole('textbox', { name: 'Topic' }), 'logs');
    type(drawer().getByRole('textbox', { name: 'User name' }), 'svc');
    expect(submit()).toBeDisabled();
    type(drawer().getByLabelText(/^Password/), 'pw');
    fireEvent.click(submit());

    await waitFor(() => expect(api.create).toHaveBeenCalledTimes(1));
    expect(sent()?.config_json).toEqual({ brokers: 'b1:9092', topic: 'logs', sasl_username: 'svc', sasl_password: 'pw' });
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
    const token = String(sent()?.config_json.push_token);
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
    const token = String(sent()?.config_json.push_token);
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
    expect(sent()?.config_json).toEqual({ bucket: 'acme-logs', prefix: 'archive/', access_key: '***', secret_key: '***' });
  });

  it('replaces only the secret that was typed', async () => {
    renderPage([ARCHIVE]);

    fireEvent.click(await screen.findByRole('button', { name: 'Edit' }));
    type(drawer().getByLabelText(/^Secret access key/), 'rotated');
    fireEvent.click(submit());

    await waitFor(() => expect(api.update).toHaveBeenCalledTimes(1));
    expect(sent()?.config_json).toMatchObject({ access_key: '***', secret_key: 'rotated' });
  });

  it('asks for the secrets again, and sends no mask, where the backend would save the mask itself', async () => {
    const { secret_keys: _unlisted, ...older } = ARCHIVE;
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
    expect(sent()?.config_json).toEqual({ bucket: 'acme-logs', prefix: 'out/', access_key: 'AKIAEXAMPLE', secret_key: 's3cr3t' });
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
    const token = String(sent()?.config_json.push_token);
    expect(token).toMatch(/^[A-Za-z0-9_-]{43}$/);
    expect(await drawer().findByText(token)).not.toBeNull();
  });

  it('switches a connector off and on, sending its configuration back as it was read', async () => {
    renderPage([ARCHIVE]);

    fireEvent.click(await screen.findByRole('switch', { name: 'Turn archive on or off' }));

    await waitFor(() => expect(api.update).toHaveBeenCalledTimes(1));
    expect(api.update.mock.calls[0]).toEqual([
      'c-s3',
      { name: 'archive', kind: 's3', config_json: ARCHIVE.config_json, enabled: false },
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
