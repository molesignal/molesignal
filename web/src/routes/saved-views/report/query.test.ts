import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type * as queryApi from '@/api/query';
import type { SavedView } from '@/api/savedViews';
import type * as streamsApi from '@/api/streams';

import { NoStreamError, REPORT_ROW_LIMIT, reportRange, runSavedView } from './query';

const mocks = vi.hoisted(() => ({
  runQuery: vi.fn(),
  listStreams: vi.fn(),
}));

vi.mock('@/api/query', async (importOriginal) => ({
  ...(await importOriginal<typeof queryApi>()),
  runQuery: mocks.runQuery,
}));

vi.mock('@/api/streams', async (importOriginal) => ({
  ...(await importOriginal<typeof streamsApi>()),
  list: mocks.listStreams,
}));

const NOW = new Date('2026-10-11T02:00:00Z');
const NOW_MICROS = NOW.getTime() * 1_000;
const HOUR_MICROS = 3_600_000_000;
const RANGE = { start: NOW_MICROS - 24 * HOUR_MICROS, end: NOW_MICROS };

function view(overrides: Partial<SavedView> = {}): SavedView {
  return {
    id: 'v1',
    org_id: 'org-1',
    owner_user_id: 'user-1',
    name: 'Yesterday errors',
    language: 'sql',
    statement: 'SELECT service, count(*) AS errors FROM "app_logs" GROUP BY service',
    time_range_secs: 6 * 3_600,
    stream: 'app_logs',
    tags: [],
    pinned: false,
    created_at: 0,
    updated_at: 0,
    ...overrides,
  };
}

function stream(name: string, type: string, queryable = true) {
  return { name, type, settings: { queryable } };
}

beforeEach(() => {
  mocks.runQuery.mockResolvedValue({ columns: ['service'], rows: [['api']], scanned_rows: 1, took_ms: 1 });
  mocks.listStreams.mockResolvedValue([]);
});

afterEach(() => {
  mocks.runQuery.mockReset();
  mocks.listStreams.mockReset();
});

describe('reportRange', () => {
  it('is the range the report states', () => {
    // A rolling window is measured from when the report is made.
    expect(reportRange(view(), { from: 'now-7d', to: 'now', mode: 'relative' }, NOW)).toEqual({
      start: NOW_MICROS - 7 * 24 * HOUR_MICROS,
      end: NOW_MICROS,
    });
    expect(
      reportRange(
        view(),
        { from: '2026-10-03T16:00:00Z', to: '2026-10-10T16:00:00Z', mode: 'absolute' },
        NOW,
      ),
    ).toEqual({
      start: Date.parse('2026-10-03T16:00:00Z') * 1_000,
      end: Date.parse('2026-10-10T16:00:00Z') * 1_000,
    });
  });

  it("is the view's own look-back when the report states none", () => {
    expect(reportRange(view({ time_range_secs: 6 * 3_600 }), undefined, NOW)).toEqual({
      start: NOW_MICROS - 6 * HOUR_MICROS,
      end: NOW_MICROS,
    });
  });

  it('refuses a stated range it cannot read', () => {
    expect(() =>
      reportRange(view(), { from: 'yesterday', to: 'now', mode: 'relative' }, NOW),
    ).toThrow('invalid time expression: yesterday');
  });
});

describe('runSavedView', () => {
  it('sends a SQL statement as the explorer does, on the stream the view names', async () => {
    const result = await runSavedView(view(), 'org-1', RANGE);

    expect(mocks.runQuery).toHaveBeenCalledExactlyOnceWith({
      org_id: 'org-1',
      language: 'sql',
      statement: 'SELECT service, count(*) AS errors FROM "app_logs" GROUP BY service',
      time_range: RANGE,
      stream: { name: 'app_logs', stream_type: 'logs' },
      limit: REPORT_ROW_LIMIT + 1,
    });
    expect(mocks.listStreams).not.toHaveBeenCalled();
    expect(result).toEqual({ columns: ['service'], rows: [['api']], truncated: false });
  });

  it('prints no more than the row limit and says the statement returns more', async () => {
    const rows = Array.from({ length: REPORT_ROW_LIMIT + 1 }, (_, index) => [`svc-${index}`]);
    mocks.runQuery.mockResolvedValue({ columns: ['service'], rows, scanned_rows: 0, took_ms: 0 });

    const result = await runSavedView(view(), 'org-1', RANGE);

    expect(result.rows).toHaveLength(REPORT_ROW_LIMIT);
    expect(result.rows.at(-1)).toEqual([`svc-${REPORT_ROW_LIMIT - 1}`]);
    expect(result.truncated).toBe(true);
  });

  it('does not call a result that fits exactly truncated', async () => {
    const rows = Array.from({ length: REPORT_ROW_LIMIT }, (_, index) => [`svc-${index}`]);
    mocks.runQuery.mockResolvedValue({ columns: ['service'], rows, scanned_rows: 0, took_ms: 0 });

    const result = await runSavedView(view(), 'org-1', RANGE);

    expect(result.rows).toHaveLength(REPORT_ROW_LIMIT);
    expect(result.truncated).toBe(false);
  });

  it('opens a SQL view saved without a stream on the first logs stream that can be queried', async () => {
    mocks.listStreams.mockResolvedValue([
      stream('default', 'traces'),
      stream('archived_logs', 'logs', false),
      stream('app_logs', 'logs'),
      stream('audit_logs', 'logs'),
    ]);

    await runSavedView(view({ stream: null }), 'org-1', RANGE);

    expect(mocks.runQuery.mock.calls[0]?.[0]).toMatchObject({
      stream: { name: 'app_logs', stream_type: 'logs' },
    });
  });

  it('fails a SQL view that has no stream to run on', async () => {
    mocks.listStreams.mockResolvedValue([stream('default', 'traces')]);

    await expect(runSavedView(view({ stream: '  ' }), 'org-1', RANGE)).rejects.toBeInstanceOf(
      NoStreamError,
    );
    expect(mocks.runQuery).not.toHaveBeenCalled();
  });

  it('sends PromQL as a range query sampled to the row limit, with no stream', async () => {
    mocks.runQuery.mockResolvedValue({
      columns: ['_timestamp', 'value', 'status'],
      rows: [[NOW_MICROS, 12.5, '500']],
      scanned_rows: 1,
      took_ms: 1,
    });

    const result = await runSavedView(
      view({ language: 'promql', statement: 'sum by (status) (rate(http_requests_total[5m]))', stream: null }),
      'org-1',
      RANGE,
    );

    expect(mocks.runQuery).toHaveBeenCalledExactlyOnceWith({
      org_id: 'org-1',
      language: 'promql',
      statement: 'sum by (status) (rate(http_requests_total[5m]))',
      time_range: RANGE,
      limit: REPORT_ROW_LIMIT,
    });
    expect(mocks.listStreams).not.toHaveBeenCalled();
    expect(result).toEqual({
      columns: ['_timestamp', 'value', 'status'],
      rows: [[NOW_MICROS, 12.5, '500']],
      truncated: false,
    });
  });
});
