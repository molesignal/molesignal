import { Activity, type LucideIcon, Timer, TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { TrendPoint, VersionSummary } from '@/api/apm';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';
import type { TimeSeriesSeries } from '@/viz/timeseries/types';

import { bucketSeconds, deployAnnotations, errorRateSeries, latencySeries, requestSeries, shiftTrend } from './series';
import { ApmCard } from '../frame/Page';

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

  return (
    <div data-apm-red-charts="" className="grid min-w-0 gap-[12px] lg:grid-cols-3">
      <Chart icon={Activity} tone="text-[var(--chart-1)]" title={t('charts.requests')} series={requestSeries(trend, shifted, seconds, labels)} range={range} annotations={annotations} cursorScope={cursorScope} stacked bars />
      <Chart icon={TriangleAlert} tone="text-[var(--red-ink)]" title={t('charts.error_rate')} series={errorRateSeries(trend, shifted, labels)} range={range} annotations={annotations} cursorScope={cursorScope} />
      <Chart icon={Timer} tone="text-[var(--chart-3)]" title={t('charts.latency')} series={latencySeries(trend, shifted, labels)} range={range} annotations={annotations} cursorScope={cursorScope} legend />
    </div>
  );
}

function Chart({
  icon,
  tone,
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
  tone: string;
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
    <ApmCard icon={icon} tone={tone} title={title} testId={`chart-${series[0]?.id ?? 'red'}`} bodyClassName="px-2 pb-2 pt-1">
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
