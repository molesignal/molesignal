import { Hash } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { RedSummary, TrendPoint } from '@/api/apm';
import { cn } from '@/shell/lib/cn';
import { TimeSeriesChart } from '@/viz/timeseries/TimeSeriesChart';

import { formatCount } from '../format';
import { bucketSeconds } from './series';
import { STATUS_FILL, STATUS_TEXT, type StatusClass, statusClassSeries, statusRows } from './statusCodeRows';
import { ApmCard } from '../frame/Page';

/** Each protocol as it is written. */
const PROTOCOL_NAME = { http: 'HTTP', grpc: 'gRPC', other: '' } as const;

/**
 * What the requests answered: each HTTP or gRPC status as a row, its bar as
 * long as its share, the code in the ink of what it says, and the classes
 * stacked bar by bar over the time in view. Buckets written before status
 * codes were counted have none.
 */
export function StatusCodes({
  red,
  trend,
  resolution,
  range,
}: {
  red: RedSummary;
  trend: readonly TrendPoint[];
  resolution: 'minute' | 'hour';
  range: { from: number; to: number };
}) {
  const { t } = useTranslation('apm');
  const rows = statusRows(red.statusCodes);
  const widest = Math.max(...rows.map((row) => row.share), 0.0001);
  const classLabel = (statusClass: StatusClass) => t(`endpoint.status.classes.${statusClass}`);
  const series = statusClassSeries(trend, bucketSeconds(resolution), classLabel);

  return (
    <ApmCard icon={Hash} tone="text-[var(--chart-5)]" title={t('endpoint.status.title')} description={t('endpoint.status.description')} testId="endpoint-status-codes">
      {rows.length === 0 ? (
        <p className="px-3 py-6 text-center font-sans text-xs text-tx-3">{t('endpoint.status.none')}</p>
      ) : (
        <div className="space-y-2 px-3 py-2">
          <ol aria-label={t('endpoint.status.title')}>
            {rows.map((row) => (
              <li
                key={row.key}
                data-apm-status={row.key}
                className="type-micro grid h-[18px] grid-cols-[112px_minmax(0,1fr)] items-center gap-2 font-sans"
                title={t('endpoint.status.row', { status: row.label || classLabel(row.statusClass), count: formatCount(row.count), share: percent(row.share) })}
              >
                <span className={cn('truncate text-right font-strong tabular-nums', STATUS_TEXT[row.statusClass])}>
                  {row.label || t('endpoint.status.other', { protocol: PROTOCOL_NAME[row.protocol] })}
                  {row.protocol === 'grpc' && row.code !== null && <span className="font-body text-tx-3"> {row.code}</span>}
                </span>
                <span className="relative mr-[96px] h-[10px]">
                  <span className="absolute inset-y-0 left-0 rounded-[3px]" style={{ width: `${Math.max(0.6, (row.share / widest) * 100)}%`, background: STATUS_FILL[row.statusClass] }} />
                  <span className="pointer-events-none absolute top-1/2 flex -translate-y-1/2 items-center gap-1.5 whitespace-nowrap pl-1.5 tabular-nums" style={{ left: `${(row.share / widest) * 100}%` }}>
                    <span className="text-tx-1">{formatCount(row.count)}</span>
                    <span className="text-tx-3">{percent(row.share)}</span>
                  </span>
                </span>
              </li>
            ))}
          </ol>
          {series.length > 0 && (
            <TimeSeriesChart
              series={series}
              xDomain={[range.from, range.to]}
              height={120}
              ariaLabel={t('endpoint.status.trend')}
              showLegend
              legendDensity="compact"
              options={{
                drawStyle: 'bar',
                legendMode: 'list',
                stackMode: 'normal',
                showPoints: 'never',
                tooltipMode: 'all',
                compactAxes: true,
                leftAxis: { scale: 'linear', min: 0, unit: 'req/s' },
              }}
            />
          )}
        </div>
      )}
    </ApmCard>
  );
}

function percent(share: number): string {
  return share >= 0.1 ? `${Math.round(share * 100)}%` : `${(share * 100).toFixed(1)}%`;
}
