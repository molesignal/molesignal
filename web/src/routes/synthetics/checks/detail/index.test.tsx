import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { MonitorDetail, SyntheticResult } from '@/api/synthetics';
import { reduceMotionInTests } from '@/shell/readout/reduceMotionInTests';

import { MINUTE, NOW, location, monitor, result, revision, runs } from '../../__tests__/builders';
import { CheckDetail } from './index';

const mocks = vi.hoisted(() => ({
  getMonitor: vi.fn(),
  listResults: vi.fn(),
  listLocations: vi.fn(),
  runMonitor: vi.fn(),
  testRevision: vi.fn(),
  publishRevision: vi.fn(),
  pauseMonitor: vi.fn(),
  resumeMonitor: vi.fn(),
  access: { current: { allowed: true, disabled: false } as Record<string, unknown> },
}));

vi.mock('@/api/synthetics', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  getMonitor: mocks.getMonitor,
  listResults: mocks.listResults,
  listLocations: mocks.listLocations,
  runMonitor: mocks.runMonitor,
  testRevision: mocks.testRevision,
  publishRevision: mocks.publishRevision,
  pauseMonitor: mocks.pauseMonitor,
  resumeMonitor: mocks.resumeMonitor,
}));
vi.mock('@/product/actionAccess', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  useActionAccess: () => mocks.access.current,
}));
vi.mock('@/viz/timeseries/TimeSeriesChart', () => ({
  TimeSeriesChart: ({ series }: { series: Array<{ name: string; data: unknown[] }> }) => (
    <div data-testid="response-chart">
      {series.map((item) => `${item.name}:${item.data.length}`).join(',')}
    </div>
  ),
}));
vi.mock('../../results/Drawer', () => ({
  ResultDrawer: ({ result: open }: { result: SyntheticResult | undefined }) =>
    open ? <div data-testid="result-drawer">{open.id}</div> : null,
}));

function detail(overrides: {
  monitor?: Parameters<typeof monitor>[0];
  revision?: Parameters<typeof revision>[0];
} = {}): MonitorDetail {
  return { monitor: monitor(overrides.monitor), revisions: [revision(overrides.revision)] };
}

/** A check whose only revision is a draft: nothing of it is on schedule yet. */
function draftDetail(revisionOverrides: Parameters<typeof revision>[0] = {}): MonitorDetail {
  const unpublished = monitor({ draft_revision_id: 'revision-1' });
  delete unpublished.active_revision_id;

  return { monitor: unpublished, revisions: [revision(revisionOverrides)] };
}

function renderDetail(url = '/synthetics/checks/check-1') {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });

  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[url]}>
        <Routes>
          {['', '/results', '/results/:resultId', '/configuration', '/revisions'].map((suffix) => (
            <Route
              key={suffix}
              path={`/synthetics/checks/:monitorId${suffix}`}
              element={<CheckDetail />}
            />
          ))}
          <Route path="/synthetics/checks/:monitorId/edit" element={<p>the editor</p>} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

let restoreMotion: () => void;

beforeEach(() => {
  restoreMotion = reduceMotionInTests();
  vi.useFakeTimers({ toFake: ['Date'], now: new Date(NOW / 1_000) });
  mocks.access.current = { allowed: true, disabled: false };
  for (const mock of Object.values(mocks)) {
    if (typeof mock === 'function') mock.mockReset();
  }
  mocks.getMonitor.mockResolvedValue(detail());
  mocks.listResults.mockResolvedValue([
    ...runs(['healthy', 'healthy', 'failing'], { location_id: 'loc-us' }),
    ...runs(['healthy'], { location_id: 'loc-eu' }),
  ]);
  mocks.listLocations.mockResolvedValue([
    location(),
    location({ id: 'loc-eu', name: 'Frankfurt', code: 'eu-central-1' }),
  ]);
  mocks.runMonitor.mockResolvedValue(undefined);
  mocks.testRevision.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
  restoreMotion();
  vi.useRealTimers();
});

describe('Synthetics check detail', () => {
  it("is the check's own page: its name, its tabs, and none of the module's", async () => {
    renderDetail();

    expect(await screen.findByRole('heading', { level: 1, name: 'Checkout API' })).toBeTruthy();
    const tabs = screen.getByTestId('synthetics-check-tabs');
    expect(
      (['Overview', 'Results', 'Configuration', 'Revisions'] as const).map((name) =>
        within(tabs).getByRole('link', { name }).getAttribute('href'),
      ),
    ).toEqual([
      '/synthetics/checks/check-1',
      '/synthetics/checks/check-1/results',
      '/synthetics/checks/check-1/configuration',
      '/synthetics/checks/check-1/revisions',
    ]);
    expect(within(tabs).getAllByRole('link')).toHaveLength(4);
    expect(within(tabs).getByRole('link', { name: 'Overview' }).getAttribute('aria-current')).toBe(
      'page',
    );
    // The tabs of the module belong to the module's pages.
    expect(screen.queryByTestId('synthetics-subnav')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Refresh' })).toBeNull();
  });

  it('opens with how the check stands and what that rests on', async () => {
    renderDetail();

    const standing = await screen.findByRole('region', { name: 'The check now' });
    expect(within(standing).getByRole('heading').textContent).toBe('Passing');
    expect(standing.textContent).toContain('Last failed 3 minutes ago');
    expect(standing.textContent).toContain('75.0%');
    expect(standing.textContent).toContain('of the last 4 runs');
    expect(standing.textContent).toContain('from 2 locations · alerts after 2 failed in a row');

    // The same runs drawn and named, per place the check runs from.
    expect(screen.getByTestId('response-chart').textContent).toBe('N. Virginia:3,Frankfurt:1');
    const places = screen.getByRole('region', { name: 'By location' });
    expect(within(places).getAllByRole('listitem')).toHaveLength(2);
    expect(
      within(places).getByRole('img', { name: '2 of the last 3 runs passed' }),
    ).toBeTruthy();
  });

  it('says since when a failing check has failed, and what it last said', async () => {
    mocks.getMonitor.mockResolvedValue(detail({ monitor: { state: 'failing' } }));
    mocks.listResults.mockResolvedValue(
      runs(['failing', 'failing', 'healthy'], {
        attempts: [
          {
            number: 1,
            started_at: 0,
            finished_at: 1,
            outcome: 'failing',
            timing: {},
            error_message: 'connect ECONNREFUSED',
            metadata: {},
          },
        ],
      }),
    );
    renderDetail();

    const standing = await screen.findByRole('region', { name: 'The check now' });
    expect(within(standing).getByRole('heading').textContent).toBe('Failing');
    expect(within(standing).getByRole('heading').className).toContain('text-red');
    expect(standing.textContent).toContain('since 2 minutes ago');
    expect(standing.textContent).toContain('connect ECONNREFUSED');
  });

  it('runs the check as its one filled action', async () => {
    vi.useRealTimers();
    const user = userEvent.setup();
    renderDetail();

    const run = await screen.findByRole('button', { name: 'Run now' });
    expect(run.className).toContain('bg-indigo');
    expect(screen.queryByRole('button', { name: 'Publish revision' })).toBeNull();
    await user.click(run);
    expect(mocks.runMonitor).toHaveBeenCalledWith('check-1');
  });

  it('offers a draft its test, and its publication once the test has passed', async () => {
    mocks.getMonitor.mockResolvedValue(draftDetail());
    renderDetail();

    const publish = await screen.findByRole('button', { name: 'Publish revision' });
    expect(publish.hasAttribute('disabled')).toBe(true);
    expect(screen.getByRole('button', { name: 'Test revision' }).className).not.toContain('bg-indigo');
    expect(screen.getByRole('note').textContent).toContain('This revision is a draft');
    // Nothing is on schedule yet, so there is nothing to pause.
    expect(screen.queryByRole('button', { name: 'Pause' })).toBeNull();

    cleanup();
    mocks.getMonitor.mockResolvedValue(draftDetail({ last_test_passed_at: NOW - MINUTE }));
    renderDetail();
    expect(
      (await screen.findByRole('button', { name: 'Publish revision' })).hasAttribute('disabled'),
    ).toBe(false);
  });

  it('lists the runs under Results and opens one from its address', async () => {
    const opened = result({ id: 'run-7', started_at: NOW - 5 * MINUTE });
    mocks.listResults.mockResolvedValue([opened, ...runs(['healthy'])]);
    renderDetail('/synthetics/checks/check-1/results/run-7');

    expect((await screen.findByTestId('result-drawer')).textContent).toBe('run-7');
    expect(screen.getAllByRole('row').slice(1)).toHaveLength(2);
    expect(
      within(screen.getByTestId('synthetics-check-tabs'))
        .getByRole('link', { name: 'Results' })
        .getAttribute('aria-current'),
    ).toBe('page');
  });

  it('states the configuration in words', async () => {
    renderDetail('/synthetics/checks/check-1/configuration');

    const configuration = await screen.findByRole('region', { name: 'Configuration' });
    const facts = Object.fromEntries(
      within(configuration)
        .getAllByRole('term')
        .map((term) => [term.textContent, term.nextElementSibling?.textContent]),
    );
    expect(facts.Target).toBe('db.acme.corp:5432');
    expect(facts.Locations).toBe('N. Virginia, Frankfurt');
    expect(facts['Location policy']).toBe('Fails when most locations fail');
    expect(facts.Notifies).toBe('Nobody: no escalation policy');
  });

  it('says a check could not be loaded', async () => {
    mocks.getMonitor.mockRejectedValue(new Error('not found'));
    renderDetail();

    expect(
      await screen.findByText('Synthetic monitoring data could not be loaded.'),
    ).toBeTruthy();
    expect(screen.queryByTestId('synthetics-check-tabs')).toBeNull();
  });
});
