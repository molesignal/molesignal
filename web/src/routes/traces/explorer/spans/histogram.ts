import * as React from 'react';

import type { SpanListItem } from '@/api/traceExplorer';
import { tracesRed } from '@/api/traceRed';
import type { TraceCondition, TraceFilter } from '@/api/web';
import type { HistogramCountsAnswer, HistogramCountsSource } from '@/shell/query/histogram/counts';
import { useExplorerLayoutStore } from '@/stores/useExplorerLayoutStore';
import type { TimeWindow } from '@/stores/useTimeStore';

import type { TraceHistogramModel } from '../Histogram';

/** The span query whose matches the histogram counts. */
export interface CountedSpanQuery {
  orgId: string;
  q?: string | undefined;
  filters: TraceFilter[];
  where?: TraceCondition | undefined;
  /** Goes up when the reader asks for the results again. */
  epoch: number;
}

/**
 * The histogram over the span list: how many spans the query matches over the
 * range, failed ones apart, and how long they took, counted as the RED view
 * counts every span, so a filter holds for each span itself as it does in the
 * list. A statement has no such query: its histogram is of the spans it
 * returned.
 */
export function useSpanHistogram({
  spans,
  range,
  onTimeWindowChange,
  counted,
}: {
  spans: readonly SpanListItem[];
  range: { from: string; to: string };
  onTimeWindowChange: (window: TimeWindow) => void;
  counted: CountedSpanQuery | null;
}): TraceHistogramModel {
  const visible = useExplorerLayoutStore((state) => state.tracesHistogramVisible);
  const onVisibleChange = useExplorerLayoutStore((state) => state.setTracesHistogramVisible);

  const counts = React.useMemo<HistogramCountsSource | undefined>(() => {
    if (!counted) return undefined;
    const { orgId, q, filters, where, epoch } = counted;

    return {
      key: ['spans', orgId, q ?? '', JSON.stringify(filters), JSON.stringify(where ?? null), epoch],
      load: async ({ range: asked, binMicros, originTime: originMicros }, signal): Promise<HistogramCountsAnswer> => {
        const red = await tracesRed(
          {
            from: asked.start,
            to: asked.end,
            ...(q ? { q } : {}),
            ...(filters.length > 0 ? { filters } : {}),
            ...(where ? { where } : {}),
            scope: 'all',
            bucketTime: binMicros,
            originTime: originMicros,
          },
          signal,
        );

        if (red.unavailable) {
          return { unavailable: red.unavailable === 'field_protected' ? 'failed' : red.unavailable };
        }

        return {
          split: red.total.errors !== null,
          bars: red.buckets.map((bucket) => ({
            startTime: bucket.startTime,
            total: bucket.requests,
            errors: bucket.errors ?? 0,
            ...(bucket.p50Ns !== null && bucket.p95Ns !== null && bucket.p99Ns !== null
              ? { latency: { p50Ns: bucket.p50Ns, p95Ns: bucket.p95Ns, p99Ns: bucket.p99Ns } }
              : {}),
          })),
        };
      },
    };
  }, [counted]);

  const points = React.useMemo(
    () =>
      spans.map((span) => ({
        micros: Number.isFinite(span.startNs) && span.startNs > 0 ? Math.floor(span.startNs / 1_000) : null,
        error: span.status === 'ERROR',
      })),
    [spans],
  );

  const microsRange = React.useMemo(
    () => ({ start: Date.parse(range.from) * 1_000, end: Date.parse(range.to) * 1_000 }),
    [range.from, range.to],
  );

  const onRangeSelect = React.useCallback(
    ({ from, to }: { from: number; to: number }) =>
      onTimeWindowChange({ mode: 'absolute', from: new Date(from / 1_000).toISOString(), to: new Date(to / 1_000).toISOString() }),
    [onTimeWindowChange],
  );

  return { visible, onVisibleChange, points, range: microsRange, counts, onRangeSelect };
}
