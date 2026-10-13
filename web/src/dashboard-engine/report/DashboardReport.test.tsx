import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import { AxiosError, type AxiosResponse } from 'axios';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type * as dashboardsApi from '@/api/dashboards';
import { setActiveTimePreference } from '@/lib/time';
import { useAuthStore } from '@/stores/auth';
import { DEFAULT_WINDOW, useTimeStore } from '@/stores/useTimeStore';
import type { Dashboard } from '@/types/dashboard';

import type * as dataSources from '../dataSources';
import { createDashboardPanel } from '../factories';
import { createEmptyDashboardDefinition, dashboardDefinitionToModel } from '../model';
import type { DashboardDefinition, DataFrame } from '../schema';
import { DashboardReport } from './DashboardReport';

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  executePanelQuery: vi.fn(),
}));

vi.mock('@/api/dashboards', async (importOriginal) => ({
  ...(await importOriginal<typeof dashboardsApi>()),
  get: mocks.get,
}));

vi.mock('../dataSources', async (importOriginal) => ({
  ...(await importOriginal<typeof dataSources>()),
  executePanelQuery: mocks.executePanelQuery,
}));

vi.mock('@/product/access', () => ({
  useProductAccess: () => null,
  canAccessProductPath: () => false,
}));

const root = document.documentElement;

function stored(definition: DashboardDefinition): Dashboard {
  return {
    id: 'd1',
    org_id: 'org-1',
    uid: definition.uid,
    title: definition.title,
    tags: [],
    model: dashboardDefinitionToModel(definition),
    version: 1,
    created_at: 0,
    updated_at: 0,
  };
}

function definition(): DashboardDefinition {
  const dashboard = createEmptyDashboardDefinition('Checkout health');
  dashboard.description = 'How the checkout path is doing.';
  // On screen this dashboard refreshes itself; a report must not.
  dashboard.refreshSettings = {
    enabled: true,
    mode: 'interval',
    defaultInterval: '5s',
    allowedIntervals: ['off', '5s'],
  };
  dashboard.variables = [
    {
      id: 'variable-env',
      name: 'env',
      label: 'Environment',
      type: 'custom',
      options: [
        { label: 'staging', value: 'staging' },
        { label: 'production', value: 'production', selected: true },
      ],
      multi: false,
      includeAll: false,
      hide: 'none',
      refresh: 'never',
    },
    {
      id: 'variable-region',
      name: 'region',
      label: 'Region',
      type: 'custom',
      options: [
        { label: 'us-east-1', value: 'us-east-1' },
        { label: 'eu-west-1', value: 'eu-west-1' },
      ],
      currentValue: ['us-east-1', 'eu-west-1'],
      multi: true,
      includeAll: false,
      hide: 'none',
      refresh: 'never',
    },
    {
      id: 'variable-secret',
      name: 'cluster',
      label: 'Cluster',
      type: 'constant',
      defaultValue: 'c-17',
      multi: false,
      includeAll: false,
      hide: 'none',
      refresh: 'never',
    },
    {
      id: 'variable-hidden',
      name: 'tenant',
      label: 'Tenant',
      type: 'custom',
      options: [{ label: 'acme', value: 'acme', selected: true }],
      multi: false,
      includeAll: false,
      hide: 'variable',
      refresh: 'never',
    },
  ];
  const panel = createDashboardPanel([], 'stat');
  panel.id = 'panel-requests';
  panel.title = 'Requests';
  panel.description = 'Five-minute request rate.';
  dashboard.elements = [panel];

  return dashboard;
}

function frame(): DataFrame {
  return {
    refId: 'A',
    length: 2,
    fields: [
      { id: 'time', name: 'time', type: 'time', values: [1_700_000_000, 1_700_000_010] },
      { id: 'value', name: 'value', type: 'number', values: [12, 42] },
    ],
  };
}

function renderReport(url = '/dashboards/d1?report_render=1') {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });

  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[url]}>
        <Routes>
          <Route path="/dashboards/:id" element={<DashboardReport />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

function facts(): Record<string, string> {
  // The report's own header: each printed panel has a header too.
  const header = screen.getByRole('heading', { level: 1 }).closest('header');
  if (!header) throw new Error('The report has no header');
  const terms = within(header).getAllByRole('term');
  const definitions = within(header).getAllByRole('definition');

  return Object.fromEntries(
    terms.map((term, index) => [term.textContent ?? '', definitions[index]?.textContent ?? '']),
  );
}

beforeEach(() => {
  useAuthStore.setState({
    token: 'token',
    ctx: { user_id: 'user-1', org_id: 'org-1', display_role: 'Owner', roles: [] },
  });
  setActiveTimePreference('Asia/Shanghai', 'iso_24h');
  mocks.get.mockResolvedValue(stored(definition()));
  mocks.executePanelQuery.mockResolvedValue([frame()]);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  mocks.get.mockReset();
  mocks.executePanelQuery.mockReset();
  setActiveTimePreference('', 'iso_24h');
  useTimeStore.setState({ window: DEFAULT_WINDOW });
  useAuthStore.setState({ token: null, ctx: null });
});

describe('DashboardReport', () => {
  it('prints the dashboard under what it is and what it covers', async () => {
    vi.useFakeTimers({ toFake: ['Date'], now: new Date('2026-10-11T01:30:00Z') });
    useTimeStore.setState({
      window: { from: '2026-10-03T16:00:00Z', to: '2026-10-10T16:00:00Z', mode: 'absolute' },
    });
    renderReport();

    expect(await screen.findByRole('heading', { level: 1, name: 'Checkout health' })).toBeTruthy();
    expect(screen.getByText('How the checkout path is doing.')).toBeTruthy();
    expect(facts()).toEqual({
      'Time range': '2026-10-04 00:00 – 2026-10-11 00:00',
      Timezone: 'Asia/Shanghai · UTC+8',
      // The variables a reader would see, with the values the panels were drawn for.
      Environment: 'production',
      Region: 'us-east-1, eu-west-1',
      Generated: '2026-10-11 09:30',
    });
    expect(await screen.findByRole('heading', { level: 3, name: 'Requests' })).toBeTruthy();
  });

  it('carries none of the page a person works in', async () => {
    const { container } = renderReport();

    await screen.findByRole('heading', { level: 3, name: 'Requests' });
    expect(screen.queryAllByRole('button')).toEqual([]);
    expect(screen.queryAllByRole('combobox')).toEqual([]);
    expect(screen.queryAllByRole('link')).toEqual([]);
    expect(screen.queryAllByRole('navigation')).toEqual([]);
    expect(container.querySelector('select, input, [data-page-header]')).toBeNull();
  });

  it('tells the renderer it is ready only once the panels have their data', async () => {
    let answer: (frames: DataFrame[]) => void = () => undefined;
    mocks.executePanelQuery.mockReturnValue(
      new Promise<DataFrame[]>((resolve) => {
        answer = resolve;
      }),
    );
    const { container } = renderReport();

    await screen.findByRole('heading', { level: 3, name: 'Requests' });
    await waitFor(() => expect(mocks.executePanelQuery).toHaveBeenCalled());
    // Long enough for the renderer's "nothing is loading" window to have passed, were it open.
    await new Promise((resolve) => setTimeout(resolve, 400));
    expect(root.dataset.reportRenderState).toBe('loading');

    answer([frame()]);
    await waitFor(() => expect(root.dataset.reportRenderState).toBe('ready'), { timeout: 3_000 });
    expect(container.querySelector('[data-printed-panel]')?.textContent).toContain('42');
    // Printed once: the dashboard's own 5s refresh is off, so the query ran a single time.
    expect(mocks.executePanelQuery).toHaveBeenCalledTimes(1);
  });

  it('fails the report with the reason when the dashboard cannot be loaded', async () => {
    mocks.get.mockRejectedValue(
      new AxiosError('Request failed with status code 404', 'ERR_BAD_REQUEST', undefined, null, {
        status: 404,
        data: { message: 'dashboard d1 not found' },
      } as AxiosResponse),
    );
    renderReport();

    expect((await screen.findByRole('alert')).textContent).toBe('dashboard d1 not found');
    expect(root.dataset.reportRenderState).toBe('error');
    expect(root.dataset.reportRenderError).toBe('dashboard d1 not found');
    expect(screen.queryByRole('heading', { level: 1 })).toBeNull();
  });

  it('fails the report when the stored model cannot be read', async () => {
    const record = stored(definition());
    // A link without its required fields: the engine refuses the model instead of guessing.
    mocks.get.mockResolvedValue({
      ...record,
      model: { ...record.model, links: [{ id: 'link-1', title: 'Runbook' }] },
    });
    renderReport();

    const reason = await screen.findByRole('alert');
    expect(reason.textContent).toContain('/links/0');
    expect(root.dataset.reportRenderState).toBe('error');
    expect(root.dataset.reportRenderError).toBe(reason.textContent);
  });

  it('leaves the time range out for a dashboard that does not read it', async () => {
    const timeless = definition();
    timeless.timeSettings = { ...timeless.timeSettings, hideTimePicker: true };
    mocks.get.mockResolvedValue(stored(timeless));
    renderReport();

    await screen.findByRole('heading', { level: 1, name: 'Checkout health' });
    expect(Object.keys(facts())).toEqual(['Timezone', 'Environment', 'Region', 'Generated']);
  });
});
