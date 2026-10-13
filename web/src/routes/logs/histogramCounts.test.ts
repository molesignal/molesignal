import { cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  logsHistogram: vi.fn(),
}));

vi.mock('@/api/web', () => ({
  logsHistogram: mocks.logsHistogram,
}));

import { type CountedLogQuery, useLogHistogramCounts } from './histogramCounts';

const QUERY: CountedLogQuery = {
  orgId: 'org-1',
  stream: 'app_logs',
  statement: "service = 'api' AND timeout",
  globalFilters: [{ key: 'env', operator: '=', value: 'prod' }],
  ranAt: 1_000,
};
const REQUEST = { range: { start: 100, end: 200 }, binMicros: 10, originMicros: 90 };

beforeEach(() => {
  mocks.logsHistogram.mockReset();
});

afterEach(() => cleanup());

describe('useLogHistogramCounts', () => {
  it('counts with the request the list sends, into the bars the chart asks for', async () => {
    mocks.logsHistogram.mockResolvedValue({
      bucket_micros: 10,
      origin_micros: 90,
      buckets: [{ start_micros: 100, total: 8, errors: 1 }],
      split: true,
    });
    const { result } = renderHook(() => useLogHistogramCounts(QUERY));
    const signal = new AbortController().signal;

    await expect(result.current?.load(REQUEST, signal)).resolves.toEqual({
      bars: [{ startMicros: 100, total: 8, errors: 1 }],
      split: true,
    });
    expect(mocks.logsHistogram).toHaveBeenCalledExactlyOnceWith(
      {
        stream: 'app_logs',
        from: 100,
        to: 200,
        filters: [
          { field: 'service', op: '=', value: 'api', quoted: true },
          { field: 'env', op: '=', value: 'prod', quoted: true },
        ],
        free_text: ['timeout'],
        bucket_micros: 10,
        origin_micros: 90,
      },
      signal,
    );
  });

  it('says why when the backend has no counts', async () => {
    mocks.logsHistogram.mockResolvedValue({
      bucket_micros: 10,
      origin_micros: 90,
      buckets: [],
      split: false,
      unavailable: 'time_budget',
    });
    const { result } = renderHook(() => useLogHistogramCounts(QUERY));

    await expect(result.current?.load(REQUEST, new AbortController().signal)).resolves.toEqual({
      unavailable: 'time_budget',
    });
  });

  it('names the query, and the run of it, that is counted', () => {
    const { result, rerender } = renderHook(({ query }) => useLogHistogramCounts(query), {
      initialProps: { query: QUERY as CountedLogQuery | null },
    });
    const first = result.current?.key;

    // The same query run again is counted again; another page of it is not.
    rerender({ query: { ...QUERY, ranAt: 2_000 } });
    expect(result.current?.key).not.toEqual(first);
    rerender({ query: { ...QUERY, statement: "service = 'web'" } });
    expect(result.current?.key).not.toEqual(first);
    rerender({ query: QUERY });
    expect(result.current?.key).toEqual(first);
  });

  it('has nothing to count for a statement the reader wrote', () => {
    const { result } = renderHook(() => useLogHistogramCounts(null));

    expect(result.current).toBeUndefined();
  });
});
