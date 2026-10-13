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
  recentErrorSummary: vi.fn(),
}));

vi.mock('@/api/apiTokens', () => ({
  getDefault: mocks.getDefault,
  getRumClient: mocks.getRumClient,
}));
vi.mock('@/api/instance', () => ({ get: mocks.instance }));
vi.mock('@/api/rumIntake', () => ({
  recentErrorSummary: mocks.recentErrorSummary,
  sendTestError: vi.fn(),
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
        stream_type: 'logs',
        status: logs.rows > 0 ? (logs.status ?? 'healthy') : 'no_data',
        total_streams: 1,
        active_streams: logs.rows > 0 ? 1 : 0,
        rows: logs.rows,
        stored_bytes: logs.rows * 40,
        last_received_at_micros: logs.rows > 0 ? NOW - 12_000_000 : null,
      },
    ],
    streams:
      logs.rows > 0
        ? [
            {
              id: 'logs/default',
              name: 'default',
              stream_type: 'logs',
              status: 'healthy',
              rows: logs.rows,
              stored_bytes: logs.rows * 40,
              first_received_at_micros: NOW - 86_400_000_000,
              last_received_at_micros: NOW - 12_000_000,
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
}: {
  id?: string;
  passive?: HomeOverview;
  refreshed?: HomeOverview | Error;
} = {}) {
  const refreshOverview = vi.fn(() =>
    refreshed instanceof Error ? Promise.reject(refreshed) : Promise.resolve(refreshed),
  );
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  render(
    <QueryClientProvider client={client}>
      <TooltipProvider delayDuration={0}>
        <MemoryRouter>
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
    token: 'ms_test_0123456789_secret',
    role_name: 'Intake token',
    token_kind: 'default_intake',
    expires_at_micros: null,
  });
  mocks.getRumClient.mockResolvedValue({
    token: 'ms_rum_0123456789_secret',
    role_name: 'RUM client',
    token_kind: 'rum_client',
    expires_at_micros: null,
  });
  mocks.instance.mockResolvedValue({ external_url: 'https://obs.example.com' });
  useAuthStore.setState({
    ctx: { user_id: 'u1', org_id: 'acme', org_name: 'Acme', display_role: 'admin', roles: [] },
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

  it('asks a RUM source for its application before it issues a token or verifies', async () => {
    mocks.recentErrorSummary.mockResolvedValue({ rows: 3, lastReceivedAtMicros: NOW - 5_000_000 });
    renderGuide({ id: 'rum' });

    expect(states()).toEqual(['current', 'upcoming', 'upcoming']);
    expect(
      screen.getByText('Enter a valid RUM application ID before an app-bound token can be issued.'),
    ).not.toBeNull();
    expect(verify()).toBeDisabled();
    expect(mocks.getRumClient).not.toHaveBeenCalled();

    fireEvent.change(screen.getByLabelText('RUM application ID'), {
      target: { value: 'checkout-web' },
    });
    // Typing alone asks for nothing; the application is confirmed first.
    expect(mocks.getRumClient).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Use this application' }));

    await waitFor(() => expect(states()[0]).toBe('done'));
    expect(mocks.getRumClient).toHaveBeenCalledWith('checkout-web');
    expect(document.querySelector('[data-datasource-guide]')?.textContent).toContain(
      "applicationId: 'checkout-web'",
    );

    fireEvent.click(verify());

    await waitFor(() => expect(outcome()).toBe('success'));
    expect(mocks.recentErrorSummary).toHaveBeenCalledWith({
      orgId: 'acme',
      applicationId: 'checkout-web',
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
