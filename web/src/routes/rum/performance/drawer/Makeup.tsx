import { Layers } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { RumAggregate } from '@/api/rum';
import { cn } from '@/shell/lib/cn';

import { formatDurationMs } from '../../_helpers';
import { formatCount, formatPercent } from '../../overview/format';
import { CardState, OverviewCard } from '../../overview/OverviewCard';
import { longest, type Makeup, type Metric } from '../model';

type Read = { isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown; data: RumAggregate | undefined };

/** The colours of the parts of a level, in the order they happen. */
const INKS = ['var(--chart-1)', 'var(--chart-2)', 'var(--chart-7)', 'var(--chart-8)'] as const;

/**
 * What a level is made of: the time it takes to answer, to start, to arrive and to be drawn, as one
 * bar as long as the sum of the parts and a key under it that says how long each took. The part that
 * took the longest is in the strong step. The parts are the mean of the views that stand for the 75th
 * percentile, so that they add up to about what it is.
 */
export function MakeupCard({
  query,
  makeup,
  metric,
  language,
}: {
  query: Read;
  makeup: Makeup | null;
  metric: Metric;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const name = t(`performance.metrics.${metric.key}`);
  const total = makeup?.parts.reduce((sum, part) => sum + part.ms, 0) ?? 0;
  const top = makeup ? longest(makeup) : null;

  return (
    <OverviewCard
      icon={Layers}
      tone="text-[var(--chart-2)]"
      title={t('performance.makeup.title', { name })}
      description={t(`performance.makeup.description.${metric.key}`)}
      testId="performance-makeup"
    >
      <CardState query={query} empty={makeup === null || total <= 0} emptyTitle={t('performance.makeup.empty', { name })}>
        {makeup && total > 0 && (
          <div className="space-y-3 px-3 pb-3 pt-3">
            <div
              role="img"
              aria-label={t('performance.makeup.aria', { name, total: formatDurationMs(total) })}
              data-rum-makeup-bar=""
              className="flex h-[18px] gap-[2px]"
            >
              {makeup.parts.map((part, index) => (
                <span
                  key={part.key}
                  data-rum-part={part.key}
                  className={cn(index === 0 && 'rounded-l-[4px]', index === makeup.parts.length - 1 && 'rounded-r-[4px]')}
                  style={{ flex: Math.max(part.ms, 0.0001), background: INKS[index % INKS.length], opacity: 0.9 }}
                />
              ))}
            </div>
            <ul className="m-0 list-none space-y-1 p-0 font-sans text-xs">
              {makeup.parts.map((part, index) => (
                <li key={part.key} data-rum-part-line={part.key} className="grid grid-cols-[10px_minmax(0,1fr)_72px_40px] items-center gap-2.5">
                  <span aria-hidden className="h-2.5 w-2.5 rounded-[2px]" style={{ background: INKS[index % INKS.length] }} />
                  <span className={cn('truncate', top?.key === part.key ? 'font-strong text-tx-0' : 'text-tx-1')}>{t(`performance.parts.${part.key}`)}</span>
                  <span className="text-right tabular-nums text-tx-0">{formatDurationMs(part.ms)}</span>
                  <span className="text-right tabular-nums text-tx-3">{formatPercent(part.ms / total)}</span>
                </li>
              ))}
            </ul>
            <p className="m-0 font-sans text-xs text-tx-3">
              {t('performance.makeup.cohort', { formatted: formatCount(makeup.views, language) })}
            </p>
          </div>
        )}
      </CardState>
    </OverviewCard>
  );
}
