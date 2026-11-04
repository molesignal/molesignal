import type { TrendPoint, VersionSummary } from '@/api/apm';
import type { TimeSeriesAnnotation, TimeSeriesSeries } from '@/viz/timeseries/types';

/** How long one bucket of a trend is, in seconds. */
export function bucketSeconds(resolution: 'minute' | 'hour'): number {
  return resolution === 'hour' ? 3_600 : 60;
}

/** A compared trend moved onto the time in view, so the two draw over one axis. */
export function shiftTrend(trend: readonly TrendPoint[], shiftMicros: number): TrendPoint[] {
  return trend.map((point) => ({
    ...point,
    bucketAt: point.bucketAt + shiftMicros,
  }));
}

export interface RedSeriesLabels {
  ok: string;
  errors: string;
  previous: string;
  errorRate: string;
  p50: string;
  p95: string;
  p99: string;
}

const DASH = [4, 3] as const;

/** Requests a second in each bucket, those that failed apart; the compared total as a dashed line. */
export function requestSeries(
  trend: readonly TrendPoint[],
  compared: readonly TrendPoint[] | null,
  seconds: number,
  labels: RedSeriesLabels,
): TimeSeriesSeries[] {
  const timestamps = trend.map((point) => point.bucketAt);
  const series: TimeSeriesSeries[] = [
    {
      id: 'ok',
      name: labels.ok,
      timestamps,
      data: trend.map((point) => (point.red.requestCount - point.red.errorCount) / seconds),
      color: 'var(--chart-1)',
      unit: 'req/s',
    },
    {
      id: 'errors',
      name: labels.errors,
      timestamps,
      data: trend.map((point) => point.red.errorCount / seconds),
      color: 'var(--red)',
      unit: 'req/s',
    },
  ];

  if (compared?.length) {
    series.push({
      id: 'previous',
      name: labels.previous,
      timestamps: compared.map((point) => point.bucketAt),
      data: compared.map((point) => point.red.requestCount / seconds),
      color: 'var(--tx-3)',
      unit: 'req/s',
      dash: DASH,
    });
  }

  return series;
}

export function errorRateSeries(trend: readonly TrendPoint[], compared: readonly TrendPoint[] | null, labels: RedSeriesLabels): TimeSeriesSeries[] {
  const rate = (point: TrendPoint) => (point.red.requestCount > 0 ? point.red.errorCount / point.red.requestCount : null);
  const series: TimeSeriesSeries[] = [
    {
      id: 'error_rate',
      name: labels.errorRate,
      timestamps: trend.map((point) => point.bucketAt),
      data: trend.map(rate),
      color: 'var(--red)',
      unit: 'percentunit',
    },
  ];

  if (compared?.length) {
    series.push({
      id: 'previous',
      name: labels.previous,
      timestamps: compared.map((point) => point.bucketAt),
      data: compared.map(rate),
      color: 'var(--tx-3)',
      unit: 'percentunit',
      dash: DASH,
    });
  }

  return series;
}

/** P50, P95 and P99; the compared P95 dashed. */
export function latencySeries(trend: readonly TrendPoint[], compared: readonly TrendPoint[] | null, labels: RedSeriesLabels): TimeSeriesSeries[] {
  const timestamps = trend.map((point) => point.bucketAt);
  const micros = (value: number | undefined) => value ?? null;
  const series: TimeSeriesSeries[] = [
    {
      id: 'p50',
      name: labels.p50,
      timestamps,
      data: trend.map((point) => micros(point.red.p50Time)),
      color: 'var(--chart-2)',
      unit: 'us',
    },
    {
      id: 'p95',
      name: labels.p95,
      timestamps,
      data: trend.map((point) => micros(point.red.p95Time)),
      color: 'var(--chart-1)',
      unit: 'us',
    },
    {
      id: 'p99',
      name: labels.p99,
      timestamps,
      data: trend.map((point) => micros(point.red.p99Time)),
      color: 'var(--chart-4)',
      unit: 'us',
    },
  ];

  if (compared?.length) {
    series.push({
      id: 'previous',
      name: labels.previous,
      timestamps: compared.map((point) => point.bucketAt),
      data: compared.map((point) => micros(point.red.p95Time)),
      color: 'var(--tx-3)',
      unit: 'us',
      dash: DASH,
    });
  }

  return series;
}

/** A mark where a version was first seen within the time in view: a deploy. */
export function deployAnnotations(versions: readonly VersionSummary[], range: { from: number; to: number }): TimeSeriesAnnotation[] {
  return versions
    .filter((version) => version.firstSeenAt >= range.from && version.firstSeenAt <= range.to)
    .map((version) => ({
      id: `deploy-${version.version}`,
      timestamp: version.firstSeenAt,
      label: version.version,
      color: 'var(--indigo)',
    }));
}
