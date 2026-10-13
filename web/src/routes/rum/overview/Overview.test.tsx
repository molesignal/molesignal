import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { OverviewParams, OverviewSummaryData } from '@/api/rum/overview';
import i18n from '@/i18n';
import { useAuthStore } from '@/stores/auth';
import { DEFAULT_WINDOW, useTimeStore } from '@/stores/useTimeStore';

const mocks = vi.hoisted(() => ({
  getOverview: vi.fn(),
  getOverviewInsights: vi.fn(),
}));

vi.mock('@/api/rum/overview', () => ({
  getOverview: mocks.getOverview,
  getOverviewInsights: mocks.getOverviewInsights,
}));

// A native select stands in for the shared one: what is offered and what is picked is the
// subject here, not the popover.
vi.mock('../RumLayout', async (importOriginal) => {
  const actual = (await importOriginal()) as Record<string, unknown>;
  return {
    ...actual,
    RumFilterSelect: ({
      label,
      value,
      options,
      onChange,
    }: {
      label: string;
      value: string;
      options: ReadonlyArray<{ value: string; label: string }>;
      onChange: (value: string) => void;
    }) => (
      <select
        aria-label={label}
        value={value}
        onChange={(event) => onChange(event.target.value)}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    ),
  };
});

import { Overview } from './Overview';

function overview(sessions: number): OverviewSummaryData {
  return {
    metrics: {
      users: sessions,
      sessions,
      errorFreeRate: 1,
      lcpP75: 1_800,
      inpP75: 120,
      clsP75: 0.04,
    },
    browserDevices: [],
    regions: [],
    facets: {
      applications: sessions > 0 ? ['storefront', 'admin'] : [],
      environments: sessions > 0 ? ['prod'] : [],
      versions: [],
      countries: [],
      devices: [],
    },
  };
}

function renderOverview() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter initialEntries={['/rum/overview']}>
        <Overview />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const scopeRow = () => document.querySelector('[data-rum-scope]');
const pulse = () => document.querySelector('[data-rum-pulse]');

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  mocks.getOverview.mockReset();
  mocks.getOverviewInsights.mockReset();
  // The analysis below the fold is not the subject; it stays loading.
  mocks.getOverviewInsights.mockReturnValue(new Promise(() => {}));
  useTimeStore.setState({ window: DEFAULT_WINDOW, anchor: null });
  useAuthStore.setState({
    ctx: { user_id: 'u1', org_id: 'acme', display_role: 'admin', roles: [] },
  });
});

afterEach(() => {
  cleanup();
  useAuthStore.setState({ ctx: null });
  useTimeStore.setState({ window: DEFAULT_WINDOW, anchor: null });
});

describe('RUM overview', () => {
  it('opens with the setup guide, and nothing to narrow, before the first session', async () => {
    mocks.getOverview.mockResolvedValue(overview(0));

    renderOverview();

    // Not while loading either: the row must not flash in and out.
    expect(scopeRow()).toBeNull();
    expect(await screen.findByTestId('rum-activation')).not.toBeNull();
    expect(scopeRow()).toBeNull();
    expect(pulse()).toBeNull();
  });

  it('offers the scope once sessions are arriving', async () => {
    mocks.getOverview.mockResolvedValue(overview(12));

    renderOverview();

    await waitFor(() => expect(pulse()).not.toBeNull());
    expect(scopeRow()).not.toBeNull();
    expect(
      Array.from(
        screen.getByLabelText<HTMLSelectElement>('Application').options,
        (option) => option.text,
      ),
    ).toEqual(['All applications', 'storefront', 'admin']);
    expect(screen.queryByTestId('rum-activation')).toBeNull();
  });

  it('says a scope matches nothing instead of asking to install the SDK, and keeps the way back', async () => {
    mocks.getOverview.mockImplementation((params: OverviewParams) =>
      Promise.resolve(overview(params.application === 'admin' ? 0 : 12)),
    );
    renderOverview();
    await waitFor(() => expect(pulse()).not.toBeNull());

    fireEvent.change(screen.getByLabelText('Application'), {
      target: { value: 'admin' },
    });

    expect(
      await screen.findByText('No sessions match these filters'),
    ).not.toBeNull();
    expect(screen.queryByTestId('rum-activation')).toBeNull();
    expect(pulse()).toBeNull();
    expect(screen.getByLabelText<HTMLSelectElement>('Application').value).toBe(
      'admin',
    );

    fireEvent.click(screen.getByRole('button', { name: 'Clear filters' }));

    await waitFor(() => expect(pulse()).not.toBeNull());
    expect(screen.getByLabelText<HTMLSelectElement>('Application').value).toBe(
      '__all__',
    );
    expect(screen.queryByText('No sessions match these filters')).toBeNull();
  });

  it('still shows a scope in force when the range no longer lists its value', async () => {
    mocks.getOverview.mockResolvedValue(overview(12));
    renderOverview();
    await waitFor(() => expect(pulse()).not.toBeNull());
    fireEvent.change(screen.getByLabelText('Application'), {
      target: { value: 'storefront' },
    });
    await waitFor(() =>
      expect(mocks.getOverview).toHaveBeenCalledWith(
        expect.objectContaining({ application: 'storefront' }),
      ),
    );

    // The next range has sessions, but none of the application in force.
    mocks.getOverview.mockImplementation((params: OverviewParams) =>
      Promise.resolve(
        params.application
          ? overview(0)
          : {
              ...overview(12),
              facets: { ...overview(12).facets, applications: ['admin'] },
            },
      ),
    );
    act(() => {
      useTimeStore.getState().setWindow({
        mode: 'absolute',
        from: '2026-05-23T08:00:00.000Z',
        to: '2026-05-23T09:00:00.000Z',
      });
    });

    await screen.findByText('No sessions match these filters');
    const application = screen.getByLabelText<HTMLSelectElement>('Application');
    expect(Array.from(application.options, (option) => option.text)).toEqual([
      'All applications',
      'storefront',
      'admin',
    ]);
    expect(application.value).toBe('storefront');
  });

  it('keeps the scope row in place while another time range loads', async () => {
    mocks.getOverview.mockResolvedValue(overview(12));
    renderOverview();
    await waitFor(() => expect(pulse()).not.toBeNull());

    mocks.getOverview.mockReturnValue(new Promise(() => {}));
    act(() => {
      useTimeStore.getState().setWindow({
        mode: 'absolute',
        from: '2026-05-23T08:00:00.000Z',
        to: '2026-05-23T09:00:00.000Z',
      });
    });

    await waitFor(() => expect(pulse()).toBeNull());
    expect(scopeRow()).not.toBeNull();
  });

  it('withdraws the scope row when a range turns out to hold no session at all', async () => {
    mocks.getOverview.mockResolvedValue(overview(12));
    renderOverview();
    await waitFor(() => expect(pulse()).not.toBeNull());

    mocks.getOverview.mockResolvedValue(overview(0));
    act(() => {
      useTimeStore.getState().setWindow({
        mode: 'absolute',
        from: '2026-05-23T08:00:00.000Z',
        to: '2026-05-23T09:00:00.000Z',
      });
    });

    expect(await screen.findByTestId('rum-activation')).not.toBeNull();
    expect(scopeRow()).toBeNull();
  });
});
