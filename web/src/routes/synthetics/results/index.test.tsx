import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { SyntheticResult } from '@/api/synthetics';
import i18n from '@/i18n';

const mocks = vi.hoisted(() => ({
  listResultPage: vi.fn(),
  refetchWorkspace: vi.fn(),
}));

vi.mock('@/api/synthetics', () => ({
  listResultPage: mocks.listResultPage,
}));

vi.mock('../data', () => ({
  SYNTHETICS_REFRESH_MILLIS: 30_000,
  useSyntheticsWorkspace: () => ({
    rows: [{ monitor: { id: 'check-1', name: 'Checkout API' }, detail: undefined }],
    locations: [{ id: 'sin', name: 'Singapore' }],
    pending: false,
    refetching: false,
    error: undefined,
    refetch: mocks.refetchWorkspace,
  }),
}));

vi.mock('./Drawer', () => ({ ResultDrawer: () => null }));

import { Results } from './index';

function renderResults(url = '/synthetics/results') {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[url]}>
        <Routes>
          <Route path="/synthetics/results" element={<Results />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  mocks.listResultPage.mockReset();
  mocks.refetchWorkspace.mockReset();
  mocks.listResultPage.mockResolvedValue({
    items: [result(), result({ id: 'result-2', outcome: 'failing', is_test: true })],
    total: 42,
    page: 1,
    per_page: 20,
  });
});

afterEach(cleanup);

describe('Synthetics results', () => {
  it('lists the runs under the module title, with the filters as the head of the list', async () => {
    const { container } = renderResults();

    expect(await screen.findAllByText('Checkout API')).toHaveLength(2);
    // The module has one name; which tab is open is said by the tabs.
    expect(screen.getByRole('heading', { level: 1 }).textContent).toBe('Synthetics');
    expect(screen.queryByRole('button', { name: 'Refresh' })).toBeNull();

    const list = container.querySelector('[data-list-surface]');
    if (!list) throw new Error('The runs are not on a list surface');
    expect(within(list as HTMLElement).getByPlaceholderText('Search check name or ID…')).toBeTruthy();
    expect(within(list as HTMLElement).getByRole('combobox', { name: 'Outcome' })).toBeTruthy();
    expect(within(list as HTMLElement).getByRole('combobox', { name: 'Location' })).toBeTruthy();
    expect(
      within(list as HTMLElement).getByRole('navigation', { name: 'Synthetic result pagination' }),
    ).toBeTruthy();
    expect(screen.getByText('Page 1 / 3 · 42 total')).toBeTruthy();
  });

  it('says how each run went, and marks only the runs a person asked for', async () => {
    renderResults();

    const rows = (await screen.findAllByRole('row')).slice(1);
    expect(rows).toHaveLength(2);
    expect(within(rows[0]!).getByText('Passed')).toBeTruthy();
    expect(within(rows[0]!).queryByText('Test run')).toBeNull();
    expect(within(rows[1]!).getByText('Failed')).toBeTruthy();
    expect(within(rows[1]!).getByText('Test run')).toBeTruthy();
    expect(within(rows[1]!).getByText('Singapore')).toBeTruthy();
  });

  it('searches on the server and starts again from the first page', async () => {
    renderResults('/synthetics/results?page=2');

    await screen.findAllByText('Checkout API');
    fireEvent.change(screen.getByPlaceholderText('Search check name or ID…'), {
      target: { value: 'checkout' },
    });

    await waitFor(() => {
      expect(mocks.listResultPage).toHaveBeenLastCalledWith(
        expect.objectContaining({ query: 'checkout', page: 1, per_page: 20 }),
      );
    });
  });
});

function result(overrides: Partial<SyntheticResult> = {}): SyntheticResult {
  return {
    id: 'result-1',
    organization_id: 'org-1',
    monitor_id: 'check-1',
    monitor_revision_id: 'revision-1',
    location_id: 'sin',
    task_id: 'task-1',
    is_test: false,
    scheduled_at: 1,
    started_at: 2,
    finished_at: 3,
    received_at: 4,
    outcome: 'healthy',
    attempts: [],
    assertions: [],
    secret_versions: {},
    protocol_version: 1,
    metadata: {},
    ...overrides,
  };
}
