import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';
import { useAuthStore } from '@/stores/auth';

const mocks = vi.hoisted(() => ({
  overview: vi.fn(),
  getDefault: vi.fn(),
  getRumClient: vi.fn(),
  instance: vi.fn(),
}));

vi.mock('@/api/home', () => ({ overview: mocks.overview }));
vi.mock('@/api/apiTokens', () => ({
  getDefault: mocks.getDefault,
  getRumClient: mocks.getRumClient,
}));
vi.mock('@/api/instance', () => ({ get: mocks.instance }));
// Colouring is not the subject: the blocks keep the text they were given.
vi.mock('./codeBlock/highlighter', () => ({
  highlightCode: vi.fn(() => new Promise(() => {})),
}));

import { Datasource } from './Datasource';

function Address() {
  const location = useLocation();
  return <output data-testid="address">{location.pathname + location.search}</output>;
}

function renderAt(path: string) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <TooltipProvider delayDuration={0}>
        <MemoryRouter initialEntries={[path]}>
          <Routes>
            <Route path="datasource" element={<Datasource />} />
            <Route path="datasource/:category" element={<Datasource />} />
            <Route path="datasource/:category/:source" element={<Datasource />} />
          </Routes>
          <Address />
        </MemoryRouter>
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

const address = () => screen.getByTestId('address').textContent;
const rows = () =>
  Array.from(document.querySelectorAll('[data-datasource-source]'), (row) =>
    row.getAttribute('data-datasource-source'),
  );
const guide = () => document.querySelector<HTMLElement>('[data-datasource-guide]')!;
const endpoints = () =>
  Array.from(
    document.querySelectorAll('[data-guide-connection] dt'),
    (label, index) =>
      `${label.textContent}: ${
        document.querySelectorAll('[data-guide-connection] dd')[index]?.querySelector('code')
          ?.textContent ?? ''
      }`,
  );
const search = () => screen.getByPlaceholderText<HTMLInputElement>('Search all sources…');
const signal = (name: string) =>
  within(screen.getByRole('group', { name: 'Signal' })).getByRole('button', { name });

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  for (const mock of Object.values(mocks)) mock.mockReset();
  mocks.overview.mockResolvedValue({ signals: [], streams: [] });
  mocks.getDefault.mockResolvedValue({
    token: 'ms_test_0123456789_secret',
    role_name: 'Intake token',
    token_kind: 'default_intake',
    expires_at_micros: null,
  });
  mocks.instance.mockResolvedValue({ external_url: 'https://obs.example.com' });
  useAuthStore.setState({
    ctx: { user_id: 'u1', org_id: 'acme', org_name: 'Acme', display_role: 'admin', roles: [] },
  });
  // jsdom lays nothing out; the selected row asks to be scrolled into view.
  Element.prototype.scrollIntoView = vi.fn();
});

afterEach(() => {
  cleanup();
  useAuthStore.setState({ ctx: null });
});

describe('Datasource page', () => {
  it('opens on the first recommended source with its guide in three steps', async () => {
    renderAt('/datasource');

    await waitFor(() => expect(address()).toBe('/datasource/recommended/kubernetes'));
    expect(rows().slice(0, 3)).toEqual(['kubernetes', 'linux', 'windows']);
    expect(
      screen.getByRole('link', { name: /^Kubernetes/ }).getAttribute('aria-current'),
    ).toBe('page');
    expect(within(guide()).getByRole('heading', { level: 2, name: 'Kubernetes' })).not.toBeNull();
    expect(
      Array.from(guide().querySelectorAll('[data-guide-section] h3'), (title) => title.textContent),
    ).toEqual(['Endpoint and token', 'Set up the source', 'Verify data is arriving']);
  });

  it('keeps one primary action, and none in the title row', async () => {
    renderAt('/datasource/recommended/kubernetes');
    await screen.findByRole('button', { name: 'Verify connection' });

    expect(screen.getAllByRole('button', { name: 'Verify connection' })).toHaveLength(1);
    expect(within(screen.getByTestId('page-header')).queryAllByRole('button')).toHaveLength(0);
    // A row says what a source is; whether data arrives is the guide's to say.
    expect(screen.queryByText(/Unverified|Signal active/)).toBeNull();
  });

  it('puts the checks the reader runs on their side with the verification', async () => {
    renderAt('/datasource/recommended/kubernetes');
    await screen.findByRole('button', { name: 'Verify connection' });

    const sections = guide().querySelectorAll<HTMLElement>('[data-guide-section]');
    const titles = (section: HTMLElement) =>
      Array.from(section.querySelectorAll('[data-guide-step] h4'), (title) => title.textContent);

    expect(titles(sections[1]!)).toEqual([
      'Create the namespace and ConfigMap',
      'Deploy the Fluent Bit DaemonSet',
    ]);
    expect(titles(sections[2]!)).toEqual(['Check that logs are arriving']);
  });

  it('fills the commands with this workspace\'s endpoint and token', async () => {
    renderAt('/datasource/recommended/kubernetes');

    await waitFor(() =>
      expect(guide().textContent).toContain(
        'value: "https://obs.example.com/api/v1/intake/logs/default"',
      ),
    );
    expect(endpoints()).toEqual([
      'Endpoint: https://obs.example.com/api/v1/intake/logs/default',
      'Intake token: ms_test_••••••••••••••secret',
    ]);
  });

  it('lists one endpoint for each signal a source sends', async () => {
    renderAt('/datasource/applications/opentelemetry');
    await screen.findByRole('button', { name: 'Verify connection' });

    await waitFor(() =>
      expect(endpoints().slice(0, 3)).toEqual([
        'Logs endpoint: https://obs.example.com/api/v1/intake/logs/default',
        'Metrics endpoint: https://obs.example.com/api/v1/intake/metrics/default',
        'Traces endpoint: https://obs.example.com/api/v1/intake/traces/default',
      ]),
    );
  });

  it('shows the signal and stream a link came for', async () => {
    renderAt('/datasource/applications/opentelemetry?signal=metrics&stream=app_metrics');
    await screen.findByRole('button', { name: 'Verify connection' });

    expect(signal('Metrics').getAttribute('aria-pressed')).toBe('true');
    await waitFor(() =>
      expect(endpoints()[0]).toBe(
        'Metrics endpoint: https://obs.example.com/api/v1/intake/metrics/app_metrics',
      ),
    );
    // Only what can send metrics is listed, and the choice rides along to the next source.
    expect(rows()).not.toContain('github-actions');
    expect(screen.getByRole('link', { name: /^Python/ }).getAttribute('href')).toBe(
      '/datasource/applications/python?signal=metrics&stream=app_metrics',
    );
  });

  it('shows a source under a category that holds it, whatever the address says', async () => {
    renderAt('/datasource/recommended/postgres');

    await waitFor(() => expect(address()).toBe('/datasource/databases/postgres'));
    expect(screen.getByRole('link', { name: 'Databases' }).getAttribute('aria-current')).toBe(
      'page',
    );
    expect(within(guide()).getByRole('heading', { level: 2, name: 'PostgreSQL' })).not.toBeNull();
  });

  it('falls back to the first source for an address that names none', async () => {
    renderAt('/datasource/recommended/http-json');

    await waitFor(() => expect(address()).toBe('/datasource/recommended/kubernetes'));
  });

  it('narrows the catalogue to a signal and moves to a source that sends it', async () => {
    renderAt('/datasource/recommended/kubernetes');
    await screen.findByRole('button', { name: 'Verify connection' });

    fireEvent.click(signal('Metrics'));

    await waitFor(() => expect(address()).toBe('/datasource/recommended/linux?signal=metrics'));
    expect(rows()).toEqual(['linux', 'aws']);

    fireEvent.click(signal('All'));

    await waitFor(() => expect(address()).toBe('/datasource/recommended/linux'));
  });

  it('keeps what was typed in a guide when the signal in view changes', async () => {
    renderAt('/datasource/recommended/rum');
    const application = await screen.findByLabelText<HTMLInputElement>('RUM application ID');

    fireEvent.change(application, { target: { value: 'checkout-web' } });
    fireEvent.click(signal('Logs'));

    await waitFor(() => expect(address()).toBe('/datasource/recommended/rum?signal=logs'));
    expect(screen.getByLabelText<HTMLInputElement>('RUM application ID').value).toBe(
      'checkout-web',
    );
  });

  it('says when a category has nothing for the signal, and offers the way back', async () => {
    renderAt('/datasource/databases/postgres');
    await screen.findByRole('button', { name: 'Verify connection' });

    fireEvent.click(signal('Traces'));

    expect(await screen.findByText('No Traces sources in Databases.')).not.toBeNull();
    expect(address()).toBe('/datasource/databases?signal=traces');
    expect(rows()).toEqual([]);

    fireEvent.click(screen.getByRole('button', { name: 'Show all signals' }));

    await waitFor(() => expect(address()).toBe('/datasource/databases/postgres'));
  });

  it('searches every category and goes to what is picked', async () => {
    renderAt('/datasource/recommended/kubernetes');
    await screen.findByRole('button', { name: 'Verify connection' });

    fireEvent.change(search(), { target: { value: 'kafka' } });

    expect(rows()).toEqual(['kafka']);
    expect(screen.getByRole('link', { name: /Apache Kafka/ }).textContent).toContain('Messaging');
    // The guide stays where it was until a result is picked.
    expect(within(guide()).getByRole('heading', { level: 2, name: 'Kubernetes' })).not.toBeNull();

    fireEvent.keyDown(search(), { key: 'Enter' });

    await waitFor(() => expect(address()).toBe('/datasource/queues/kafka'));
    expect(search().value).toBe('');
    expect(rows()).toContain('rabbitmq');
  });

  it('lets a search result override a signal it does not send', async () => {
    renderAt('/datasource/applications/opentelemetry?signal=traces&stream=app_traces');
    await screen.findByRole('button', { name: 'Verify connection' });

    fireEvent.change(search(), { target: { value: 'postgres' } });

    expect(screen.getByRole('link', { name: /^PostgreSQL/ }).getAttribute('href')).toBe(
      '/datasource/databases/postgres',
    );
  });

  it('says when a search finds nothing and clears it', async () => {
    renderAt('/datasource/recommended/kubernetes');
    await screen.findByRole('button', { name: 'Verify connection' });

    fireEvent.change(search(), { target: { value: 'zzzz' } });

    expect(screen.getByText('No datasources match "zzzz"')).not.toBeNull();

    fireEvent.click(
      within(screen.getByRole('navigation', { name: 'Sources' })).getByRole('button', {
        name: 'Clear search',
      }),
    );

    expect(search().value).toBe('');
    expect(rows()[0]).toBe('kubernetes');
  });
});
