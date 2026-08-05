import { ArrowRight, CircleDot, RefreshCw } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { KpiStrip } from '@/admin';
import type { ApmMeta, RedSummary, TimeRange, TrendPoint } from '@/api/apm';
import { ProductState } from '@/product/states';
import { cn } from '@/shell/lib/cn';
import { PageBody, PageHeader } from '@/shell/PageHeader';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';
import type { TimeSeriesSeries } from '@/viz/timeseries/types';

import { DataQualityNotice } from './DataQualityNotice';
import {
  formatCount,
  formatDuration,
  formatRate,
  formatThroughput,
} from './format';
import { ApmNavigation } from './Layout';

export function RedKpis({
  red,
  trend,
  resolution,
}: {
  red: RedSummary;
  trend: TrendPoint[];
  resolution: ApmMeta['resolution'];
}) {
  const { t } = useTranslation('apm');
  const throughput = averageThroughput(red.request_count, trend, resolution);
  return (
    <KpiStrip
      items={[
        {
          label: t('metrics.throughput'),
          value: formatThroughput(throughput),
          sub: t('metrics.total_requests', {
            count: formatCount(red.request_count),
          }),
        },
        {
          label: t('metrics.error_rate'),
          value: formatRate(red.error_rate),
          sub: `${formatCount(red.error_count)} ${t('metrics.errors').toLowerCase()}`,
          tone: red.error_rate >= 0.1 ? 'danger' : red.error_rate >= 0.02 ? 'warn' : 'good',
        },
        {
          label: t('metrics.p95'),
          value: formatDuration(red.p95_micros),
          sub: t('metrics.merged_histogram'),
        },
        {
          label: t('metrics.p99'),
          value: formatDuration(red.p99_micros),
          sub: t('metrics.merged_histogram'),
        },
      ]}
    />
  );
}

type TrendMetric = 'throughput' | 'latency' | 'error_rate';

export function TrendStrip({
  points,
  range,
  resolution,
}: {
  points: TrendPoint[];
  range: TimeRange;
  resolution: ApmMeta['resolution'];
}) {
  const { t } = useTranslation('apm');
  const [metric, setMetric] = React.useState<TrendMetric>('throughput');
  const bucketSeconds = resolution === 'hour' ? 3_600 : 60;
  const timestamps = points.map((point) => point.bucket_at);
  let series: TimeSeriesSeries[];
  let drawStyle: 'bar' | 'line';
  let axis: { min: number; label: string; unit?: string; softMax?: number };
  let ariaLabel: string;

  if (metric === 'latency') {
    series = [
      {
        id: 'apm-p50-latency',
        name: t('metrics.p50'),
        color: 'var(--blue)',
        data: points.map((point) => point.red.p50_micros ?? null),
        timestamps,
        unit: 'us',
      },
      {
        id: 'apm-p95-latency',
        name: t('metrics.p95'),
        color: 'var(--orange)',
        data: points.map((point) => point.red.p95_micros ?? null),
        timestamps,
        unit: 'us',
      },
      {
        id: 'apm-p99-latency',
        name: t('metrics.p99'),
        color: 'var(--red)',
        data: points.map((point) => point.red.p99_micros ?? null),
        timestamps,
        unit: 'us',
      },
    ];
    drawStyle = 'line';
    axis = { min: 0, label: t('trend.latency'), unit: 'us' };
    ariaLabel = t('trend.latency_aria');
  } else if (metric === 'error_rate') {
    series = [
      {
        id: 'apm-error-rate',
        name: t('metrics.error_rate'),
        color: 'var(--red)',
        data: points.map((point) => point.red.error_rate),
        timestamps,
        unit: 'percentunit',
      },
    ];
    drawStyle = 'line';
    axis = {
      min: 0,
      softMax: 0.1,
      label: t('metrics.error_rate'),
      unit: 'percentunit',
    };
    ariaLabel = t('trend.error_rate_aria');
  } else {
    series = [
      {
        id: 'apm-throughput',
        name: t('metrics.throughput'),
        color: 'var(--indigo)',
        data: points.map((point) => point.red.request_count / bucketSeconds),
        timestamps,
        unit: 'req/s',
      },
    ];
    drawStyle = 'bar';
    axis = { min: 0, label: t('metrics.throughput'), unit: 'req/s' };
    ariaLabel = t('trend.throughput_aria');
  }

  return (
    <section className="rounded-lg border border-bd-0 bg-bg-1 p-4">
      <div className="mb-4 flex flex-col items-start gap-3 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h2 className="type-section-title font-strong text-tx-0">{t('trend.title')}</h2>
          <p className="mt-1 text-xs text-tx-2">
            {t(`trend.${metric}_subtitle`)}
          </p>
        </div>
        <div
          role="group"
          aria-label={t('trend.metric_selector')}
          className="inline-flex w-full rounded-md border border-bd-0 bg-bg-2 p-0.5 sm:w-auto"
        >
          {(['throughput', 'latency', 'error_rate'] as const).map((item) => (
            <button
              key={item}
              type="button"
              aria-pressed={metric === item}
              onClick={() => setMetric(item)}
              className={cn(
                'min-h-11 flex-1 rounded px-3 text-sm outline-none transition-colors sm:min-h-8 sm:flex-none sm:text-xs',
                metric === item
                  ? 'bg-bg-4 font-strong text-tx-0'
                  : 'text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0',
              )}
            >
              {t(`trend.${item}`)}
            </button>
          ))}
        </div>
      </div>
      {points.length === 0 ? (
        <div className="grid h-28 place-items-center text-xs text-tx-3">
          {t('states.no_trend')}
        </div>
      ) : (
        <TimeSeriesChart
          series={series}
          xDomain={[range.from, range.to]}
          height={190}
          ariaLabel={ariaLabel}
          showLegend={metric === 'latency'}
          legendDensity="compact"
          options={{
            drawStyle,
            showPoints: 'never',
            tooltipMode: metric === 'latency' ? 'all' : 'single',
            compactAxes: true,
            leftAxis: axis,
          }}
        />
      )}
      <div className="mt-2 text-right font-mono text-xs text-tx-3">
        {points.length} {t('trend.buckets')}
      </div>
    </section>
  );
}

export function averageThroughput(
  requestCount: number,
  points: TrendPoint[],
  resolution: ApmMeta['resolution'],
): number | undefined {
  if (requestCount === 0) return 0;
  if (points.length === 0) return undefined;
  const bucketMicros = resolution === 'hour' ? 3_600_000_000 : 60_000_000;
  let first = Number.POSITIVE_INFINITY;
  let last = Number.NEGATIVE_INFINITY;
  for (const point of points) {
    first = Math.min(first, point.bucket_at);
    last = Math.max(last, point.bucket_at);
  }
  const observedMicros = Math.max(bucketMicros, last - first + bucketMicros);
  return requestCount / (observedMicros / 1_000_000);
}

export function Section({
  title,
  description,
  action,
  children,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  action?: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section className="overflow-hidden rounded-lg border border-bd-0 bg-bg-1">
      <div className="flex min-h-14 items-center justify-between gap-4 border-b border-bd-0 px-4 py-3">
        <div>
          <h2 className="type-section-title font-strong text-tx-0">{title}</h2>
          {description && <p className="mt-0.5 text-xs text-tx-2">{description}</p>}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}

export function SectionLink({ to, label }: { to: string; label: string }) {
  return (
    <Link
      to={to}
      className="inline-flex items-center gap-1 rounded px-2 py-1 text-xs font-strong text-indigo-soft outline-none hover:bg-bg-2 focus-visible:bg-bg-2"
    >
      {label}
      <ArrowRight aria-hidden className="h-3 w-3" />
    </Link>
  );
}

export function TraceIdLink({
  traceId,
  spanId,
  children,
  className,
}: {
  traceId: string;
  spanId?: string;
  children?: React.ReactNode;
  className?: string;
}) {
  const { t } = useTranslation('apm');
  const search = new URLSearchParams();
  if (spanId) search.set('spanId', spanId);
  const query = search.toString();
  return (
    <Link
      to={`/traces/${encodeURIComponent(traceId)}${query ? `?${query}` : ''}`}
      title={traceId}
      aria-label={`${t('actions.open_trace')}: ${traceId}`}
      className={cn(
        'inline-flex min-w-0 items-center rounded-sm font-mono text-indigo-soft underline decoration-dotted decoration-1 underline-offset-2 outline-none',
        'hover:text-indigo hover:decoration-solid focus-visible:bg-bg-2 focus-visible:text-indigo',
        className,
      )}
    >
      <span className="truncate">{children ?? traceId}</span>
    </Link>
  );
}

export function QueryBoundary({
  pending,
  error,
  empty,
  filtered,
  refetching,
  onRetry,
  children,
}: {
  pending: boolean;
  error: unknown;
  empty: boolean;
  filtered?: boolean;
  refetching?: boolean;
  onRetry: () => void;
  children: React.ReactNode;
}) {
  const { t } = useTranslation('apm');
  if (pending) return <ProductState variant="loading" />;
  if (error) {
    const forbidden =
      typeof error === 'object' &&
      error !== null &&
      'response' in error &&
      (error as { response?: { status?: number } }).response?.status === 403;
    return (
      <ProductState
        variant={forbidden ? 'permission-denied' : 'error'}
        error={error}
        action={
          forbidden ? undefined : (
            <button
              type="button"
              onClick={onRetry}
              className="inline-flex h-8 items-center gap-1.5 rounded-md bg-indigo px-3 text-xs font-strong text-white outline-none hover:bg-indigo-soft focus-visible:bg-indigo-soft"
            >
              <RefreshCw aria-hidden className="h-3.5 w-3.5" />
              {t('actions.retry')}
            </button>
          )
        }
      />
    );
  }
  if (empty) {
    return (
      <ProductState
        variant="empty"
        title={filtered ? t('states.filtered_empty') : t('states.activation_empty')}
        description={
          filtered
            ? t('states.filtered_empty_description')
            : t('states.activation_empty_description')
        }
      />
    );
  }
  return (
    <div className="relative">
      {refetching && (
        <div className="absolute right-0 top-0 z-10 inline-flex items-center gap-1.5 rounded bg-bg-3 px-2 py-1 text-xs text-tx-2">
          <CircleDot aria-hidden className="h-3 w-3 animate-pulse text-blue" />
          {t('states.refreshing')}
        </div>
      )}
      {children}
    </div>
  );
}

export function ApmPageFrame({
  title,
  subtitle,
  toolbar,
  navigation,
  meta,
  children,
}: {
  title: React.ReactNode;
  subtitle?: string;
  toolbar?: React.ReactNode;
  navigation?: React.ReactNode;
  meta?: ApmMeta | undefined;
  children: React.ReactNode;
}) {
  return (
    <div className="min-h-0 bg-bg-0">
      <PageHeader
        title={<h1 className="m-0">{title}</h1>}
        subtitle={subtitle}
        toolbar={toolbar}
      />
      {navigation === undefined ? <ApmNavigation /> : navigation}
      <PageBody className="space-y-5">
        {meta && <DataQualityNotice meta={meta} />}
        {children}
      </PageBody>
    </div>
  );
}

export function HealthDot({ status }: { status: string }) {
  return (
    <span
      className={cn(
        'inline-block h-2 w-2 rounded-full',
        status === 'healthy' && 'bg-green',
        status === 'warning' && 'bg-yellow',
        status === 'critical' && 'bg-red',
        status === 'no_traffic' && 'bg-tx-3',
      )}
      aria-hidden
    />
  );
}
