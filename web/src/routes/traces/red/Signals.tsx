import { Filter } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { TraceRedResponse } from '@/api/traceRed';
import { cn } from '@/shell/lib/cn';
import { QueryIconAction } from '@/shell/query/IconAction';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';
import type { TimeSeriesSeries } from '@/viz/timeseries/types';

import { formatDuration, formatErrorRate, formatRate, formatRequests, rateSuffix } from './format';
import { LatencyChart } from './LatencyChart';
import { errorRate, everyBar, latencySlots, rateUnitFor, type RedSignal } from './model';
import type { RedBars } from './useTraceRed';

export type DurationView = 'trend' | 'distribution';

const CHART_HEIGHT = 132;

/** The three percentiles of the duration, each in the colour its line is drawn in. */
const PERCENTILES = [
  { key: 'p50Ns', name: 'p50', color: 'var(--blue)' },
  { key: 'p95Ns', name: 'p95', color: 'var(--orange)' },
  { key: 'p99Ns', name: 'p99', color: 'var(--red)' },
] as const;

interface RedSignalsProps {
  overview: TraceRedResponse;
  bars: RedBars;
  /** The figure the breakdown is sorted and followed by. */
  signal: RedSignal;
  onSignalChange: (signal: RedSignal) => void;
  durationView: DurationView;
  onDurationViewChange: (view: DurationView) => void;
  /** The query already holds to failed requests: there is nothing narrower to offer. */
  failuresOnly: boolean;
  /** The figures are those of the query before, kept while the next ones are read. */
  stale: boolean;
  timezone: string;
  onRangeSelect: (range: { from: number; to: number }) => void;
  onFailuresOnly: () => void;
  onDurationSelect: (fromNs: number, toNs: number | null) => void;
}

/**
 * Rate, errors and duration side by side: each its figure for the range and
 * its course over it, in the same bars, the pointer at the same moment on all
 * three. Dragging across any of them narrows the time range; the errors offer
 * the failed requests alone, the duration the requests that took as long as
 * the bars dragged across.
 */
export function RedSignals({
  overview,
  bars,
  signal,
  onSignalChange,
  durationView,
  onDurationViewChange,
  failuresOnly,
  stale,
  timezone,
  onRangeSelect,
  onFailuresOnly,
  onDurationSelect,
}: RedSignalsProps) {
  const { t, i18n } = useTranslation('traces');
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const { total } = overview;
  const bucketSeconds = bars.bucketTime / 1_000_000;
  const rangeSeconds = Math.max(1, (bars.count * bars.bucketTime) / 1_000_000);
  const timestamps = React.useMemo(
    () => Array.from({ length: bars.count }, (_, index) => bars.originTime + index * bars.bucketTime),
    [bars],
  );
  const perSecond = React.useMemo(
    () => everyBar(overview.buckets, bars, (bucket) => bucket.requests / bucketSeconds, 0),
    [bars, bucketSeconds, overview.buckets],
  );
  const unit = rateUnitFor(perSecond);
  const xDomain: [number, number] = [bars.originTime, bars.originTime + bars.count * bars.bucketTime];
  const failed = errorRate(total);

  const rateSeries = React.useMemo<TimeSeriesSeries[]>(
    () => [
      {
        id: 'red-rate',
        name: t('explore.red.rate.title'),
        color: 'var(--indigo)',
        data: perSecond.map((value) => value * unit.seconds),
        timestamps,
        unit: rateSuffix(unit),
      },
    ],
    [perSecond, t, timestamps, unit],
  );
  const errorSeries = React.useMemo<TimeSeriesSeries[]>(
    () => [
      {
        id: 'red-error-rate',
        name: t('explore.red.errors.rate'),
        color: 'var(--red)',
        data: everyBar(overview.buckets, bars, (bucket) => errorRate(bucket), null),
        timestamps,
        unit: 'percentunit',
      },
    ],
    [bars, overview.buckets, t, timestamps],
  );
  const durationSeries = React.useMemo<TimeSeriesSeries[]>(
    () =>
      PERCENTILES.map(({ key, name, color }) => ({
        id: `red-${name}`,
        name,
        color,
        data: everyBar(overview.buckets, bars, (bucket) => bucket[key], null),
        timestamps,
        unit: 'ns',
      })),
    [bars, overview.buckets, timestamps],
  );
  const slots = React.useMemo(() => latencySlots(overview.latency), [overview.latency]);
  const split = overview.latency.every((bar) => bar.errors !== null);

  const chart = (series: TimeSeriesSeries[], name: string, options: { bars?: boolean; percent?: boolean }) => (
    <TimeSeriesChart
      series={series}
      height={CHART_HEIGHT}
      xDomain={xDomain}
      timezone={timezone}
      // The three move together: the pointer stands at one moment on all of them.
      cursorScopeId="traces-red"
      // The figures over the chart name what it draws; a legend would take its height.
      showLegend={false}
      ariaLabel={name}
      rangeSelectionAriaLabel={t('explore.red.select_range')}
      onRangeSelect={onRangeSelect}
      options={{
        drawStyle: options.bars ? 'bar' : 'line',
        showPoints: 'never',
        tooltipMode: 'all',
        compactAxes: true,
        leftAxis: options.percent ? { min: 0, softMax: 0.05, unit: 'percentunit' } : { min: 0 },
      }}
    />
  );

  return (
    <div
      data-red-signals
      className={cn('grid gap-[8px] [@container_(min-width:960px)]:grid-cols-3', stale && 'opacity-60')}
    >
      <SignalCard
        signal="rate"
        active={signal === 'rate'}
        onActivate={onSignalChange}
        title={t('explore.red.rate.title')}
        value={formatRate(total.requests / rangeSeconds, unit, locale)}
        detail={t('explore.red.rate.detail', { count: total.requests, requests: formatRequests(total.requests, locale) })}
      >
        {chart(rateSeries, t('explore.red.rate.aria'), { bars: true })}
      </SignalCard>
      <SignalCard
        signal="errors"
        active={signal === 'errors'}
        onActivate={onSignalChange}
        title={t('explore.red.errors.title')}
        value={formatErrorRate(failed)}
        tone={failed === null || failed === 0 ? undefined : failed >= 0.05 ? 'text-red' : 'text-yellow'}
        detail={
          total.errors === null
            ? t('explore.red.errors.untold')
            : t('explore.red.errors.detail', { count: total.errors, errors: formatRequests(total.errors, locale) })
        }
        action={
          failuresOnly || !total.errors ? null : (
            <QueryIconAction icon={Filter} label={t('explore.red.errors.only')} onClick={onFailuresOnly} />
          )
        }
      >
        {chart(errorSeries, t('explore.red.errors.aria'), { percent: true })}
      </SignalCard>
      <SignalCard
        signal="duration"
        active={signal === 'duration'}
        onActivate={onSignalChange}
        title={t('explore.red.duration.title')}
        value={`p95 ${formatDuration(total.p95Ns)}`}
        // Each figure carries the colour of its line: the headline is the 95th, the other two follow it.
        marker={PERCENTILES[1].color}
        detail={
          <span className="inline-flex min-w-0 items-center gap-2.5">
            {PERCENTILES.filter(({ name }) => name !== 'p95').map(({ key, name, color }) => (
              <span key={key} className="inline-flex shrink-0 items-center gap-1">
                <span aria-hidden="true" className="h-1.5 w-1.5 rounded-full" style={{ backgroundColor: color }} />
                {name} {formatDuration(total[key])}
              </span>
            ))}
          </span>
        }
        action={
          <ViewTabs<DurationView>
            label={t('explore.red.duration.view_label')}
            items={[
              { key: 'trend', label: t('explore.red.duration.trend') },
              { key: 'distribution', label: t('explore.red.duration.distribution') },
            ]}
            value={durationView}
            onValueChange={onDurationViewChange}
          />
        }
      >
        {durationView === 'trend' ? (
          chart(durationSeries, t('explore.red.duration.aria'), {})
        ) : (
          <LatencyChart
            slots={slots}
            split={split}
            p95={total.p95Ns}
            p99={total.p99Ns}
            labels={{
              chart: t('explore.red.duration.distribution_name'),
              ok: t('explore.red.ok'),
              errors: t('explore.red.errors.title'),
            }}
            onSelect={onDurationSelect}
          />
        )}
      </SignalCard>
    </div>
  );
}

function SignalCard({
  signal,
  active,
  onActivate,
  title,
  value,
  marker,
  tone,
  detail,
  action,
  children,
}: {
  signal: RedSignal;
  active: boolean;
  onActivate: (signal: RedSignal) => void;
  title: string;
  value: string;
  /** The colour of the line the figure is drawn as, where the chart draws several. */
  marker?: string | undefined;
  /** The figure's colour where it says something is wrong. */
  tone?: string | undefined;
  detail: React.ReactNode;
  action?: React.ReactNode;
  children: React.ReactNode;
}) {
  const { t } = useTranslation('traces');

  return (
    <section
      data-red-signal={signal}
      data-active={active || undefined}
      aria-label={title}
      className="flex min-w-0 flex-col rounded-md bg-[var(--functional-surface)] px-4 pb-3 pt-2"
    >
      <div className="flex h-8 items-center justify-between gap-2">
        {/* The figure in front orders the breakdown: pressing another title puts it there. */}
        <button
          type="button"
          aria-pressed={active}
          title={t('explore.red.lead_hint')}
          onClick={() => onActivate(signal)}
          className={cn(
            '-ml-2 inline-flex h-7 items-center rounded-md px-2 font-sans text-xs outline-none transition-colors',
            active
              ? 'bg-indigo-dim font-strong text-indigo-soft'
              : 'font-medium text-tx-1 hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0',
          )}
        >
          {title}
        </button>
        {action}
      </div>
      <div className="flex min-w-0 items-baseline gap-2">
        <span
          data-red-figure
          className={cn('type-heading inline-flex shrink-0 items-center gap-1.5 font-strong tabular-nums', tone ?? 'text-tx-0')}
        >
          {marker && <span aria-hidden="true" className="h-2 w-2 rounded-full" style={{ backgroundColor: marker }} />}
          {value}
        </span>
        <span className="type-caption min-w-0 truncate text-tx-2">{detail}</span>
      </div>
      <div className="mt-2 min-w-0">{children}</div>
    </section>
  );
}
