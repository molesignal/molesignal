import { useTranslation } from 'react-i18next';

import type { BaselineFigures, InsightsResponse } from '@/api/apmInsights';
import { cn } from '@/shell/lib/cn';

import { formatRelative, formatSigned } from '../format';
import { baselineDeltas } from './sentence';

/**
 * Now against the stretch of the same length before and against the same
 * stretch a week before, a line each: throughput by share, the error rate
 * in points, P95 in time. A rise of errors or latency reads in red.
 */
export function BaselineTable({ data }: { data: InsightsResponse }) {
  const { t } = useTranslation('apm');
  const lines = (
    [
      ['previous', data.previous],
      ['last_week', data.lastWeek],
    ] as const
  ).filter((line): line is readonly ['previous' | 'last_week', BaselineFigures] => line[1] !== undefined);

  if (lines.length === 0 || data.now.requestCount === 0) return null;

  return (
    <div data-apm-baseline="" className="space-y-0.5 px-3 py-2 font-sans text-xs text-tx-2">
      {lines.map(([key, before]) => {
        const delta = baselineDeltas(data.now, before);

        return (
          <p key={key} data-apm-baseline-line={key}>
            <span className="font-strong text-tx-1">{t(`insights.baseline.${key}`)}</span>
            {'  '}
            {t('insights.baseline.throughput')} <span className="tabular-nums text-tx-1">{delta.throughput === null ? '—' : formatRelative(delta.throughput)}</span>
            {t('insights.separator')}
            {t('insights.baseline.error_rate')}{' '}
            <span className={cn('tabular-nums', delta.errorRate >= 0.005 ? 'text-[var(--red-ink)]' : 'text-tx-1')}>
              {t('insights.baseline.points', { value: formatSigned(delta.errorRate * 100) })}
            </span>
            {t('insights.separator')}
            P95{' '}
            <span className={cn('tabular-nums', (delta.p95 ?? 0) >= 50_000 ? 'text-[var(--red-ink)]' : 'text-tx-1')}>
              {delta.p95 === null ? '—' : formatSigned(delta.p95, false, true)}
            </span>
          </p>
        );
      })}
    </div>
  );
}
