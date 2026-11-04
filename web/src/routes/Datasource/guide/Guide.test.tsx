import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { HomeOverview } from '@/api/home';
import i18n from '@/i18n';
import { TooltipProvider } from '@/shell/ui/tooltip';
import { useAuthStore } from '@/stores/auth';

const mocks = vi.hoisted(() => ({
  getDefault: vi.fn(),
  getRumClient: vi.fn(),
  instance: vi.fn(),
  listApplications: vi.fn(),
  recentReceipt: vi.fn(),
}));

vi.mock('@/api/apiTokens', () => ({
  getDefault: mocks.getDefault,
  getRumClient: mocks.getRumClient,
  scopes: async () => ({ policies: [], permissions: [] }),
}));
vi.mock('@/api/instance', () => ({ get: mocks.instance }));
vi.mock('@/api/rum', () => ({ listApplications: mocks.listApplications }));
vi.mock('@/api/rumIntake', () => ({
  recentReceipt: mocks.recentReceipt,
  sendTestEvents: vi.fn(),
}));
vi.mock('../codeBlock/highlighter', () => ({
  highlightCode: vi.fn(() => new Promise(() => {})),
}));

import { SOURCES, type Source } from '../sources';
import { Guide } from './Guide';

const NOW = Date.now() * 1000;

function overview(logs: { rows: number; status?: 'healthy' | 'delayed' }): HomeOverview {
  return {
    signals: [
      {
        streamType: 'logs',
        status: logs.rows > 0 ? (logs.status ?? 'healthy') : 'no_data',
        totalStreams: 1,
        activeStreams: logs.rows > 0 ? 1 : 0,
        rows: logs.rows,
        storedBytes: logs.rows * 40,
        lastReceivedAt: logs.rows > 0 ? NOW - 12_000_000 : null,
      },
    ],
    streams:
      logs.rows > 0
        ? [
            {
              id: 'logs/default',
              name: 'default',
              streamType: 'logs',
              status: 'healthy',
              rows: logs.rows,
              storedBytes: logs.rows * 40,
              firstReceivedAt: NOW - 86_400_000_000,
              lastReceivedAt: NOW - 12_000_000,
            },
          ]
        : [],
  } as unknown as HomeOverview;
}

function source(id: string): Source {
  return SOURCES.find((item) => item.id === id)!;
}

function renderGuide({
  id = 'kubernetes',
  passive,
  refreshed = overview({ rows: 0 }),
  search = '',
}: {
  id?: string;
  passive?: HomeOverview;
  refreshed?: HomeOverview | Error;
  /** The query string the guide is opened with. */
  search?: string;
} = {}) {
  const refreshOverview = vi.fn(() =>
    refreshed instanceof Error ? Promise.reject(refreshed) : Promise.resolve(refreshed),
  );
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  render(
    <QueryClientProvider client={client}>
      <TooltipProvider delayDuration={0}>
        <MemoryRouter initialEntries={[`/${search}`]}>
          <Guide
            source={source(id)}
            signal={undefined}
            streamName="default"
            overview={passive}
            refreshOverview={refreshOverview}
          />
        </MemoryRouter>
      </TooltipProvider>
    </QueryClientProvider>,
  );

  return { refreshOverview };
}

const states = () =>
  Array.from(document.querySelectorAll('[data-guide-section]'), (section) =>
    section.getAttribute('data-state'),
  );
const outcome = () =>
  document.querySelector('[data-verification]')?.getAttribute('data-verification');
const facts = () =>
  Array.from(document.querySelectorAll('[data-verification-facts] dt'), (fact) => fact.textContent);
const verify = () => screen.getByRole('button', { name: 'Verify connection' });

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  for (const mock of Object.values(mocks)) mock.mockReset();
  mocks.getDefault.mockResolvedValue({
    token: 'mspat-test0123456789secret',
    policyId: 'builtin:data_intake',
    tokenKind: 'default_intake',
  });
  mocks.getRumClient.mockResolvedValue({
    token: 'mspat-rum0123456789secret',
    permissions: ['rum.write'],
    tokenKind: 'rum_client',
  });
  mocks.instance.mockResolvedValue({ externalUrl: 'https://obs.example.com' });
  useAuthStore.setState({
    ctx: { userId: 'u1', orgId: 'acme', orgName: 'Acme', displayRole: 'admin', roles: [] },
  });
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => ({ ok: true, status: 200 })),
  );
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  useAuthStore.setState({ ctx: null });
});

describe('Datasource guide', () => {
  it('starts with the endpoint and token ready and the rest to do', async () => {
    renderGuide();

    await waitFor(() => expect(states()).toEqual(['done', 'current', 'upcoming']));
    expect(outcome()).toBe('idle');
    // Nothing is claimed about receipt before there is something to show.
    expect(facts()).toEqual([]);
  });

  it('shows what the workspace already receives before anything is checked', async () => {
    renderGuide({ passive: overview({ rows: 48_210 }) });

    await waitFor(() => expect(states()[0]).toBe('done'));
    expect(outcome()).toBe('idle');
    expect(facts()).toEqual(['Last received', 'Active streams', 'Last hour']);
    expect(document.querySelector('[data-verification-facts]')?.textContent).toContain(
      '48.2K rows',
    );
  });

  it('finishes the guide when the check finds data, and points onward', async () => {
    const { refreshOverview } = renderGuide({ refreshed: overview({ rows: 1_200 }) });
    await waitFor(() => expect(states()[0]).toBe('done'));

    fireEvent.click(verify());

    await waitFor(() => expect(outcome()).toBe('success'));
    expect(refreshOverview).toHaveBeenCalledTimes(1);
    expect(states()).toEqual(['done', 'done', 'done']);
    expect(facts()).toEqual(['Receiver', 'Last received', 'Active streams', 'Last hour']);
    expect(screen.getByText('Data detected')).not.toBeNull();
    expect(screen.getByRole('button', { name: 'View received data' })).not.toBeNull();
    expect(screen.getByRole('button', { name: 'Create pipeline' })).not.toBeNull();
    // The check can be repeated, but it is no longer what there is to do.
    expect(screen.queryByRole('button', { name: 'Verify connection' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Check again' })).not.toBeNull();
  });

  it('says what may be wrong when the receiver answers but nothing has arrived', async () => {
    renderGuide({ refreshed: overview({ rows: 0 }) });
    await waitFor(() => expect(states()[0]).toBe('done'));

    fireEvent.click(verify());

    await waitFor(() => expect(outcome()).toBe('warning'));
    expect(states()).toEqual(['done', 'current', 'attention']);
    expect(facts()).toEqual(['Receiver']);
    const causes = within(document.querySelector<HTMLElement>('[data-verification-causes]')!);
    expect(causes.getAllByRole('listitem')).toHaveLength(3);
    expect(causes.getByRole('button', { name: 'Send test event' })).not.toBeNull();
    expect(screen.queryByRole('button', { name: 'View received data' })).toBeNull();
  });

  it('treats data that arrives late as not yet verified', async () => {
    renderGuide({ refreshed: overview({ rows: 900, status: 'delayed' }) });
    await waitFor(() => expect(states()[0]).toBe('done'));

    fireEvent.click(verify());

    await waitFor(() => expect(outcome()).toBe('warning'));
    expect(facts()).toEqual(['Receiver', 'Last received', 'Active streams', 'Last hour']);
  });

  it('reports a receiver that does not answer', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => ({ ok: false, status: 503 })),
    );
    renderGuide({ refreshed: overview({ rows: 1_200 }) });
    await waitFor(() => expect(states()[0]).toBe('done'));

    fireEvent.click(verify());

    await waitFor(() => expect(outcome()).toBe('error'));
    expect(screen.getByText('HTTP 503')).not.toBeNull();
    expect(states()[2]).toBe('attention');
    expect(facts()).toEqual([]);
  });

  it('reports a check that could not read what was received', async () => {
    renderGuide({ refreshed: new Error('overview unavailable') });
    await waitFor(() => expect(states()[0]).toBe('done'));

    fireEvent.click(verify());

    await waitFor(() => expect(outcome()).toBe('error'));
    expect(screen.getByText('overview unavailable')).not.toBeNull();
  });

  describe('of a RUM source', () => {
    const application = (id: string, name: string) => ({
      id,
      name,
      platform: 'browser',
      settings: { collectIp: true, collectGeo: true, defaultPrivacy: 'mask', issuesPerHour: 100, scrub: ['secrets', 'emails', 'phones', 'cards', 'national_ids', 'url_params'], sessionSampleRate: 100 },
      createdBy: null,
      createdAt: NOW,
      updatedAt: NOW,
    });
    const web = application('checkout-web', 'Checkout web');
    const app = application('checkout-app', 'Checkout app');
    // The choice is drawn once the applications are known: until then it is a disabled stand-in.
    const choice = async () => {
      await waitFor(() => expect(screen.getByRole('combobox', { name: 'RUM application' })).toBeEnabled());
      return screen.getByRole('combobox', { name: 'RUM application' });
    };

    it('asks for the application before it shows a token or verifies, and verifies what it sent', async () => {
      mocks.listApplications.mockResolvedValue([web, app]);
      mocks.recentReceipt.mockResolvedValue({
        rows: 3,
        lastReceivedAt: NOW - 5_000_000,
        streams: ['rum_views'],
      });
      renderGuide({ id: 'rum' });

      // Two to choose from: none is taken for the reader.
      const select = await choice();
      expect(states()).toEqual(['current', 'upcoming', 'upcoming']);
      expect(
        screen.getByText('Choose a RUM application first: its client token is shown once it is chosen.'),
      ).not.toBeNull();
      expect(verify()).toBeDisabled();
      expect(mocks.getRumClient).not.toHaveBeenCalled();

      fireEvent.keyDown(select, { key: 'Enter' });
      fireEvent.click(screen.getByRole('option', { name: 'Checkout app · checkout-app' }));

      await waitFor(() => expect(states()[0]).toBe('done'));
      expect(mocks.getRumClient).toHaveBeenCalledWith('checkout-app');
      expect(document.querySelector('[data-datasource-guide]')?.textContent).toContain(
        "applicationId: 'checkout-app'",
      );
      // The events of an SDK go to the one entry, whatever their kind.
      expect(screen.getByText('https://obs.example.com/api/v1/rum/events')).not.toBeNull();

      fireEvent.click(verify());

      await waitFor(() => expect(outcome()).toBe('success'));
      expect(mocks.recentReceipt).toHaveBeenCalledWith({ applicationId: 'checkout-app' });
    });

    it('takes the only application there is', async () => {
      mocks.listApplications.mockResolvedValue([web]);
      renderGuide({ id: 'rum' });

      await waitFor(() => expect(states()[0]).toBe('done'));
      expect(mocks.getRumClient).toHaveBeenCalledWith('checkout-web');
    });

    it('opens on the application a link names, and drops one that is gone', async () => {
      mocks.listApplications.mockResolvedValue([web, app]);
      renderGuide({ id: 'rum', search: '?app=checkout-app' });

      await waitFor(() => expect(states()[0]).toBe('done'));
      expect(mocks.getRumClient).toHaveBeenCalledWith('checkout-app');
      cleanup();

      mocks.getRumClient.mockClear();
      renderGuide({ id: 'rum', search: '?app=deleted-long-ago' });

      await choice();
      expect(states()[0]).toBe('current');
      expect(mocks.getRumClient).not.toHaveBeenCalled();
    });

    it('sends the reader to make an application when the organization has none', async () => {
      mocks.listApplications.mockResolvedValue([]);
      renderGuide({ id: 'rum' });

      const create = await screen.findByRole('link', { name: 'Create an application' });
      expect(create.getAttribute('href')).toBe('/rum/applications?create=1');
      expect(screen.getByText(/There is no RUM application yet\./)).not.toBeNull();
      expect(mocks.getRumClient).not.toHaveBeenCalled();
      expect(verify()).toBeDisabled();
    });
  });

  it('has no test event to offer for a source that only sends profiles', async () => {
    renderGuide({ id: 'continuous-profiling', refreshed: overview({ rows: 0 }) });
    await waitFor(() => expect(states()[0]).toBe('done'));

    fireEvent.click(verify());

    await waitFor(() => expect(outcome()).toBe('warning'));
    expect(screen.queryByRole('button', { name: 'Send test event' })).toBeNull();
  });

  it('links to the documentation of a source that has some', async () => {
    renderGuide({ id: 'rum-flutter' });

    expect(screen.getByRole('link', { name: 'Docs' }).getAttribute('href')).toBe(
      'https://docs.molesignal.com/en-US/rum/flutter-sdk',
    );
    cleanup();

    renderGuide();
    expect(screen.queryByRole('link', { name: 'Docs' })).toBeNull();
  });
});
