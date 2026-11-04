import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { QuerySplitToggle } from '@/shell/queryTabs';

import {
  TraceHistogramBand,
  TraceHistogramToggle,
  type TraceHistogramModel,
} from './Histogram';

/**
 * The head of the trace results, the same over both views of them: how many
 * are loaded, the order they are in, and the histogram of when they started,
 * across the full width of the results. The switch that splits the page leads
 * its signs. Under it the Spans view sets the traces beside the one that is
 * open, and the Traces view lays them out as a table.
 */
export function TraceResultsHead({
  loadedTraceCount,
  summary,
  histogram,
  actions,
  latency = false,
  unit = 'traces',
  className,
  bandClassName,
}: {
  loadedTraceCount: number;
  /** What the head says of the results, in place of how many traces are loaded. */
  summary?: string | undefined;
  histogram: TraceHistogramModel;
  /** The view's own controls, before the histogram's switch. */
  actions?: React.ReactNode;
  /** How long the traces took is drawn beside the histogram. */
  latency?: boolean;
  /** What the histogram counts. */
  unit?: 'traces' | 'spans';
  className?: string;
  bandClassName?: string;
}) {
  const { t } = useTranslation('traces');

  return (
    <div data-trace-results-head className={cn('shrink-0', className)}>
      <div className="flex min-h-11 items-center justify-between gap-3 px-3 py-1.5">
        <span className="min-w-0 truncate font-sans text-xs font-semibold text-tx-0">
          {summary ?? t('explore.results.loaded_count', { count: loadedTraceCount })}
        </span>
        <div className="flex shrink-0 items-center gap-1.5">
          <QuerySplitToggle size="sm" />
          {actions}
          <TraceHistogramToggle model={histogram} />
        </div>
      </div>
      <TraceHistogramBand
        model={histogram}
        latency={latency}
        unit={unit}
        {...(bandClassName ? { className: bandClassName } : {})}
      />
    </div>
  );
}
