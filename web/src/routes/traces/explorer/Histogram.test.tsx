import { cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  tracesHistogram: vi.fn(),
}));

vi.mock('@/api/web', () => ({
  tracesHistogram: mocks.tracesHistogram,
}));

import { type CountedTraceQuery, useTraceHistogram } from './Histogram';
import type { DisplayTrace } from './types';

const RANGE = { from: '2026-10-11T00:00:00.000Z', to: '2026-10-12T00:00:00.000Z' };
const TRACES: DisplayTrace[] = [
  { id: 'a', op: 'GET /', service: 'web', startNs: 1_791_676_800_000_000_000, durationMs: 12, spans: 3, errors: 0 },
  { id: 'b', op: 'POST /pay', service: 'checkout', startNs: 1_791_676_801_500_000_000, durationMs: 40, spans: 9, errors: 2 },
];
const QUERY: CountedTraceQuery = {
  orgId: 'org-1',
  q: 'checkout',
  filters: [{ field: 'service.name', op: '=', value: 'checkout' }],
  epoch: 0,
};
const REQUEST = { range: { start: 100, end: 200 }, binMicros: 10, originMicros: 90 };

const histogramOf = (counted: CountedTraceQuery | null) =>
  renderHook(({ query }) => useTraceHistogram({ traces: TRACES, range: RANGE, onTimeWindowChange: vi.fn(), counted: query }), {
    initialProps: { query: counted },
  });

beforeEach(() => {
  mocks.tracesHistogram.mockReset();
});

afterEach(() => cleanup());

describe('useTraceHistogram', () => {
  it('draws the loaded traces by when they started, the ones with errors apart', () => {
    const { result } = histogramOf(null);

    expect(result.current.points).toEqual([
      { micros: 1_791_676_800_000_000, error: false },
      { micros: 1_791_676_801_500_000, error: true },
    ]);
    expect(result.current.range).toEqual({
      start: Date.parse(RANGE.from) * 1_000,
      end: Date.parse(RANGE.to) * 1_000,
    });
  });

  it('counts with the query the list sends, into the bars the chart asks for', async () => {
    mocks.tracesHistogram.mockResolvedValue({
      bucket_micros: 10,
      origin_micros: 90,
      buckets: [{ start_micros: 110, total: 31, errors: 4 }],
      split: true,
    });
    const { result } = histogramOf(QUERY);
    const signal = new AbortController().signal;

    await expect(result.current.counts?.load(REQUEST, signal)).resolves.toEqual({
      bars: [{ startMicros: 110, total: 31, errors: 4 }],
      split: true,
    });
    expect(mocks.tracesHistogram).toHaveBeenCalledExactlyOnceWith(
      {
        from: 100,
        to: 200,
        q: 'checkout',
        filters: [{ field: 'service.name', op: '=', value: 'checkout' }],
        bucket_micros: 10,
        origin_micros: 90,
      },
      signal,
    );
  });

  it('sends no free text and no filters where the query has none', async () => {
    mocks.tracesHistogram.mockResolvedValue({ bucket_micros: 10, origin_micros: 90, buckets: [], split: true });
    const { result } = histogramOf({ orgId: 'org-1', filters: [], epoch: 0 });

    await result.current.counts?.load(REQUEST, new AbortController().signal);

    expect(mocks.tracesHistogram.mock.calls[0]?.[0]).toEqual({
      from: 100,
      to: 200,
      bucket_micros: 10,
      origin_micros: 90,
    });
  });

  it('counts again when the reader asks for the results again', () => {
    const { result, rerender } = histogramOf(QUERY);
    const first = result.current.counts?.key;

    rerender({ query: { ...QUERY, epoch: 1 } });
    expect(result.current.counts?.key).not.toEqual(first);
    rerender({ query: QUERY });
    expect(result.current.counts?.key).toEqual(first);
  });

  it('has nothing to count for a statement: its histogram is of the traces it returned', () => {
    expect(histogramOf(null).result.current.counts).toBeUndefined();
  });
});
