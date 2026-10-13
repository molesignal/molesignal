import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes, useLocation } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { NOW, checkRow, runs } from '../__tests__/builders';
import type { CheckRow } from '../data';
import { Checks } from './index';

const mocks = vi.hoisted(() => ({
  workspace: { current: {} as Record<string, unknown> },
  access: { current: { allowed: true, disabled: false } as Record<string, unknown> },
  runMonitor: vi.fn(),
  pauseMonitor: vi.fn(),
  resumeMonitor: vi.fn(),
  archiveMonitor: vi.fn(),
  getMonitor: vi.fn(),
}));

vi.mock('../data', () => ({
  SYNTHETICS_REFRESH_MILLIS: 30_000,
  useSyntheticsWorkspace: () => mocks.workspace.current,
}));
vi.mock('@/product/actionAccess', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useActionAccess: () => mocks.access.current,
}));
vi.mock('@/api/synthetics', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  runMonitor: mocks.runMonitor,
  pauseMonitor: mocks.pauseMonitor,
  resumeMonitor: mocks.resumeMonitor,
  archiveMonitor: mocks.archiveMonitor,
  getMonitor: mocks.getMonitor,
}));
// The editor has its own tests; here it would only pull its queries in.
vi.mock('../CheckEditor', () => ({
  CheckEditor: ({ open, mode }: { open: boolean; mode: string }) =>
    open ? <div data-testid="check-editor">{mode}</div> : null,
}));

const ROWS: CheckRow[] = [
  checkRow({
    monitor: { id: 'api', name: 'Checkout API', kind: 'http', tags: ['tier-1'] },
    results: runs(['healthy', 'healthy', 'failing']),
  }),
  checkRow({
    monitor: { id: 'signin', name: 'Sign-in journey', kind: 'browser', state: 'failing' },
    results: runs(['failing', 'failing']),
  }),
  checkRow({ monitor: { id: 'db', name: 'Postgres primary', kind: 'tcp' }, results: runs(['healthy']) }),
  checkRow({ monitor: { id: 'dns', name: 'Edge DNS', kind: 'dns', state: 'flaky' } }),
  checkRow({
    monitor: { id: 'vpn', name: 'Legacy VPN', kind: 'icmp', lifecycle: 'paused', state: 'failing' },
  }),
];

function Where() {
  const location = useLocation();
  return <p data-testid="where">{`${location.pathname}${location.search}`}</p>;
}

function renderChecks(url = '/synthetics/checks') {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[url]}>
        <Routes>
          <Route path="/synthetics/checks" element={<Checks />} />
          <Route path="/synthetics/checks/new" element={<Checks />} />
          <Route path="/synthetics/checks/:monitorId" element={<p>the check</p>} />
          <Route path="/synthetics/checks/:monitorId/edit" element={<Checks />} />
        </Routes>
        <Where />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

const names = () =>
  screen
    .getAllByRole('row')
    .slice(1)
    .map((row) => within(row).getAllByRole('cell')[0]?.textContent ?? '');

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['Date'], now: new Date(NOW / 1_000) });
  mocks.access.current = { allowed: true, disabled: false };
  mocks.workspace.current = {
    rows: ROWS,
    locations: [],
    pending: false,
    error: undefined,
    refetch: vi.fn(),
  };
  for (const mock of [mocks.runMonitor, mocks.pauseMonitor, mocks.resumeMonitor, mocks.archiveMonitor]) {
    mock.mockReset();
    mock.mockResolvedValue(undefined);
  }
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe('Synthetics checks', () => {
  it('lists every check with how it stands and how its latest runs went', () => {
    renderChecks();

    expect(screen.getByRole('heading', { level: 1 }).textContent).toBe('Synthetics');
    const rows = screen.getAllByRole('row').slice(1);
    expect(rows).toHaveLength(5);
    expect(within(rows[0]!).getByText('Passing')).toBeTruthy();
    expect(
      within(rows[0]!).getByRole('img', { name: '2 of the last 3 runs passed' }),
    ).toBeTruthy();
    expect(rows[0]?.textContent).toContain('1m · 2 locations');
    expect(within(rows[1]!).getByText('Failing')).toBeTruthy();
    // A check off schedule says so; its last state is not what it is doing now.
    expect(within(rows[4]!).getByText('Paused')).toBeTruthy();
    expect(within(rows[4]!).queryByText('Failing')).toBeNull();
    expect(within(rows[3]!).getByText('No runs yet')).toBeTruthy();
  });

  it('offers the kinds of check as views of the one list, with what each holds', () => {
    renderChecks();

    const views = screen.getByRole('group', { name: 'Kinds of check' });
    expect(within(views).getAllByRole('button')).toHaveLength(4);
    expect(within(views).getByRole('button', { name: /All/ }).textContent).toContain('5');
    expect(within(views).getByRole('button', { name: /Network/ }).textContent).toContain('3');
    // Nobody has a heartbeat check, so it is not a view worth offering.
    expect(within(views).queryByRole('button', { name: /Heartbeat/ })).toBeNull();

    fireEvent.click(within(views).getByRole('button', { name: /Network/ }));
    expect(names()).toEqual([
      expect.stringContaining('Postgres primary'),
      expect.stringContaining('Edge DNS'),
      expect.stringContaining('Legacy VPN'),
    ]);
    expect(screen.getByTestId('where').textContent).toBe('/synthetics/checks?kind=network');
  });

  it('reads the view, the state and the search from the address', () => {
    renderChecks('/synthetics/checks?kind=network&state=paused');
    expect(names()).toEqual([expect.stringContaining('Legacy VPN')]);

    cleanup();
    renderChecks('/synthetics/checks?state=failing');
    // The paused check last failed, but it is not failing now.
    expect(names()).toEqual([expect.stringContaining('Sign-in journey')]);

    cleanup();
    renderChecks('/synthetics/checks?query=tier-1');
    expect(names()).toEqual([expect.stringContaining('Checkout API')]);
  });

  it('runs a check from its row and keeps the rest behind one menu', async () => {
    vi.useRealTimers();
    const user = userEvent.setup();
    renderChecks();

    await user.click(screen.getByRole('button', { name: 'Run Checkout API now' }));
    expect(mocks.runMonitor).toHaveBeenCalledWith('api', expect.anything());
    // Neither opens the check: the row's own click is for that.
    expect(screen.getByTestId('where').textContent).toBe('/synthetics/checks');

    await user.click(screen.getByRole('button', { name: 'More actions for Legacy VPN' }));
    const menu = await screen.findByRole('menu');
    expect(
      within(menu)
        .getAllByRole('menuitem')
        .map((item) => item.textContent),
    ).toEqual(['Edit', 'Clone', 'Resume', 'Delete']);
    await user.click(within(menu).getByRole('menuitem', { name: 'Resume' }));
    expect(mocks.resumeMonitor).toHaveBeenCalledWith('vpn');
  });

  it('cannot run a paused check, and offers nothing to change without the permission', () => {
    renderChecks();
    expect(screen.getByRole('button', { name: 'Run Legacy VPN now' }).hasAttribute('disabled')).toBe(true);

    cleanup();
    mocks.access.current = { allowed: false, disabled: true, reason: 'Requires synthetics.manage.' };
    renderChecks();
    expect(screen.getByRole('button', { name: 'Run Checkout API now' }).hasAttribute('disabled')).toBe(true);
    expect(screen.getByRole('button', { name: 'Create check' }).hasAttribute('disabled')).toBe(true);
  });

  it('opens the editor at the addresses that ask for it', () => {
    renderChecks('/synthetics/checks/new');
    expect(screen.getByTestId('check-editor').textContent).toBe('create');
  });

  it('says how to begin when there is no check', () => {
    mocks.workspace.current = { ...mocks.workspace.current, rows: [] };
    renderChecks();

    expect(screen.getByTestId('synthetics-first-run')).toBeTruthy();
    expect(screen.queryByRole('group', { name: 'Kinds of check' })).toBeNull();
    expect(screen.getAllByRole('button', { name: 'Create check' })).toHaveLength(1);
  });
});
