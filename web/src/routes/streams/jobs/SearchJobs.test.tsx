import '@/i18n';

import { focusManager, QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { MemoryRouter, useLocation } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import * as searchJobsApi from '@/api/searchJobs';
import type { SearchJob, SearchJobState, SearchJobSummary } from '@/api/searchJobs';
import i18n from '@/i18n';
import { setActiveTimePreference } from '@/lib/time';
import type * as productAccess from '@/product/access';
import type { ProductAccess } from '@/product/access';
import { TooltipProvider } from '@/shell/ui/tooltip';

import { downloadJobResults } from './download';
import { ACTIVE_REFRESH_MS, IDLE_REFRESH_MS } from './model';

import { SearchJobs } from './index';

vi.mock('@/api/searchJobs', async (importOriginal) => ({
  ...(await importOriginal<typeof searchJobsApi>()),
  list: vi.fn(),
  summary: vi.fn(),
  cancel: vi.fn(),
  retry: vi.fn(),
  remove: vi.fn(),
}));
vi.mock('./download', () => ({ downloadJobResults: vi.fn() }));
vi.mock('@/shell/ui/sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

const session = vi.hoisted(() => ({ access: null as unknown }));
vi.mock('@/product/access', async (importOriginal) => ({
  ...(await importOriginal<typeof productAccess>()),
  useProductAccess: () => session.access,
}));

const api = vi.mocked(searchJobsApi);
const download = vi.mocked(downloadJobResults);

const SECOND = 1_000_000;
const NOW = Date.UTC(2026, 4, 23, 10) * 1000;

function access(permissions: string[], pipelines = true): ProductAccess {
  return {
    organizationId: 'acme-prod',
    role: 'Owner',
    scope: 'organization',
    permissions: new Set(permissions) as ProductAccess['permissions'],
    version: 1,
    routeCatalogVersion: 1,
    routes: [{ id: 'pipeline.detail', path_pattern: '/pipelines/:id', allowed: pipelines }],
    status: 'ready',
  };
}

function job(id: string, state: SearchJobState, overrides: Partial<SearchJob> = {}): SearchJob {
  const finished = state === 'done' || state === 'failed' || state === 'cancelled';

  return {
    job_id: id,
    state,
    kind: 'query',
    pipeline_id: null,
    attempt: 0,
    user_id: 'u1',
    submitted_by: { display_name: 'Dana Reyes', email: 'sre@example.com' },
    request: {
      language: 'sql',
      statement: `SELECT *\nFROM "${id}"`,
      time_range: { start: NOW - 3_600 * SECOND, end: NOW },
      stream: { name: `stream-${id}`, stream_type: 'logs' },
      limit: null,
    },
    submitted_at_micros: NOW - 120 * SECOND,
    started_at_micros: state === 'pending' ? null : NOW - 110 * SECOND,
    finished_at_micros: finished ? NOW - 68 * SECOND : null,
    result_object_key: null,
    result_rows: state === 'done' ? 12_480 : null,
    error: state === 'failed' ? 'resource exhausted: the query needs 2.4 GiB of memory' : null,
    expires_at_micros: NOW + 86_400 * SECOND,
    ...overrides,
  };
}

const RUNNING = job('job-running', 'running');
const BACKFILL = job('job-backfill', 'pending', {
  kind: 'backfill',
  pipeline_id: 'checkout-logs',
  submitted_by: { display_name: 'Kenji Watanabe', email: 'kenji@example.com' },
});
const DONE = job('job-done', 'done');
const EMPTY = job('job-empty', 'done', { result_rows: 0 });
const FAILED = job('job-failed', 'failed', { attempt: 1 });
const CANCELLED = job('job-cancelled', 'cancelled', { submitted_by: null, user_id: 'u-former' });

/** A backend holding `jobs`: the list and the counts are answered from them, as it stands now. */
function serve(jobs: SearchJob[]) {
  const held = [...jobs];
  api.summary.mockImplementation(async () => {
    const count = (state: SearchJobState) => held.filter((item) => item.state === state).length;
    const counts: SearchJobSummary = {
      pending: count('pending'),
      running: count('running'),
      done: count('done'),
      failed: count('failed'),
      cancelled: count('cancelled'),
      total: held.length,
    };
    return counts;
  });
  api.list.mockImplementation(async ({ states = [], limit = 50, offset = 0 } = {}) =>
    held
      .filter((item) => states.length === 0 || states.includes(item.state))
      .slice(offset, offset + limit),
  );
  const change = (id: string, next: Partial<SearchJob>) => {
    const index = held.findIndex((item) => item.job_id === id);
    held[index] = { ...held[index]!, ...next };
    return held[index]!;
  };
  api.cancel.mockImplementation(async (id) => change(id, { state: 'cancelled' }));
  api.retry.mockImplementation(async (id) => change(id, { state: 'pending', error: null }));
  api.remove.mockImplementation(async (id) => {
    held.splice(
      held.findIndex((item) => item.job_id === id),
      1,
    );
  });
  return { change };
}

let address = '';
function Address() {
  const location = useLocation();
  address = `${location.pathname}${location.search}`;
  return null;
}

function renderPage(entry = '/streams/jobs') {
  render(
    <QueryClientProvider client={new QueryClient({ defaultOptions: { queries: { retry: false } } })}>
      <TooltipProvider delayDuration={0}>
        <MemoryRouter initialEntries={[entry]}>
          <SearchJobs />
          <Address />
        </MemoryRouter>
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

const row = (id: string) => {
  const cell = document.querySelector<HTMLElement>(`[data-search-job="${id}"]`);
  if (!cell) throw new Error(`no row for ${id}`);
  return within(cell.closest('tr')!);
};
const rowIds = () =>
  [...document.querySelectorAll<HTMLElement>('[data-search-job]')].map(
    (cell) => cell.dataset.searchJob,
  );
const listed = async (id: string) => waitFor(() => row(id));
const dialog = () => within(screen.getByRole('dialog'));

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
  vi.clearAllMocks();
  vi.spyOn(Date, 'now').mockReturnValue(NOW / 1000);
  setActiveTimePreference('UTC', 'iso_24h');
  // An administrator, who is shown the jobs of everybody in the organization.
  session.access = access(['streams.query', 'pipelines.read', 'org.settings.manage']);
  download.mockResolvedValue(12_480);
});

afterEach(() => {
  cleanup();
  focusManager.setFocused(undefined);
  setActiveTimePreference('', 'iso_24h');
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe('search jobs', () => {
  it('says of each job what was asked, by whom, and how it went', async () => {
    serve([BACKFILL, RUNNING, DONE, FAILED, CANCELLED]);
    renderPage();
    await listed('job-running');

    expect(rowIds()).toEqual(['job-backfill', 'job-running', 'job-done', 'job-failed', 'job-cancelled']);

    const running = row('job-running');
    expect(running.getByText('Running')).toBeInTheDocument();
    expect(running.getByText('Query')).toBeInTheDocument();
    expect(running.getByText('stream-job-running')).toBeInTheDocument();
    // The statement is on one line, whatever lines it was written on.
    expect(running.getByText('SELECT * FROM "job-running"')).toBeInTheDocument();
    expect(running.getByText('2026-05-23 09:00')).toBeInTheDocument();
    expect(running.getByText('2026-05-23 10:00')).toBeInTheDocument();
    expect(running.getByText('Dana Reyes')).toBeInTheDocument();
    // Still running: timed up to now, and not finished.
    expect(running.getByText('1 min 50 s')).toBeInTheDocument();
    expect(running.getByText('Not finished')).toBeInTheDocument();
    expect(running.getByTitle('Attempt 1')).toHaveTextContent('1');

    const done = row('job-done');
    expect(done.getByText('Done')).toBeInTheDocument();
    expect(done.getByText('42 s')).toBeInTheDocument();
    expect(done.getByText('12,480')).toBeInTheDocument();
    expect(done.getByText('in 24 hours')).toBeInTheDocument();

    // Why a job failed is said where its statement is, and a retry counts.
    const failed = row('job-failed');
    expect(failed.getByText('Failed')).toBeInTheDocument();
    expect(
      failed.getByText('Failed: resource exhausted: the query needs 2.4 GiB of memory'),
    ).toBeInTheDocument();
    expect(failed.getByTitle('Attempt 2')).toHaveTextContent('2');

    // A backfill says so and leads to its pipeline.
    const backfill = row('job-backfill');
    expect(backfill.getByText('Queued')).toBeInTheDocument();
    expect(backfill.getByText('Backfill')).toBeInTheDocument();
    expect(backfill.getByText('Kenji Watanabe')).toBeInTheDocument();
    expect(backfill.getByRole('link', { name: 'Open pipeline' })).toHaveAttribute(
      'href',
      '/pipelines/checkout-logs',
    );

    // Whoever submitted it has left: the id is what remains of them.
    expect(row('job-cancelled').getByText('u-former')).toBeInTheDocument();
    expect(running.queryByRole('link', { name: 'Open pipeline' })).toBeNull();
    // An administrator's list is everybody's, so it does not say it is one's own.
    expect(screen.queryByText('Only the jobs you submitted')).toBeNull();
  });

  it('tells a member that the list is their own jobs, and does not repeat their name on each', async () => {
    session.access = access(['streams.query']);
    serve([RUNNING, DONE]);
    renderPage();
    await listed('job-running');

    expect(screen.getByText('Only the jobs you submitted')).toBeInTheDocument();
    expect(screen.queryByText('Dana Reyes')).toBeNull();
    // When it was submitted is still said, now as the first thing in its cell.
    expect(row('job-running').getByText('2 minutes ago')).toBeInTheDocument();
  });

  it('shows everybody\'s jobs to whoever manages system telemetry', async () => {
    session.access = access(['sys.telemetry.read', 'sys.telemetry.manage']);
    serve([RUNNING]);
    renderPage();
    await listed('job-running');

    expect(row('job-running').getByText('Dana Reyes')).toBeInTheDocument();
    expect(screen.queryByText('Only the jobs you submitted')).toBeNull();
  });

  it('does not take reading the organization\'s settings for managing its jobs', async () => {
    session.access = access(['streams.query', 'org.settings.read', 'pipelines.run']);
    serve([RUNNING]);
    renderPage();
    await listed('job-running');

    expect(screen.getByText('Only the jobs you submitted')).toBeInTheDocument();
  });

  it('does not lead to a pipeline the reader may not open', async () => {
    session.access = access(['streams.query'], false);
    serve([BACKFILL]);
    renderPage();
    await listed('job-backfill');

    expect(row('job-backfill').getByText('Backfill')).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Open pipeline' })).toBeNull();
  });

  it('counts the jobs of every state and lists those of the one chosen', async () => {
    serve([BACKFILL, RUNNING, DONE, EMPTY, FAILED, CANCELLED]);
    renderPage();
    await listed('job-running');

    const views = within(screen.getByRole('group', { name: 'Job state' }));
    const counted = [
      ['All', 6],
      ['Queued', 1],
      ['Running', 1],
      ['Done', 2],
      ['Failed', 1],
      ['Cancelled', 1],
    ] as const;
    expect(views.getAllByRole('button')).toHaveLength(counted.length);
    for (const [label, count] of counted) {
      expect(
        views.getByRole('button', { name: new RegExp(`^${label}\\s*${count}$`) }),
      ).toBeInTheDocument();
    }
    expect(api.list).toHaveBeenLastCalledWith({ states: [], limit: 50, offset: 0 });

    fireEvent.click(views.getByRole('button', { name: /Done/ }));

    await waitFor(() => expect(rowIds()).toEqual(['job-done', 'job-empty']));
    expect(api.list).toHaveBeenLastCalledWith({ states: ['done'], limit: 50, offset: 0 });
    expect(address).toBe('/streams/jobs?state=done');

    fireEvent.click(views.getByRole('button', { name: /All/ }));

    await listed('job-running');
    expect(address).toBe('/streams/jobs');
  });

  it('opens on the state the address names', async () => {
    serve([BACKFILL, RUNNING, DONE]);
    renderPage('/streams/jobs?state=running');
    await listed('job-running');

    expect(rowIds()).toEqual(['job-running']);
    expect(
      within(screen.getByRole('group', { name: 'Job state' })).getByRole('button', {
        name: /Running/,
      }),
    ).toHaveAttribute('aria-pressed', 'true');
  });

  it('keeps the views when the chosen state has no job, so another can be chosen', async () => {
    serve([RUNNING, DONE]);
    renderPage('/streams/jobs?state=failed');

    expect(await screen.findByText('No jobs have failed')).toBeInTheDocument();
    expect(screen.getByRole('group', { name: 'Job state' })).toBeInTheDocument();
    expect(rowIds()).toEqual([]);
  });

  it('cancels a job only once the reader has said so, and shows it cancelled', async () => {
    serve([RUNNING, DONE]);
    renderPage();
    await listed('job-running');

    fireEvent.click(row('job-running').getByRole('button', { name: 'Cancel job job-runnin' }));
    expect(dialog().getByText('Cancel this job?')).toBeInTheDocument();
    expect(api.cancel).not.toHaveBeenCalled();

    // Changing one's mind leaves the job running.
    fireEvent.click(dialog().getByRole('button', { name: 'Keep running' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(api.cancel).not.toHaveBeenCalled();

    fireEvent.click(row('job-running').getByRole('button', { name: 'Cancel job job-runnin' }));
    fireEvent.click(dialog().getByRole('button', { name: 'Cancel job' }));

    await waitFor(() => expect(row('job-running').getByText('Cancelled')).toBeInTheDocument());
    expect(api.cancel).toHaveBeenCalledExactlyOnceWith('job-running');
    expect(screen.queryByRole('dialog')).toBeNull();
    // What a cancelled job offers is another run.
    expect(row('job-running').getByRole('button', { name: 'Retry job job-runnin' })).toBeEnabled();
    expect(row('job-running').queryByRole('button', { name: /Cancel job/ })).toBeNull();
  });

  it('retries a failed job at once and shows it queued again', async () => {
    serve([FAILED]);
    renderPage();
    await listed('job-failed');

    fireEvent.click(row('job-failed').getByRole('button', { name: 'Retry job job-failed' }));

    await waitFor(() => expect(row('job-failed').getByText('Queued')).toBeInTheDocument());
    expect(api.retry).toHaveBeenCalledExactlyOnceWith('job-failed');
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(row('job-failed').queryByText(/resource exhausted/)).toBeNull();
  });

  it('deletes a job after a warning that fits its state', async () => {
    serve([RUNNING, DONE]);
    renderPage();
    await listed('job-done');

    fireEvent.click(row('job-running').getByRole('button', { name: 'Delete job job-runnin' }));
    expect(
      dialog().getByText(
        'The job has not finished: it is cancelled first, then deleted, and cannot be restored.',
      ),
    ).toBeInTheDocument();
    fireEvent.click(dialog().getByRole('button', { name: 'Cancel' }));
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull());
    expect(api.remove).not.toHaveBeenCalled();

    fireEvent.click(row('job-done').getByRole('button', { name: 'Delete job job-done' }));
    expect(
      dialog().getByText('The job and its stored results are deleted and cannot be restored.'),
    ).toBeInTheDocument();
    fireEvent.click(dialog().getByRole('button', { name: 'Delete' }));

    await waitFor(() => expect(rowIds()).toEqual(['job-running']));
    expect(api.remove).toHaveBeenCalledExactlyOnceWith('job-done');
  });

  it('downloads the results of a finished job, unless it has none', async () => {
    serve([DONE, EMPTY, RUNNING]);
    renderPage();
    await listed('job-done');

    fireEvent.click(
      row('job-done').getByRole('button', { name: 'Download the results of job job-done' }),
    );

    await waitFor(() => expect(download).toHaveBeenCalledTimes(1));
    expect(download.mock.calls[0]![0]).toMatchObject({ job_id: 'job-done' });
    expect(
      row('job-empty').getByRole('button', { name: 'Download the results of job job-empty' }),
    ).toBeDisabled();
    // A job that has not finished has nothing to download.
    expect(row('job-running').queryByRole('button', { name: /Download/ })).toBeNull();
  });

  it('pages through a long list a page at a time', async () => {
    serve(Array.from({ length: 120 }, (_, index) => job(`job-${index}`, 'done')));
    renderPage();
    await listed('job-0');

    expect(rowIds()).toHaveLength(50);
    expect(screen.getByText('1 / 3')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Next page' }));

    await listed('job-50');
    expect(api.list).toHaveBeenLastCalledWith({ states: [], limit: 50, offset: 50 });
    expect(rowIds()[0]).toBe('job-50');
  });

  it('says what a search job is and where one comes from when there is none', async () => {
    serve([]);
    renderPage();

    expect(await screen.findByText('No search jobs yet')).toBeInTheDocument();
    expect(screen.getByText(/runs in the background and keeps its results/)).toBeInTheDocument();
    const state = within(document.querySelector<HTMLElement>('[data-product-state]')!);
    expect(state.getAllByRole('listitem').map((item) => item.textContent)).toEqual([
      'A query sent with Prefer: respond-async, or one whose result is expected to be too large',
      'The backfill of a data pipeline',
      'A query submitted by Mole Agent',
    ]);
    // With no job there is no state to choose between.
    expect(screen.queryByRole('group', { name: 'Job state' })).toBeNull();
  });

  it('tells a member with no job that they have submitted none', async () => {
    session.access = access(['streams.query']);
    serve([]);
    renderPage();

    expect(await screen.findByText('You have not submitted a search job yet')).toBeInTheDocument();
    expect(screen.queryByText('No search jobs yet')).toBeNull();
  });

  it('turns away a reader who may not run a query, without asking the backend', async () => {
    session.access = access(['streams.read']);
    serve([RUNNING]);
    renderPage();

    expect(await screen.findByText('You cannot view search jobs')).toBeInTheDocument();
    expect(api.list).not.toHaveBeenCalled();
    expect(api.summary).not.toHaveBeenCalled();
  });

  it('lets a reader of system telemetry in', async () => {
    session.access = access(['sys.telemetry.read']);
    serve([RUNNING]);
    renderPage();

    await listed('job-running');
  });

  it('shows what the backend said when the jobs cannot be read', async () => {
    serve([RUNNING]);
    api.summary.mockRejectedValue(new Error('search jobs unavailable'));
    renderPage();

    expect(await screen.findByRole('alert')).toHaveTextContent('search jobs unavailable');
  });
});

describe('search jobs refresh', () => {
  const listCalls = () => api.list.mock.calls.length;

  it('reads the list again every few seconds while a job is under way, and shows it move on', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const backend = serve([RUNNING, DONE]);
    renderPage();
    await listed('job-running');

    expect(screen.getByText('Jobs are under way, refreshing every 3 s')).toBeInTheDocument();
    const before = listCalls();
    backend.change('job-running', { state: 'done', result_rows: 7, finished_at_micros: NOW });

    await vi.advanceTimersByTimeAsync(ACTIVE_REFRESH_MS);

    await waitFor(() => expect(row('job-running').getByText('Done')).toBeInTheDocument());
    expect(listCalls()).toBeGreaterThan(before);
    // Nothing is under way any more, and the page stops saying so.
    await waitFor(() =>
      expect(screen.queryByText('Jobs are under way, refreshing every 3 s')).toBeNull(),
    );
  });

  it('reads an idle list only now and then', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    serve([DONE]);
    renderPage();
    await listed('job-done');

    expect(screen.queryByText('Jobs are under way, refreshing every 3 s')).toBeNull();
    const before = listCalls();

    await vi.advanceTimersByTimeAsync(ACTIVE_REFRESH_MS * 3);
    expect(listCalls()).toBe(before);

    await vi.advanceTimersByTimeAsync(IDLE_REFRESH_MS);
    expect(listCalls()).toBeGreaterThan(before);
  });

  it('is not read while nobody is looking, and is read at once when somebody looks again', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    serve([RUNNING]);
    renderPage();
    await listed('job-running');

    focusManager.setFocused(false);
    const hidden = listCalls();

    await vi.advanceTimersByTimeAsync(ACTIVE_REFRESH_MS * 5);
    expect(listCalls()).toBe(hidden);

    focusManager.setFocused(true);

    await waitFor(() => expect(listCalls()).toBeGreaterThan(hidden));
  });
});
