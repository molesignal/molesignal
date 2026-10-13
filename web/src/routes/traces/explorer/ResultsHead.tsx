import { useTranslation } from 'react-i18next';

import type * as webApi from '@/api/web';
import { cn } from '@/shell/lib/cn';
import { TimezoneSelect } from '@/shell/TimezoneSelect';

import {
  TraceHistogramBand,
  TraceHistogramToggle,
  type TraceHistogramModel,
} from './Histogram';
import { TraceListSortSelect } from './shared';

/**
 * The head of the trace results, the same over both views of them: how many
 * are loaded, the order they are in, and the histogram of when they started,
 * across the full width of the results. Under it the Spans view sets the
 * traces beside the one that is open, and the Traces view lays them out as a
 * table.
 */
export function TraceResultsHead({
  loadedTraceCount,
  sort,
  sortEnabled,
  onSortChange,
  histogram,
  timezone,
  className,
  bandClassName,
}: {
  loadedTraceCount: number;
  sort: webApi.TraceListSort;
  sortEnabled: boolean;
  onSortChange: (sort: webApi.TraceListSort) => void;
  histogram: TraceHistogramModel;
  /** The zone the view shows times in, where the view lets it be changed. */
  timezone?: { value: string; onChange: (timezone: string) => void } | undefined;
  className?: string;
  bandClassName?: string;
}) {
  const { t } = useTranslation('traces');

  return (
    <div data-trace-results-head className={cn('shrink-0', className)}>
      <div className="flex min-h-11 items-center justify-between gap-3 px-3 py-1.5">
        <span className="min-w-0 truncate font-sans text-xs font-semibold text-tx-0">
          {t('explore.results.loaded_count', { count: loadedTraceCount })}
        </span>
        <div className="flex shrink-0 items-center gap-1.5">
          {sortEnabled ? <TraceListSortSelect value={sort} onChange={onSortChange} /> : null}
          {timezone ? (
            <TimezoneSelect value={timezone.value} onChange={timezone.onChange} className="h-8" />
          ) : null}
          <TraceHistogramToggle model={histogram} />
        </div>
      </div>
      <TraceHistogramBand
        model={histogram}
        timezone={timezone?.value}
        {...(bandClassName ? { className: bandClassName } : {})}
      />
    </div>
  );
}
