import '@/i18n';

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import { AxiosError, type AxiosResponse } from 'axios';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type * as queryApi from '@/api/query';
import type * as savedViewsApi from '@/api/savedViews';
import type { SavedView } from '@/api/savedViews';
import type * as streamsApi from '@/api/streams';
import { setActiveTimePreference } from '@/lib/time';
import { useAuthStore } from '@/stores/auth';
import type { QueryResult } from '@/types/query';

import { REPORT_ROW_LIMIT } from './query';
import { SavedViewReport } from './SavedViewReport';

const mocks = vi.hoisted(() => ({
  get: vi.fn(),
  runQuery: vi.fn(),
  listStreams: vi.fn(),
}));

vi.mock('@/api/savedViews', async (importOriginal) => ({
  ...(await importOriginal<typeof savedViewsApi>()),
  get: mocks.get,
}));

vi.mock('@/api/query', async (importOriginal) => ({
  ...(await importOriginal<typeof queryApi>()),
  runQuery: mocks.runQuery,
}));

vi.mock('@/api/streams', async (importOriginal) => ({
  ...(await importOriginal<typeof streamsApi>()),
  list: mocks.listStreams,
}));

const root = document.documentElement;
const NOW = new Date('2026-10-11T01:30:00Z');
const NOW_MICROS = NOW.getTime() * 1_000;
const DAY_MICROS = 86_400_000_000;

function view(overrides: Partial<SavedView> = {}): SavedView {
  return {
    id: 'v1',
    org_id: 'org-1',
    owner_user_id: 'user-1',
    name: 'Errors by service',
    language: 'sql',
    statement: 'SELECT _timestamp, service, errors FROM "app_logs"',
    time_range_secs: 86_400,
    stream: 'app_logs',
    tags: ['errors'],
    pinned: true,
    created_at: 0,
    updated_at: 0,
    ...overrides,
  };
}

function rows(): QueryResult {
  return {
    columns: ['_timestamp', 'service', 'errors', 'detail'],
    rows: [
      [Date.parse('2026-10-10T08:15:30Z') * 1_000, 'checkout', 42, { code: 502 }],
      [Date.parse('2026-10-10T09:00:00Z') * 1_000, 'payments', 7, null],
    ],
    scanned_rows: 1_200,
    took_ms: 12,
  };
}

function apiError(status: number, message: string): AxiosError {
  return new AxiosError(`Request failed with status code ${status}`, 'ERR_BAD_REQUEST', undefined, null, {
    status,
    data: { message },
  } as AxiosResponse);
}

function renderReport(url = '/saved-views?view=v1&report_render=1') {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });

  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter initialEntries={[url]}>
        <Routes>
          <Route path="/saved-views" element={<SavedViewReport />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

function facts(): Record<string, string> {
  const header = screen.getByRole('heading', { level: 1 }).closest('header');
  if (!header) throw new Error('The report has no header');
  const terms = within(header).getAllByRole('term');
  const definitions = within(header).getAllByRole('definition');

  return Object.fromEntries(
    terms.map((term, index) => [term.textContent ?? '', definitions[index]?.textContent ?? '']),
  );
}

function cells(): string[][] {
  return within(screen.getByRole('table'))
    .getAllByRole('row')
    .map((row) => Array.from(row.querySelectorAll('th, td'), (cell) => cell.textContent ?? ''));
}

beforeEach(() => {
  useAuthStore.setState({
    token: 'token',
    ctx: { user_id: 'user-1', org_id: 'org-1', display_role: 'Owner', roles: [] },
  });
  setActiveTimePreference('Asia/Shanghai', 'iso_24h');
  mocks.get.mockResolvedValue(view());
  mocks.runQuery.mockResolvedValue(rows());
  mocks.listStreams.mockResolvedValue([]);
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  mocks.get.mockReset();
  mocks.runQuery.mockReset();
  mocks.listStreams.mockReset();
  setActiveTimePreference('', 'iso_24h');
  useAuthStore.setState({ token: null, ctx: null });
});

describe('SavedViewReport', () => {
  it('prints the rows the view returns over the range the report states', async () => {
    vi.useFakeTimers({ toFake: ['Date'], now: NOW });
    renderReport(
      '/saved-views?view=v1&report_render=1&time=2026-10-03T16%3A00%3A00Z..2026-10-10T16%3A00%3A00Z',
    );

    expect(await screen.findByRole('heading', { level: 1, name: 'Errors by service' })).toBeTruthy();
    await screen.findByRole('table', { name: 'Results' });

    expect(mocks.get).toHaveBeenCalledExactlyOnceWith('v1');
    expect(mocks.runQuery).toHaveBeenCalledExactlyOnceWith({
      org_id: 'org-1',
      language: 'sql',
      statement: 'SELECT _timestamp, service, errors FROM "app_logs"',
      time_range: {
        start: Date.parse('2026-10-03T16:00:00Z') * 1_000,
        end: Date.parse('2026-10-10T16:00:00Z') * 1_000,
      },
      stream: { name: 'app_logs', stream_type: 'logs' },
      limit: REPORT_ROW_LIMIT + 1,
    });
    expect(facts()).toEqual({
      'Time range': '2026-10-04 00:00 – 2026-10-11 00:00',
      Timezone: 'Asia/Shanghai · UTC+8',
      Stream: 'app_logs',
      Rows: '2',
      Generated: '2026-10-11 09:30',
    });
    // The statement the rows come from, under the language it is written in.
    const statement = screen.getByRole('figure');
    expect(within(statement).getByText('SQL')).toBeTruthy();
    expect(within(statement).getByText('SELECT _timestamp, service, errors FROM "app_logs"')).toBeTruthy();
    // An instant is read as a time in the report's zone; anything else is printed as returned.
    expect(cells()).toEqual([
      ['_timestamp', 'service', 'errors', 'detail'],
      ['2026-10-10 16:15:30', 'checkout', '42', '{"code":502}'],
      ['2026-10-10 17:00:00', 'payments', '7', '—'],
    ]);
  });

  it("covers the view's own look-back when the report states no range", async () => {
    vi.useFakeTimers({ toFake: ['Date'], now: NOW });
    mocks.get.mockResolvedValue(view({ time_range_secs: 86_400 }));
    renderReport();

    await screen.findByRole('table');
    expect(mocks.runQuery.mock.calls[0]?.[0]).toMatchObject({
      time_range: { start: NOW_MICROS - DAY_MICROS, end: NOW_MICROS },
    });
    expect(facts()['Time range']).toBe('2026-10-10 09:30 – 2026-10-11 09:30');
  });

  it('prints a PromQL view as the points the range query returns', async () => {
    mocks.get.mockResolvedValue(
      view({
        language: 'promql',
        statement: 'sum by (status) (rate(http_requests_total[5m]))',
        stream: null,
      }),
    );
    mocks.runQuery.mockResolvedValue({
      columns: ['_timestamp', 'value', 'status'],
      rows: [[Date.parse('2026-10-10T08:00:00Z') * 1_000, 12.5, '500']],
      scanned_rows: 1,
      took_ms: 3,
    });
    renderReport('/saved-views?view=v1&report_render=1&time=now-24h..now');

    await screen.findByRole('table');
    expect(mocks.runQuery.mock.calls[0]?.[0]).toMatchObject({
      language: 'promql',
      statement: 'sum by (status) (rate(http_requests_total[5m]))',
      limit: REPORT_ROW_LIMIT,
    });
    expect(within(screen.getByRole('figure')).getByText('PromQL')).toBeTruthy();
    // A view that names no stream says nothing about one.
    expect(Object.keys(facts())).toEqual(['Time range', 'Timezone', 'Rows', 'Generated']);
    expect(cells()).toEqual([
      ['_timestamp', 'value', 'status'],
      ['2026-10-10 16:00:00', '12.5', '500'],
    ]);
  });

  it('carries none of the page a person works in', async () => {
    const { container } = renderReport();

    await screen.findByRole('table');
    expect(screen.queryAllByRole('button')).toEqual([]);
    expect(screen.queryAllByRole('combobox')).toEqual([]);
    expect(screen.queryAllByRole('link')).toEqual([]);
    expect(screen.queryAllByRole('navigation')).toEqual([]);
    expect(container.querySelector('select, input, textarea, [data-page-header]')).toBeNull();
  });

  it('tells the renderer it is ready only once the rows are in', async () => {
    let answer: (result: QueryResult) => void = () => undefined;
    mocks.runQuery.mockReturnValue(
      new Promise<QueryResult>((resolve) => {
        answer = resolve;
      }),
    );
    renderReport();

    await screen.findByRole('heading', { level: 1, name: 'Errors by service' });
    await waitFor(() => expect(mocks.runQuery).toHaveBeenCalled());
    // Long enough for the renderer's "nothing is loading" window to have passed, were it open.
    await new Promise((resolve) => setTimeout(resolve, 400));
    expect(root.dataset.reportRenderState).toBe('loading');
    expect(screen.queryByRole('table')).toBeNull();

    answer(rows());
    await waitFor(() => expect(root.dataset.reportRenderState).toBe('ready'), { timeout: 3_000 });
    expect(screen.getByRole('table').textContent).toContain('checkout');
    // Printed once: the statement ran a single time.
    expect(mocks.runQuery).toHaveBeenCalledTimes(1);
  });

  it('is ready with an empty result, and says the range has no rows', async () => {
    mocks.runQuery.mockResolvedValue({ columns: ['service'], rows: [], scanned_rows: 0, took_ms: 1 });
    renderReport();

    expect(await screen.findByText('No rows in this time range.')).toBeTruthy();
    await waitFor(() => expect(root.dataset.reportRenderState).toBe('ready'));
    expect(screen.queryByRole('table')).toBeNull();
    expect(facts().Rows).toBe('0');
  });

  it('says so when the statement returns more rows than a report prints', async () => {
    mocks.runQuery.mockResolvedValue({
      columns: ['service'],
      rows: Array.from({ length: REPORT_ROW_LIMIT + 1 }, (_, index) => [`svc-${index}`]),
      scanned_rows: 0,
      took_ms: 1,
    });
    renderReport();

    await screen.findByRole('table');
    expect(facts().Rows).toBe(`First ${REPORT_ROW_LIMIT} only`);
    // The header row and the rows that are printed.
    expect(within(screen.getByRole('table')).getAllByRole('row')).toHaveLength(REPORT_ROW_LIMIT + 1);
  });

  it('fails the report with the reason when the view cannot be loaded', async () => {
    mocks.get.mockRejectedValue(apiError(404, 'saved view v1 not found'));
    renderReport();

    expect((await screen.findByRole('alert')).textContent).toBe('saved view v1 not found');
    expect(root.dataset.reportRenderState).toBe('error');
    expect(root.dataset.reportRenderError).toBe('saved view v1 not found');
    expect(screen.queryByRole('heading', { level: 1 })).toBeNull();
    expect(mocks.runQuery).not.toHaveBeenCalled();
  });

  it('fails the report with the reason when the statement cannot be run', async () => {
    mocks.runQuery.mockRejectedValue(apiError(400, 'query could not be planned: check the SQL syntax'));
    renderReport();

    const reason = await screen.findByRole('alert');
    expect(reason.textContent).toBe('query could not be planned: check the SQL syntax');
    await waitFor(() => expect(root.dataset.reportRenderState).toBe('error'));
    expect(root.dataset.reportRenderError).toBe(reason.textContent);
    // An error is the whole page: there is no report around it to print.
    expect(screen.queryByRole('heading', { level: 1 })).toBeNull();
  });

  it('fails the report when a SQL view has no stream to run on', async () => {
    mocks.get.mockResolvedValue(view({ stream: null }));
    mocks.listStreams.mockResolvedValue([]);
    renderReport();

    const reason = await screen.findByRole('alert');
    expect(reason.textContent).toBe(
      'The saved view names no stream, and the organization has no logs stream to run it on.',
    );
    await waitFor(() => expect(root.dataset.reportRenderState).toBe('error'));
    expect(mocks.runQuery).not.toHaveBeenCalled();
  });

  it('fails the report when it names no view or a range that cannot be read', async () => {
    const unnamed = renderReport('/saved-views?report_render=1');

    expect((await screen.findByRole('alert')).textContent).toBe('The report does not name a saved view.');
    expect(root.dataset.reportRenderState).toBe('error');
    expect(mocks.get).not.toHaveBeenCalled();
    unnamed.unmount();

    renderReport('/saved-views?view=v1&report_render=1&time=yesterday..now');

    expect((await screen.findByRole('alert')).textContent).toBe('invalid time expression: yesterday');
    await waitFor(() => expect(root.dataset.reportRenderState).toBe('error'));
    expect(mocks.runQuery).not.toHaveBeenCalled();
  });
});
