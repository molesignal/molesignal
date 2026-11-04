import { BarChart3 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as webApi from '@/api/web';
import { ResultHistogram } from '@/shell/query/histogram/Chart';
import { answerFromPayload, type HistogramCountsSource } from '@/shell/query/histogram/counts';
import type { HistogramPoint, HistogramRange } from '@/shell/query/histogram/model';
import { ModuleToggle } from '@/shell/query/ModuleToggle';
import { useExplorerLayoutStore } from '@/stores/useExplorerLayoutStore';
import type { TimeWindow } from '@/stores/useTimeStore';

import type { DisplayTrace } from './types';

/** What the two result views need to draw the trace histogram and its switch. */
export interface TraceHistogramModel {
  visible: boolean;
  onVisibleChange: (visible: boolean) => void;
  points: HistogramPoint[];
  /** The range the traces were asked from. */
  range: HistogramRange;
  /** Where the counts of every matching trace come from, when the list query can be counted. */
  counts: HistogramCountsSource | undefined;
  onRangeSelect: (range: { from: number; to: number }) => void;
}

/** The list query whose matches the histogram counts. */
export interface CountedTraceQuery {
  orgId: string;
  q?: string | undefined;
  filters: webApi.TraceFilter[];
  where?: webApi.TraceCondition | undefined;
  /** Goes up when the reader asks for the results again. */
  epoch: number;
}

/**
 * The trace histogram as a module of the explorer: when the traces the query
 * matches started, the ones with errors apart from the rest. Like the Logs
 * histogram it can be closed, the choice is remembered, and dragging across it
 * narrows the time range.
 *
 * It counts every trace the list query matches in the range, which is asked of
 * the backend with the query the list itself sends. A statement has no list
 * query (`counted` is `null`): its histogram is of the traces it returned.
 */
export function useTraceHistogram({
  traces,
  range,
  onTimeWindowChange,
  counted,
}: {
  traces: DisplayTrace[];
  range: { from: string; to: string };
  onTimeWindowChange: (window: TimeWindow) => void;
  counted: CountedTraceQuery | null;
}): TraceHistogramModel {
  const visible = useExplorerLayoutStore((state) => state.tracesHistogramVisible);
  const onVisibleChange = useExplorerLayoutStore((state) => state.setTracesHistogramVisible);
  const counts = React.useMemo<HistogramCountsSource | undefined>(() => {
    if (!counted) return undefined;

    const { orgId, q, filters, where, epoch } = counted;

    return {
      key: ['traces', orgId, q ?? '', JSON.stringify(filters), JSON.stringify(where ?? null), epoch],
      load: async ({ range: asked, binMicros, originTime: originMicros }, signal) =>
        answerFromPayload(
          await webApi.tracesHistogram(
            {
              from: asked.start,
              to: asked.end,
              ...(q ? { q } : {}),
              ...(filters.length > 0 ? { filters } : {}),
              ...(where ? { where } : {}),
              bucketTime: binMicros,
              originTime: originMicros,
            },
            signal,
          ),
        ),
    };
  }, [counted]);

  const points = React.useMemo(
    () =>
      traces.map((trace) => ({
        micros:
          Number.isFinite(trace.startNs) && trace.startNs > 0
            ? Math.floor(trace.startNs / 1_000)
            : null,
        error: trace.errors > 0,
      })),
    [traces],
  );
  const microsRange = React.useMemo(
    () => ({ start: Date.parse(range.from) * 1_000, end: Date.parse(range.to) * 1_000 }),
    [range.from, range.to],
  );
  const onRangeSelect = React.useCallback(
    ({ from, to }: { from: number; to: number }) =>
      onTimeWindowChange({
        mode: 'absolute',
        from: new Date(from / 1_000).toISOString(),
        to: new Date(to / 1_000).toISOString(),
      }),
    [onTimeWindowChange],
  );

  return { visible, onVisibleChange, points, range: microsRange, counts, onRangeSelect };
}

/** The switch for the results header. */
export function TraceHistogramToggle({ model }: { model: TraceHistogramModel }) {
  const { t } = useTranslation('traces');

  return (
    <ModuleToggle
      size="sm"
      open={model.visible}
      icon={BarChart3}
      actionLabel={model.visible ? t('explore.histogram.hide') : t('explore.histogram.show')}
      onOpenChange={model.onVisibleChange}
    />
  );
}

/**
 * The band under the results header, as wide as the results. Without traces
 * there is nothing to draw, so nothing is drawn.
 */
export function TraceHistogramBand({
  model,
  latency = false,
  unit = 'traces',
  className,
}: {
  model: TraceHistogramModel;
  /** How long the traces of each bar took is drawn beside the bars. */
  latency?: boolean;
  /** What the bars count. */
  unit?: 'traces' | 'spans';
  className?: string;
}) {
  const { t } = useTranslation('traces');

  if (!model.visible || model.points.length === 0) return null;

  return (
    <ResultHistogram
      points={model.points}
      range={model.range}
      counts={model.counts}
      short
      labels={{
        chart: t(unit === 'spans' ? 'explore.histogram.span_label' : 'explore.histogram.label'),
        ok: t(unit === 'spans' ? 'explore.histogram.span_ok' : 'explore.histogram.ok'),
        errors: t(unit === 'spans' ? 'explore.histogram.span_errors' : 'explore.histogram.errors'),
      }}
      {...(latency
        ? { latency: { chart: t('explore.histogram.latency_label'), p50: 'p50', p95: 'p95', p99: 'p99' } }
        : {})}
      onRangeSelect={model.onRangeSelect}
      {...(className ? { className } : {})}
    />
  );
}
