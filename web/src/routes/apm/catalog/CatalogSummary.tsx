import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import { formatRate, formatThroughput } from '../format';
import { type CatalogRow, throughputOf } from './model';

const HEALTH = ['critical', 'warning', 'healthy', 'no_traffic'] as const;

const DOT: Record<(typeof HEALTH)[number], string> = {
  critical: 'bg-red',
  warning: 'bg-[var(--yellow)]',
  healthy: 'bg-[var(--green)]',
  no_traffic: 'bg-tx-4',
};

/**
 * The catalog's statement, a line over the list: how many services, how
 * their health divides, and what they served together in the time in view.
 */
export function CatalogSummary({ rows, range, quality }: { rows: readonly CatalogRow[]; range: { from: Date; to: Date }; quality?: React.ReactNode }) {
  const { t } = useTranslation('apm');
  const counts = { critical: 0, warning: 0, healthy: 0, no_traffic: 0 };
  let requests = 0;
  let errors = 0;

  for (const row of rows) {
    if (!row.summary) continue;
    counts[row.summary.health] += 1;
    requests += row.summary.red.requestCount;
    errors += row.summary.red.errorCount;
  }

  return (
    <div data-apm-catalog-summary="" className="flex flex-wrap items-center gap-x-4 gap-y-1 px-1 font-sans text-xs text-tx-2">
      <span className="font-strong text-tx-0">{t('catalog.summary.services', { count: rows.length })}</span>
      <span className="flex items-center gap-3">
        {HEALTH.map((health) => (
          <span key={health} className={cn('flex items-center gap-1.5', counts[health] === 0 && 'text-tx-3')}>
            <span aria-hidden className={cn('h-2 w-2 rounded-full', DOT[health])} />
            {t(`health.${health}`)} <span className="tabular-nums text-tx-1">{counts[health]}</span>
          </span>
        ))}
      </span>
      <span>
        {t('catalog.summary.throughput')} <span className="tabular-nums text-tx-1">{formatThroughput(throughputOf(requests, range))}</span>
      </span>
      <span>
        {t('catalog.summary.error_rate')}{' '}
        <span className={cn('tabular-nums', errors > 0 ? 'font-strong text-[var(--red-ink)]' : 'text-tx-1')}>
          {formatRate(requests > 0 ? errors / requests : 0)}
        </span>
      </span>
      {quality && <span className="ml-auto">{quality}</span>}
    </div>
  );
}
