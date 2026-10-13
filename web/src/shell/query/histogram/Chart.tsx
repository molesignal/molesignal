import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { useTimeFormatter } from '@/lib/time';

import { type AxisClock, utcOffsetMicros } from './axis';
import { type HistogramCountsSource, type HistogramUnavailable, useHistogramCounts } from './counts';
import {
  binHistogram,
  fitHistogramRange,
  type Histogram,
  histogramFromCounts,
  type HistogramPoint,
  type HistogramRange,
  planHistogram,
} from './model';
import { ASSUMED_PLOT_PIXELS, type HistogramLabels, HistogramPlot } from './Plot';
import { useWidth } from './width';

interface ResultHistogramProps {
  /** When each loaded result happened, and whether it is an error. */
  points: readonly HistogramPoint[];
  /** The range the results were asked from. */
  range: HistogramRange | null;
  /**
   * Where the counts of everything the query matches come from. With it the
   * bars span the range and count every match; without it, or when it has no
   * answer, they are of the loaded results.
   */
  counts?: HistogramCountsSource | undefined;
  /** A handful of results needs less of a chart. */
  short?: boolean;
  labels: HistogramLabels;
  onRangeSelect: (range: { from: number; to: number }) => void;
  /** The zone the view beside it shows times in, where that is not the reader's own. */
  timezone?: string | undefined;
  className?: string;
}

/** A bar with the gap beside it is at least this wide, so it can be pointed at. */
const MIN_BAR_PIXELS = 10;
const MAX_BARS = 120;
/**
 * What the chart keeps of its width for itself: its padding and the labels of
 * the count axis. The bars are settled from the width of the whole chart less
 * this, not from the plot, which narrows and widens with those labels: counts
 * are asked for the bars, and must not change what was asked.
 */
const AXIS_ALLOWANCE_PIXELS = 72;

/** What is drawn, and what it is of. */
interface Drawn {
  histogram: Histogram;
  /** The bars count everything the query matches, not only what is loaded. */
  all: boolean;
  split: boolean;
  /** The counts that were asked for are still on their way. */
  counting: boolean;
  /** The bars are those of the query before. */
  stale: boolean;
  /** Why the bars are of the loaded results when everything was asked for. */
  reason?: HistogramUnavailable | undefined;
}

/**
 * The histogram above an explorer's results: how many there are over time,
 * errors apart from the rest.
 *
 * Given a source of counts, it draws every result the query matches across the
 * range that was queried. The bars are settled here, for the width there is,
 * and the counts are asked for exactly those bars. Until they arrive the frame
 * stands empty, or keeps what it showed before.
 *
 * Without counts it draws the results that are loaded, and frames them by
 * their own span: a page of results is usually a sliver of the range it was
 * asked from. That is what a statement the reader wrote gets, and what is left
 * when the counts cannot be had; the line under the chart says which.
 */
export function ResultHistogram({
  points,
  range,
  counts,
  short = false,
  labels,
  onRangeSelect,
  timezone,
  className,
}: ResultHistogramProps) {
  const { t, i18n } = useTranslation('common');
  const time = useTimeFormatter({ timezone: timezone || undefined });
  const clock = React.useMemo<AxisClock>(
    () => ({ tz: time.tz, format: time.format, dateFormat: time.dateFormat }),
    [time.dateFormat, time.format, time.tz],
  );
  const [module, setModule] = React.useState<HTMLDivElement | null>(null);
  const moduleWidth = useWidth(module);
  const barPixels = moduleWidth ? moduleWidth - AXIS_ALLOWANCE_PIXELS : ASSUMED_PLOT_PIXELS;
  const maxBins = Math.max(1, Math.min(MAX_BARS, Math.floor(barPixels / MIN_BAR_PIXELS)));

  const loaded = React.useMemo(() => {
    const frame = fitHistogramRange(points, range);

    return binHistogram(points, frame, {
      maxBins,
      utcOffsetMicros: frame ? utcOffsetMicros(frame.start, clock.tz) : 0,
    });
  }, [clock.tz, maxBins, points, range]);
  const plan = React.useMemo(
    () =>
      counts && range && range.end > range.start
        ? planHistogram(range, { maxBins, utcOffsetMicros: utcOffsetMicros(range.start, clock.tz) })
        : null,
    [clock.tz, counts, maxBins, range],
  );
  // The bars are not settled until the chart has been measured.
  const { counted, pending } = useHistogramCounts(counts, range, plan, moduleWidth !== null);

  const drawn = React.useMemo((): Drawn | null => {
    const whole = { split: true, counting: false, stale: false };
    if (!plan) return loaded && { histogram: loaded, all: false, ...whole };

    const answer = counted?.answer;

    if (counted && answer && 'bars' in answer) {
      return {
        histogram: histogramFromCounts(counted.plan, answer.bars),
        all: true,
        split: answer.split,
        counting: pending,
        stale: pending,
      };
    }
    // While it waits, the chart keeps what it showed: the loaded results if the counts before
    // could not be had, an empty frame if there was nothing before.
    if (pending && !answer) {
      return { histogram: histogramFromCounts(plan, []), all: true, ...whole, counting: true };
    }

    const reason = !pending && answer && 'unavailable' in answer ? answer.unavailable : undefined;

    return loaded && { histogram: loaded, all: false, ...whole, counting: pending, reason };
  }, [counted, loaded, pending, plan]);

  // Results that carry no time cannot be drawn across it.
  if (!drawn) return null;

  const total = drawn.histogram.buckets.reduce((sum, bucket) => sum + bucket.ok + bucket.err, 0);
  const counting = drawn.counting ? 'counting' : drawn.all ? 'all' : 'loaded';
  const said = t(drawn.all ? 'histogram.scope_all' : 'histogram.scope_loaded', {
    count: total,
    total: total.toLocaleString(i18n.resolvedLanguage ?? i18n.language),
  });
  const scope = drawn.counting
    ? t('histogram.counting')
    : drawn.reason
      ? t('histogram.scope_because', { scope: said, reason: t(`histogram.unavailable.${drawn.reason}`) })
      : said;

  return (
    <HistogramPlot
      histogram={drawn.histogram}
      clock={clock}
      short={short}
      labels={labels}
      split={drawn.split}
      scope={scope}
      counting={counting}
      stale={drawn.stale}
      onRangeSelect={onRangeSelect}
      onModule={setModule}
      className={className}
    />
  );
}
