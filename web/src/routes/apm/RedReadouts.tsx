import { useTranslation } from 'react-i18next';

import type { ApmMeta, RedSummary, TrendPoint } from '@/api/apm';
import { cn } from '@/shell/lib/cn';
import { Readout } from '@/shell/readout/Readout';

import { averageThroughput } from './components';
import { formatCount, formatDuration, formatRate, formatThroughput } from './format';

const CELL = 'bg-[var(--functional-surface)] px-[20px] py-[16px]';

interface RedReadoutsProps {
  red: RedSummary;
  trend: TrendPoint[];
  resolution: ApmMeta['resolution'];
}

/**
 * Requests, errors and latency as four cells of one instrument. Each figure
 * carries the shape it has had over the window, so "1.1% errors" also says
 * whether that is a spike or the usual.
 *
 * The cells are laid out by the surface that holds them: a strip of their own
 * (`RedKpis`) or the right side of the overview's opening statement.
 */
export function RedReadoutCells({
  red,
  trend,
  resolution,
  firstIndex = 0,
}: RedReadoutsProps & {
  /** Where the first cell falls in the page's entrance order. */
  firstIndex?: number;
}) {
  const { t } = useTranslation('apm');
  const bucketSeconds = resolution === 'hour' ? 3_600 : 60;

  return (
    <>
      <Readout
        className={CELL}
        riseIndex={firstIndex}
        label={t('metrics.throughput')}
        value={formatThroughput(averageThroughput(red.request_count, trend, resolution))}
        trend={{
          values: trend.map((point) => point.red.request_count / bucketSeconds),
          variant: 'bars',
        }}
        detail={t('metrics.total_requests', { count: formatCount(red.request_count) })}
      />
      <Readout
        className={CELL}
        riseIndex={firstIndex + 1}
        label={t('metrics.error_rate')}
        value={formatRate(red.error_rate)}
        tone={red.error_rate >= 0.1 ? 'danger' : red.error_rate >= 0.02 ? 'warn' : 'good'}
        trend={{ values: trend.map((point) => point.red.error_rate), zeroBased: true }}
        detail={`${formatCount(red.error_count)} ${t('metrics.errors').toLowerCase()}`}
      />
      <Readout
        className={CELL}
        riseIndex={firstIndex + 2}
        label={t('metrics.p95')}
        value={formatDuration(red.p95_micros)}
        trend={{ values: trend.map((point) => point.red.p95_micros) }}
        trendClassName="text-orange-soft"
        detail={`${t('metrics.p50')} ${formatDuration(red.p50_micros)}`}
      />
      <Readout
        className={CELL}
        riseIndex={firstIndex + 3}
        label={t('metrics.p99')}
        value={formatDuration(red.p99_micros)}
        trend={{ values: trend.map((point) => point.red.p99_micros) }}
        trendClassName="text-red-soft"
        detail={`${t('metrics.average')} ${formatDuration(red.duration_average_micros)}`}
      />
    </>
  );
}

/** The RED figures as a strip of their own, on the pages of one service or transaction. */
export function RedKpis({ className, ...props }: RedReadoutsProps & { className?: string }) {
  return (
    <section
      data-apm-red
      className={cn(
        'grid gap-px overflow-hidden rounded-md bg-bd-0 [box-shadow:var(--shadow-functional-surface)] sm:grid-cols-2 xl:grid-cols-4',
        className,
      )}
    >
      <RedReadoutCells {...props} />
    </section>
  );
}
