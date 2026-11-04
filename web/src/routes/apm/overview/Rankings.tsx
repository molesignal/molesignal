import { Trophy } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { ServiceRank, ServiceRankings } from '@/api/apm';
import { cn } from '@/shell/lib/cn';

import { throughputOf } from '../catalog/model';
import { formatDuration, formatRate, formatThroughput } from '../format';
import { ApmCard } from '../frame/Page';
import { servicePath } from '../scope';
import { HEALTH_FILL, serviceRowKey } from './model';

const LISTS = ['slowest', 'most_errors', 'busiest'] as const;

/**
 * The services that weigh most, three ways: the slowest by P95, the ones
 * failing most often, and the busiest. A row opens the service.
 */
export function RankingsCard({
  rankings,
  range,
  searchParams,
}: {
  rankings: ServiceRankings;
  range: { from: Date; to: Date };
  searchParams: URLSearchParams;
}) {
  const { t } = useTranslation('apm');

  return (
    <ApmCard icon={Trophy} tone="text-[var(--chart-3)]" title={t('landing.rankings')} testId="overview-rankings" bodyClassName="grid min-w-0 gap-px bg-bd-0 lg:grid-cols-3">
      {LISTS.map((list) => (
        <section key={list} data-apm-ranking={list} className="min-w-0 bg-[var(--functional-surface)]">
          <h3 className="flex h-8 items-center bg-[var(--control-surface)] px-3 font-sans text-xs font-strong text-tx-2">{t(`landing.ranking.${list}`)}</h3>
          {rankings[list].length === 0 ? (
            <p className="px-3 py-6 text-center font-sans text-xs text-tx-3">{t('landing.ranking.empty')}</p>
          ) : (
            rankings[list].map((rank) => (
              <Link
                key={serviceRowKey(rank.service)}
                to={servicePath(rank.service, 'overview', searchParams)}
                className="grid h-8 grid-cols-[minmax(0,1fr)_auto] items-center gap-3 border-b border-bd-0 px-3 font-sans text-xs last:border-b-0 hover:bg-bg-2 focus-visible:bg-bg-2 focus-visible:outline-none"
              >
                <span className="flex min-w-0 items-center gap-2">
                  <span aria-hidden className="h-2 w-2 shrink-0 rounded-full" style={{ background: HEALTH_FILL[rank.health] }} />
                  <span className="truncate font-strong text-tx-0">{rank.service.name}</span>
                  <span className="truncate text-tx-3">{rank.service.environment}</span>
                </span>
                <RankFigure list={list} rank={rank} range={range} />
              </Link>
            ))
          )}
        </section>
      ))}
    </ApmCard>
  );
}

function RankFigure({ list, rank, range }: { list: (typeof LISTS)[number]; rank: ServiceRank; range: { from: Date; to: Date } }) {
  if (list === 'slowest') return <span className="tabular-nums text-tx-1">{formatDuration(rank.p95Time ?? undefined)}</span>;
  if (list === 'most_errors') {
    return <span className={cn('tabular-nums', rank.errorRate > 0 ? 'font-strong text-[var(--red-ink)]' : 'text-tx-3')}>{formatRate(rank.errorRate)}</span>;
  }
  return <span className="tabular-nums text-tx-1">{formatThroughput(throughputOf(rank.requestCount, range))}</span>;
}
