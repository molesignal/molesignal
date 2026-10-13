import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { NOW, agent, location } from '../__tests__/builders';
import { Locations } from './index';

const mocks = vi.hoisted(() => ({
  listLocations: vi.fn(),
  listAgents: vi.fn(),
  access: { current: { allowed: true, disabled: false } as Record<string, unknown> },
}));

vi.mock('@/api/synthetics', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  listLocations: mocks.listLocations,
  listAgents: mocks.listAgents,
}));
vi.mock('@/product/actionAccess', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useActionAccess: () => mocks.access.current,
}));

function renderLocations(url = '/synthetics/locations') {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[url]}>
        <Routes>
          <Route path="/synthetics/locations" element={<Locations />} />
          <Route path="/synthetics/locations/:locationId" element={<Locations />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'], now: new Date(NOW / 1_000) });
  mocks.access.current = { allowed: true, disabled: false };
  mocks.listLocations.mockReset();
  mocks.listAgents.mockReset();
  mocks.listLocations.mockResolvedValue([
    location(),
    location({
      id: 'loc-ap',
      name: 'Singapore',
      code: 'ap-southeast-1',
      health: 'degraded',
      lifecycle: 'paused',
    }),
    location({
      id: 'loc-office',
      name: 'Office',
      code: 'office',
      scope: 'organization',
      health: 'offline',
      system_managed: false,
    }),
    location({ id: 'loc-core', name: 'Built-in', code: 'core', execution: 'embedded' }),
  ]);
  mocks.listAgents.mockImplementation((locationId: string) =>
    Promise.resolve(
      locationId === 'loc-us'
        ? [agent({ id: 'a', location_id: 'loc-us' }), agent({ id: 'b', location_id: 'loc-us' })]
        : locationId === 'loc-office'
          ? [agent({ id: 'c', location_id: 'loc-office', status: 'offline' })]
          : [],
    ),
  );
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe('Synthetics locations', () => {
  it('says whether each place can run checks, in the words for a place', async () => {
    renderLocations();

    const rows = (await screen.findAllByRole('row')).slice(1);
    expect(rows).toHaveLength(4);
    expect(within(rows[0]!).getByText('Online')).toBeTruthy();
    expect(within(rows[1]!).getByText('Degraded')).toBeTruthy();
    expect(within(rows[2]!).getByText('Offline')).toBeTruthy();
    // A location is online or not; "passed" is what a check does.
    expect(screen.queryByText('Passed')).toBeNull();
    expect(screen.queryByText('Failed')).toBeNull();
  });

  it('says what runs the checks there, and marks only a location that is paused', async () => {
    renderLocations();

    const rows = (await screen.findAllByRole('row')).slice(1);
    expect(rows[0]?.textContent).toContain('2 agents');
    expect(rows[0]?.textContent).toContain('Cloud');
    expect(rows[2]?.textContent).toContain('1 agent');
    expect(rows[2]?.textContent).toContain('Private');
    expect(rows[3]?.textContent).toContain('Built in');
    expect(within(rows[1]!).getByText('Paused')).toBeTruthy();
    expect(screen.getAllByText('Paused')).toHaveLength(1);
    // An embedded location has no agents to ask for.
    expect(mocks.listAgents).not.toHaveBeenCalledWith('loc-core');
  });

  it('stands in the module frame with the one action of the tab', async () => {
    renderLocations();

    await screen.findAllByRole('row');
    expect(screen.getByRole('heading', { level: 1 }).textContent).toBe('Synthetics');
    expect(screen.getAllByRole('button', { name: 'Add private location' })).toHaveLength(1);
    expect(screen.queryByRole('button', { name: 'Refresh' })).toBeNull();
  });

  it('opens a location from its address, with its agents and what it may reach', async () => {
    renderLocations('/synthetics/locations/loc-office');

    const drawer = await screen.findByRole('dialog');
    expect(within(drawer).getByText('Office')).toBeTruthy();
    expect(within(drawer).getByText('probe-a')).toBeTruthy();
    expect(within(drawer).getAllByText('Offline').length).toBeGreaterThan(0);
    expect(within(drawer).getByRole('button', { name: 'Register agent' })).toBeTruthy();
    expect(within(drawer).getByText('Public networks only')).toBeTruthy();
  });
});
