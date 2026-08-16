import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MemoryRouter, Route, Routes } from 'react-router-dom';

import type { SyntheticResult } from '@/api/synthetics';
import i18n from '@/i18n';

const mocks = vi.hoisted(() => ({
  listResultPage: vi.fn(),
  refetchWorkspace: vi.fn(),
}));

vi.mock('@/api/synthetics', () => ({
  listResultPage: mocks.listResultPage,
}));

vi.mock('./data', () => ({
  useSyntheticsWorkspace: () => ({
    rows: [
      {
        monitor: { id: 'check-1', name: 'Checkout API' },
        detail: undefined,
      },
    ],
    locations: [{ id: 'sin', name: 'Singapore' }],
    pending: false,
    refetching: false,
    error: undefined,
    refetch: mocks.refetchWorkspace,
  }),
}));

vi.mock('./components', async (importOriginal) => {
  const actual = (await importOriginal()) as Record<string, unknown>;
  return {
    ...actual,
    SyntheticsPage: ({ children }: { children: ReactNode }) => <div>{children}</div>,
    WorkspaceBoundary: ({ children }: { children: ReactNode }) => <>{children}</>,
    StatePill: ({ state }: { state: string }) => <span>{state}</span>,
  };
});

vi.mock('./ResultDrawer', () => ({ ResultDrawer: () => null }));

import { Results } from './Results';

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  mocks.listResultPage.mockReset();
  mocks.refetchWorkspace.mockReset();
  mocks.listResultPage.mockResolvedValue({
    items: [result()],
    total: 42,
    page: 1,
    per_page: 20,
  });
});

afterEach(cleanup);

describe('Synthetics Results', () => {
  it('renders a flat list surface, check search, and server-backed pagination', async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    const { container } = render(
      <QueryClientProvider client={queryClient}>
        <MemoryRouter initialEntries={['/synthetics/results']}>
          <Routes>
            <Route path="/synthetics/results" element={<Results />} />
          </Routes>
        </MemoryRouter>
      </QueryClientProvider>,
    );

    expect(await screen.findByText('Checkout API')).toBeTruthy();
    const surface = container.querySelector('[data-synthetics-list-surface]');
    expect(surface?.className).toContain('border-b');
    expect(surface?.className).not.toMatch(/rounded|shadow|border-x|border-y/);
    const filterBar = container.querySelector('[data-synthetics-filter-bar]');
    expect(filterBar?.className).toContain('border-b');
    expect(filterBar?.className).toContain('px-4');
    expect(filterBar?.className).not.toMatch(/rounded|shadow|lg:w-/);
    expect(
      screen.getByRole('navigation', { name: 'Synthetic result pagination' }),
    ).toBeTruthy();
    expect(screen.getByText('Page 1 / 3 · 42 total')).toBeTruthy();

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

function result(): SyntheticResult {
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
  };
}
