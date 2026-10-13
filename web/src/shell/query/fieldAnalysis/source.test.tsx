import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { renderHook, waitFor } from '@testing-library/react';
import * as React from 'react';
import { describe, expect, it, vi } from 'vitest';

import { analyzeLoadedValues } from './model';
import { answerFromPayload, type FieldAnalysisAnswer, type FieldAnalysisSource, useFieldAnalysis } from './source';

// One client for the life of a rendered hook: a new one on a re-render would read everything again.
function wrapper({ children }: { children: React.ReactNode }) {
  const [client] = React.useState(() => new QueryClient());

  return <QueryClientProvider client={client}>{children}</QueryClientProvider>;
}
const loaded = () => analyzeLoadedValues(['a', 'a', 'b'], 'text');
const source = (answer: () => Promise<FieldAnalysisAnswer>, key = 'q'): FieldAnalysisSource => ({
  key: [key],
  load: vi.fn(answer),
});
const WHOLE = answerFromPayload({
  kind: 'text',
  total: 900,
  present: 880,
  distinct: 3,
  distinct_exact: true,
  top: [{ value: 'a', count: 600 }],
  top_skipped: false,
});

describe('answerFromPayload', () => {
  it('reads an analysis as the endpoints answer it', () => {
    expect(
      answerFromPayload({
        kind: 'number',
        total: 10,
        present: 8,
        distinct: 5,
        distinct_exact: false,
        range: { min: '200', max: '504' },
        spread: { avg: 301.5, q1: 200, median: 204, q3: 404, stddev_samp: 120, stddev_pop: 112, skewness: null, kurtosis: 0.4 },
        bars: { origin: 200, width: 20, counts: [4, 0, 4] },
        top: [{ value: '200', count: 4 }],
        top_skipped: false,
        trend: { origin_micros: 60_000_000, bucket_micros: 60_000_000, series: [{ value: '200', counts: [3, 1] }] },
      }),
    ).toEqual({
      analysis: {
        kind: 'number',
        total: 10,
        present: 8,
        distinct: 5,
        distinctExact: false,
        range: { min: '200', max: '504' },
        spread: { avg: 301.5, q1: 200, median: 204, q3: 404, stddevSamp: 120, stddevPop: 112, skewness: null, kurtosis: 0.4 },
        bars: { origin: 200, width: 20, counts: [4, 0, 4] },
        top: [{ value: '200', count: 4 }],
        topSkipped: false,
        trend: { originMicros: 60_000_000, bucketMicros: 60_000_000, series: [{ value: '200', counts: [3, 1] }] },
      },
    });
  });

  it('has no trend where the endpoint sent none', () => {
    const answer = answerFromPayload({ kind: 'text', total: 3, present: 3, distinct: 3, top: [], top_skipped: true });

    expect('analysis' in answer && answer.analysis.trend).toBeNull();
  });

  it('says why there is no analysis', () => {
    expect(answerFromPayload({ kind: 'text', total: 0, unavailable: 'field_protected' })).toEqual({
      unavailable: 'field_protected',
    });
    expect(answerFromPayload({ kind: 'text', total: 0, unavailable: 'time_budget' })).toEqual({ unavailable: 'time_budget' });
  });

  it('counts an answer that is not an analysis as a failure', () => {
    expect(answerFromPayload(null)).toEqual({ unavailable: 'failed' });
    expect(answerFromPayload({ kind: 'colour', total: 1 })).toEqual({ unavailable: 'failed' });
    expect(answerFromPayload({ kind: 'text', total: 1, unavailable: 'something new' })).toEqual({ unavailable: 'failed' });
  });
});

describe('useFieldAnalysis', () => {
  it('is of the loaded results where nothing can read more', () => {
    const { result } = renderHook(() => useFieldAnalysis(undefined, loaded), { wrapper });

    expect(result.current).toMatchObject({ scope: 'loaded', pending: false, unavailable: null });
    expect(result.current.analysis).toMatchObject({ total: 3, distinct: 2 });
  });

  it('is of everything the query matches once that is read', async () => {
    const { result } = renderHook(() => useFieldAnalysis(source(async () => WHOLE), loaded), { wrapper });

    // What is asked for is everything: nothing of the loaded page is shown meanwhile.
    expect(result.current).toMatchObject({ scope: 'all', pending: true, analysis: null });
    await waitFor(() => expect(result.current.pending).toBe(false));
    expect(result.current).toMatchObject({ scope: 'all', unavailable: null });
    expect(result.current.analysis).toMatchObject({ total: 900, present: 880 });
  });

  it('falls back to the loaded results and says why', async () => {
    const busy = source(async () => ({ unavailable: 'busy' }));
    const { result } = renderHook(() => useFieldAnalysis(busy, loaded), { wrapper });

    await waitFor(() => expect(result.current.pending).toBe(false));
    expect(result.current).toMatchObject({ scope: 'loaded', unavailable: 'busy' });
    expect(result.current.analysis).toMatchObject({ total: 3 });
  });

  it('counts a request that did not come back as a failure', async () => {
    const broken = source(async () => {
      throw new Error('network');
    });
    const { result } = renderHook(() => useFieldAnalysis(broken, loaded), { wrapper });

    await waitFor(() => expect(result.current.pending).toBe(false));
    expect(result.current).toMatchObject({ scope: 'loaded', unavailable: 'failed' });
  });

  it('has nothing to show of a field whose values are not the reader’s to count', async () => {
    const masked = source(async () => ({ unavailable: 'field_protected' }));
    const local = vi.fn(loaded);
    const { result } = renderHook(() => useFieldAnalysis(masked, local), { wrapper });

    await waitFor(() => expect(result.current.pending).toBe(false));
    expect(result.current).toMatchObject({ analysis: null, unavailable: 'field_protected' });
    // What is loaded of a masked field is masks: it is not counted either.
    expect(local).not.toHaveBeenCalled();
  });

  it('reads again for another query and not for the same one', async () => {
    const first = source(async () => WHOLE, 'one');
    const second = source(async () => WHOLE, 'two');
    const { result, rerender } = renderHook(({ from }) => useFieldAnalysis(from, loaded), {
      wrapper,
      initialProps: { from: first },
    });

    await waitFor(() => expect(result.current.pending).toBe(false));
    rerender({ from: { ...first } });
    rerender({ from: second });
    await waitFor(() => expect(second.load).toHaveBeenCalledTimes(1));
    expect(first.load).toHaveBeenCalledTimes(1);
  });
});
