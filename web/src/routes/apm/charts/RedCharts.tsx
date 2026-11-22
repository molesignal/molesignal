import { Activity, type LucideIcon, Timer, TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { TrendPoint, VersionSummary } from '@/api/apm';
import type { BaselineFigures } from '@/api/apmInsights';
import { cn } from '@/shell/lib/cn';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';
import type { TimeSeriesSeries } from '@/viz/timeseries/types';

import { bucketSeconds, deployAnnotations, errorRateSeries, latencySeries, requestSeries, shiftTrend } from './series';
import { formatDuration, formatRate, formatThroughput } from '../format';
import { ApmCard } from '../frame/Page';

interface Headline {
  value: string;
  change?: { text: string; tone: 'worse' | 'better' | 'neutral' } | undefined;
}

/** A relative change, with its arrow: `↑3.0%`. */
function relativeChange(now: number | undefined, before: number | undefined): string | undefined {
  if (now === undefined || before === undefined || before === 0) return undefined;
  const change = (now - before) / before;
  return `${change >= 0 ? '↑' : '↓'}${Math.abs(change * 100).toFixed(1)}%`;
}

/** Up is worse for errors and latency, down is better; a flat change says nothing either way. */
function toneOf(now: number | undefined, before: number | undefined): 'worse' | 'better' | 'neutral' {
  if (now === undefined || before === undefined || now === before) return 'neutral';
  return now > before ? 'worse' : 'better';
}

/**
 * Requests, errors and latency side by side over the time in view, as
 * Datadog opens a service: the requests that failed stacked on those that
 * did not, the error rate, and P50 / P95 / P99. A comparison draws the same
 * stretch a day or a week before as a dashed line; a deploy is a mark.
 */
export function RedCharts({
  trend,
  compared,
  shiftMicros,
  resolution,
  versions,
  range,
  cursorScope = 'main',
  comparedLabel,
  figures,
}: {
  trend: readonly TrendPoint[];
  compared?: readonly TrendPoint[] | null | undefined;
  /** How far back the compared stretch lies. */
  shiftMicros: number;
  resolution: 'minute' | 'hour';
  versions: readonly VersionSummary[];
  range: { from: number; to: number };
  /** The charts that move their lines in step with these: a drawer's keep among themselves, apart from the page under it. */
  cursorScope?: string;
  /** What the compared line is called when it is not the stretch before: another environment. */
  comparedLabel?: string | undefined;
  /** The window's figures and the stretch before it, for each chart's head: its value now and how it moved. */
  figures?: { now: BaselineFigures; previous?: BaselineFigures | undefined } | undefined;
}) {
  const { t } = useTranslation('apm');
  const labels = {
    ok: t('charts.ok'),
    errors: t('charts.errors'),
    previous: comparedLabel ?? t('charts.previous'),
    errorRate: t('charts.error_rate'),
    p50: 'P50',
    p95: 'P95',
    p99: 'P99',
  };
  const shifted = compared?.length ? shiftTrend(compared, shiftMicros) : null;
  const seconds = bucketSeconds(resolution);
  const annotations = deployAnnotations(versions, range);
  const now = figures?.now;
  const before = figures?.previous;
  const points = (value: number) => t('charts.points', { value: `${value >= 0 ? '↑' : '↓'}${Math.abs(value * 100).toFixed(1)}` });
  const headlines: Record<'requests' | 'errors' | 'latency', Headline | undefined> = now
    ? {
        requests: {
          value: formatThroughput(now.throughput),
          change: relativeChange(now.throughput, before?.throughput)
            ? { text: relativeChange(now.throughput, before?.throughput)!, tone: 'neutral' }
            : undefined,
        },
        errors: {
          value: formatRate(now.errorRate),
          change: before ? { text: points(now.errorRate - before.errorRate), tone: toneOf(now.errorRate, before.errorRate) } : undefined,
        },
        latency: now.p95Time === undefined
          ? undefined
          : {
              value: `P95 ${formatDuration(now.p95Time)}`,
              change: relativeChange(now.p95Time, before?.p95Time)
                ? { text: relativeChange(now.p95Time, before?.p95Time)!, tone: toneOf(now.p95Time, before?.p95Time) }
                : undefined,
            },
      }
    : { requests: undefined, errors: undefined, latency: undefined };

  return (
    <div data-apm-red-charts="" className="grid min-w-0 gap-[8px] lg:grid-cols-3">
      <Chart icon={Activity} headline={headlines.requests} changeLabel={t('charts.vs_previous')} title={t('charts.requests')} series={requestSeries(trend, shifted, seconds, labels)} range={range} annotations={annotations} cursorScope={cursorScope} stacked bars />
      <Chart icon={TriangleAlert} headline={headlines.errors} changeLabel={t('charts.vs_previous')} title={t('charts.error_rate')} series={errorRateSeries(trend, shifted, labels)} range={range} annotations={annotations} cursorScope={cursorScope} />
      <Chart icon={Timer} headline={headlines.latency} changeLabel={t('charts.vs_previous')} title={t('charts.latency')} series={latencySeries(trend, shifted, labels)} range={range} annotations={annotations} cursorScope={cursorScope} legend />
    </div>
  );
}

function Chart({
  icon,
  headline,
  changeLabel,
  title,
  series,
  range,
  annotations,
  cursorScope,
  stacked = false,
  bars = false,
  legend = false,
}: {
  icon: LucideIcon;
  headline?: Headline | undefined;
  changeLabel: string;
  title: string;
  series: TimeSeriesSeries[];
  range: { from: number; to: number };
  annotations: ReturnType<typeof deployAnnotations>;
  cursorScope: string;
  stacked?: boolean;
  bars?: boolean;
  legend?: boolean;
}) {
  const unit = series[0]?.unit;

  return (
    <ApmCard
      icon={icon}
      title={title}
      testId={`chart-${series[0]?.id ?? 'red'}`}
      bodyClassName="px-2 pb-2 pt-1"
      actions={
        headline && (
          <span data-apm-chart-headline className="flex items-baseline gap-1.5 font-sans tabular-nums">
            <span className="text-md font-display text-tx-0">{headline.value}</span>
            {headline.change && (
              <span
                title={changeLabel}
                className={cn(
                  'text-xs',
                  headline.change.tone === 'worse'
                    ? 'text-[var(--red-ink)]'
                    : headline.change.tone === 'better'
                      ? 'text-[var(--green-ink)]'
                      : 'text-tx-3',
                )}
              >
                {headline.change.text}
              </span>
            )}
          </span>
        )
      }
    >
      <TimeSeriesChart
        series={series}
        xDomain={[range.from, range.to]}
        height={180}
        cursorScopeId={cursorScope}
        ariaLabel={title}
        showLegend={legend || series.some((entry) => entry.id === 'previous')}
        legendDensity="compact"
        options={{
          drawStyle: bars ? 'bar' : 'line',
          legendMode: 'list',
          stackMode: stacked ? 'normal' : 'none',
          showPoints: 'never',
          tooltipMode: 'all',
          compactAxes: true,
          leftAxis: { scale: 'linear', min: 0, ...(unit ? { unit } : {}) },
          annotations,
        }}
      />
    </ApmCard>
  );
}
