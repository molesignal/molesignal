import { useQuery } from '@tanstack/react-query';
import { PieChart } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { TransactionIdentity } from '@/api/apm';
import { type TimeCategory, traceTimeSpent } from '@/api/traceExplorer';
import { cn } from '@/shell/lib/cn';

import { useSlowRequests } from './SlowRequests';
import type { ServiceAddress } from './useService';
import { formatDuration } from '../format';
import { ApmCard, ApmQueryState } from '../frame/Page';

/** Each part in the ink of what it is, the request's own time plainest. */
const PART_TEXT: Record<TimeCategory, string> = {
  own: 'text-tx-0',
  database: 'text-[var(--blue-ink)]',
  cache: 'text-[var(--purple-ink)]',
  http: 'text-[var(--green-ink)]',
  rpc: 'text-[var(--chart-5)]',
  messaging: 'text-[var(--orange-ink)]',
  internal: 'text-[var(--chart-7)]',
  other: 'text-tx-2',
};

const PART_FILL: Record<TimeCategory, string> = {
  own: 'var(--chart-1)',
  database: 'var(--blue)',
  cache: 'var(--purple)',
  http: 'var(--green)',
  rpc: 'var(--chart-5)',
  messaging: 'var(--orange)',
  internal: 'var(--chart-7)',
  other: 'var(--tx-4)',
};

/**
 * Where the endpoint's requests spend their time, from the latest of them in
 * the trace store: what the calls out to each kind of thing took, the
 * service's internal work and the request's own time, each a row with its
 * share and its time per request, drawn as the latency distribution is.
 */
export function TimeSpent({ address, transaction }: { address: ServiceAddress; transaction: TransactionIdentity }) {
  const { t } = useTranslation('apm');
  const { from, to, query: slow } = useSlowRequests(address, transaction);
  const filters = slow.data?.filters ?? [];

  const query = useQuery({
    queryKey: ['apm', 'time-spent', address.orgId, filters, from, to],
    queryFn: () => traceTimeSpent({ from, to, filters }),
    enabled: filters.length > 0,
    staleTime: 60_000,
  });
  const data = query.data;
  const total = data?.parts.reduce((sum, part) => sum + part.micros, 0) ?? 0;
  const widest = Math.max(...(data?.parts ?? []).map((part) => part.micros), 1);

  return (
    <ApmCard
      icon={PieChart}
      tone="text-[var(--chart-2)]"
      title={t('endpoint.time_spent.title')}
      description={data && data.sampled > 0 ? t('endpoint.time_spent.description', { count: data.sampled }) : t('endpoint.time_spent.about')}
      testId="endpoint-time-spent"
    >
      <ApmQueryState
        query={filters.length > 0 ? query : slow}
        empty={!data || data.parts.length === 0}
        emptyTitle={t(data?.unavailable ? 'endpoint.time_spent.unavailable' : 'endpoint.time_spent.none')}
        compact
      >
        <ol aria-label={t('endpoint.time_spent.title')} className="px-3 py-2">
          {(data?.parts ?? []).map((part) => {
            const share = total > 0 ? part.micros / total : 0;
            return (
              <li
                key={part.category}
                data-apm-time-part={part.category}
                className="type-micro grid h-[18px] grid-cols-[112px_minmax(0,1fr)] items-center gap-2 font-sans"
                title={t('endpoint.time_spent.row', {
                  part: t(`endpoint.time_spent.parts.${part.category}`),
                  share: `${Math.round(share * 100)}%`,
                  each: formatDuration(part.micros / Math.max(1, data!.sampled)),
                })}
              >
                <span className={cn('truncate text-right font-strong', PART_TEXT[part.category])}>{t(`endpoint.time_spent.parts.${part.category}`)}</span>
                <span className="relative mr-[120px] h-[10px]">
                  <span className="absolute inset-y-0 left-0 rounded-[3px]" style={{ width: `${Math.max(0.6, (part.micros / widest) * 100)}%`, background: PART_FILL[part.category] }} />
                  <span className="pointer-events-none absolute top-1/2 flex -translate-y-1/2 items-center gap-1.5 whitespace-nowrap pl-1.5 tabular-nums" style={{ left: `${(part.micros / widest) * 100}%` }}>
                    <span className="text-tx-1">{Math.round(share * 100)}%</span>
                    <span className="text-tx-3">{t('endpoint.time_spent.each', { time: formatDuration(part.micros / Math.max(1, data!.sampled)) })}</span>
                  </span>
                </span>
              </li>
            );
          })}
        </ol>
      </ApmQueryState>
    </ApmCard>
  );
}
