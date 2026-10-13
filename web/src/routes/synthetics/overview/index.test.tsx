import '@/i18n';

import { cleanup, render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { reduceMotionInTests } from '@/shell/readout/reduceMotionInTests';

import { NOW, checkRow, location, runs } from '../__tests__/builders';
import type { CheckRow } from '../data';
import { SyntheticsOverview } from './index';

const mocks = vi.hoisted(() => ({
  workspace: { current: {} as Record<string, unknown> },
  access: { current: { allowed: true, disabled: false } as Record<string, unknown> },
}));

vi.mock('../data', () => ({
  SYNTHETICS_REFRESH_MILLIS: 30_000,
  useSyntheticsWorkspace: () => mocks.workspace.current,
}));
vi.mock('@/product/actionAccess', () => ({
  useActionAccess: () => mocks.access.current,
}));
vi.mock('../map/WorldAvailabilityMap', () => ({
  WorldAvailabilityMap: () => <div data-testid="availability-map" />,
}));

function workspace(rows: CheckRow[], overrides: Record<string, unknown> = {}) {
  const results = rows.flatMap((row) => row.results);

  return {
    rows,
    operationalResults: results,
    locations: [
      location(),
      location({ id: 'loc-eu', name: 'Frankfurt', code: 'eu-central-1', health: 'degraded' }),
    ],
    pending: false,
    error: undefined,
    refetch: vi.fn(),
    ...overrides,
  };
}

function renderOverview() {
  return render(
    <MemoryRouter initialEntries={['/synthetics/overview']}>
      <SyntheticsOverview />
    </MemoryRouter>,
  );
}

let restoreMotion: () => void;

beforeEach(() => {
  restoreMotion = reduceMotionInTests();
  vi.useFakeTimers({ toFake: ['Date'], now: new Date(NOW / 1_000) });
  mocks.access.current = { allowed: true, disabled: false };
});

afterEach(() => {
  cleanup();
  restoreMotion();
  vi.useRealTimers();
});

describe('Synthetics overview', () => {
  it('opens with one statement of what the checks say', () => {
    mocks.workspace.current = workspace([
      checkRow({
        monitor: { id: 'signin', name: 'Sign-in journey', kind: 'browser', state: 'failing' },
        revision: { escalation_policy_id: 'policy-1' },
        results: runs(['failing', 'failing', 'healthy']),
      }),
      checkRow({ monitor: { id: 'api', name: 'Checkout API' }, results: runs(['healthy', 'healthy']) }),
      checkRow({ monitor: { id: 'vpn', name: 'Legacy VPN', lifecycle: 'paused', state: 'failing' } }),
    ]);
    const { container } = renderOverview();

    const statement = screen.getByRole('region', { name: 'Synthetic checks now' });
    expect(within(statement).getByRole('heading').textContent).toBe('1 check is failing');
    expect(within(statement).getByText('2 checks running · 1 paused')).toBeTruthy();
    expect(statement.getAttribute('data-checks-standing')).toBe('failing');
    // The figures the verdict rests on: runs, places, and whether anyone is told.
    expect(statement.textContent).toContain('60.0%');
    expect(statement.textContent).toContain('of the last 5 runs');
    expect(statement.textContent).toContain('1of 2');
    expect(statement.textContent).toContain('Frankfurt not online');
    expect(statement.textContent).toContain('checks with an escalation policy');

    // One module title, one action, and no number cards or refresh button.
    expect(screen.getByRole('heading', { level: 1 }).textContent).toBe('Synthetics');
    expect(screen.getAllByRole('button', { name: 'Create check' })).toHaveLength(1);
    expect(screen.queryByRole('button', { name: 'Refresh' })).toBeNull();
    expect(container.querySelectorAll('[data-statement]')).toHaveLength(1);
  });

  it('names the checks that need attention, with where and since when', () => {
    mocks.workspace.current = workspace([
      checkRow({
        monitor: { id: 'signin', name: 'Sign-in journey', kind: 'browser', state: 'failing' },
        results: runs(['failing', 'failing', 'healthy'], {
          location_id: 'loc-eu',
          attempts: [
            {
              number: 1,
              started_at: 0,
              finished_at: 1,
              outcome: 'failing',
              timing: {},
              error_message: 'Timed out waiting for the dashboard',
              metadata: {},
            },
          ],
        }),
      }),
      checkRow({ monitor: { id: 'api', name: 'Checkout API' }, results: runs(['healthy']) }),
    ]);
    renderOverview();

    const attention = screen.getByRole('region', { name: 'Needs attention' });
    const links = within(attention).getAllByRole('link');
    expect(links).toHaveLength(1);
    expect(links[0]?.getAttribute('href')).toBe('/synthetics/checks/signin');
    expect(links[0]?.textContent).toContain('Sign-in journey');
    expect(links[0]?.textContent).toContain('Browser · from Frankfurt · since 2 minutes ago');
    expect(links[0]?.textContent).toContain('Timed out waiting for the dashboard');
    // A check that passes is not in the list: the statement has counted it.
    expect(within(attention).queryByText('Checkout API')).toBeNull();
  });

  it('draws no list of trouble when every check passes', () => {
    mocks.workspace.current = workspace([
      checkRow({ monitor: { id: 'api' }, results: runs(['healthy', 'healthy']) }),
    ]);
    renderOverview();

    expect(
      within(screen.getByRole('region', { name: 'Synthetic checks now' })).getByRole('heading')
        .textContent,
    ).toBe('1 check, and it is passing');
    expect(screen.queryByRole('region', { name: 'Needs attention' })).toBeNull();
    expect(screen.getByTestId('availability-map')).toBeTruthy();
  });

  it('says how to begin when there is no check, with the one way to a first one', () => {
    mocks.workspace.current = workspace([]);
    renderOverview();

    const firstRun = screen.getByTestId('synthetics-first-run');
    expect(within(firstRun).getByRole('heading').textContent).toBe(
      'Check your services from the outside',
    );
    expect(screen.getAllByRole('button', { name: 'Create check' })).toHaveLength(1);
    expect(
      within(firstRun).getByRole('link', { name: 'Certificate validity' }).getAttribute('href'),
    ).toBe('/synthetics/checks/new?preset=tls');
    expect(screen.queryByRole('region', { name: 'Synthetic checks now' })).toBeNull();
    expect(screen.queryByTestId('availability-map')).toBeNull();
  });

  it('says the data could not be loaded, and offers to try again', () => {
    const refetch = vi.fn();
    mocks.workspace.current = workspace([], { error: new Error('boom'), refetch });
    renderOverview();

    expect(screen.getByText('Synthetic monitoring data could not be loaded.')).toBeTruthy();
    screen.getByRole('button', { name: 'Try again' }).click();
    expect(refetch).toHaveBeenCalled();
    expect(screen.queryByTestId('synthetics-first-run')).toBeNull();
  });
});
