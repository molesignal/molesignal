import { Flame } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { RumAggregate } from '@/api/rum';
import { useTimeFormatter } from '@/lib/time';
import { cn } from '@/shell/lib/cn';

import { clause, explorerPath, formatVital, gradeOf, type OverviewScope } from '../../overview/model';
import { CardState, OverviewCard } from '../../overview/OverviewCard';
import { GRADE_TEXT } from '../ink';
import type { Metric, WorstView } from '../model';

type Read = { isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown; data: RumAggregate | undefined };

const LINK =
  'inline-flex h-7 items-center rounded-md bg-[var(--control-surface)] px-2.5 font-sans text-xs font-strong text-tx-1 outline-none hover:bg-bg-3 focus-visible:bg-bg-3';

/**
 * The views that had the highest value of the level: when each was, who it was of, which version and
 * browser ran it, and how high it was. Each opens in the explorer, and its session opens whole.
 */
export function WorstCard({
  query,
  worst,
  metric,
  scope,
  onOpenView,
}: {
  query: Read;
  worst: readonly WorstView[];
  metric: Metric;
  scope: OverviewScope;
  /** Opens the load of a view over the drawer, when the page can. */
  onOpenView?: ((view: string, at: number | null) => void) | undefined;
}) {
  const { t } = useTranslation('rum');
  const formatter = useTimeFormatter();

  return (
    <OverviewCard
      icon={Flame}
      tone="text-[var(--red-ink)]"
      title={t('performance.worst.title')}
      description={t('performance.worst.description', { name: t(`performance.metrics.${metric.key}`) })}
      testId="performance-worst"
    >
      <CardState query={query} empty={worst.length === 0} emptyTitle={t('performance.worst.empty')}>
        <ol data-rum-worst="" className="m-0 list-none p-0">
          {worst.map((entry) => (
            <li
              key={entry.id}
              data-rum-worst-view={entry.id}
              className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-[var(--row-rule)] px-3 py-2 font-sans text-xs last:border-b-0"
            >
              <div className="min-w-0 flex-1 basis-[200px]">
                <div className="truncate font-strong text-tx-0">
                  {[entry.at === null ? null : formatter.micros(entry.at), entry.user].filter(Boolean).join(' · ') || entry.id}
                </div>
                <div className="truncate text-tx-3">
                  {[entry.version, entry.browser, entry.system].filter(Boolean).join(' · ') || '—'}
                </div>
              </div>
              <span className={cn('w-16 text-right font-strong tabular-nums', GRADE_TEXT[gradeOf(metric, entry.value)])}>
                {formatVital(metric, entry.value)}
              </span>
              {onOpenView && (
                <button type="button" data-rum-worst-load="" onClick={() => onOpenView(entry.id, entry.at)} className={cn(LINK, 'border-0')}>
                  {t('performance.worst.load')}
                </button>
              )}
              <Link to={explorerPath('views', scope, [clause('view.id', entry.id)])} className={LINK}>
                {t('performance.worst.explore')}
              </Link>
              {entry.session && (
                <Link to={`/rum/sessions/view/${encodeURIComponent(entry.session)}`} className={LINK}>
                  {t('performance.worst.session')}
                </Link>
              )}
            </li>
          ))}
        </ol>
      </CardState>
    </OverviewCard>
  );
}
