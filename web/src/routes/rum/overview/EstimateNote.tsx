import { Info } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { RumAggregate, RumAggregateParams } from '@/api/rum';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { formatShare } from '../_helpers';
import { type OverviewRange, scopeFilters } from './model';
import { useAggregate } from './useOverview';

/** Less kept than this share of the sessions is worth a word; nearer to all of them says nothing. */
const SAID_BELOW = 0.995;

/** How many sessions there are to say, and how many were kept to say it: the second against the first. */
export function keptShare(answer: RumAggregate | undefined): number | null {
  const [estimated, stored] = answer?.rows[0]?.values ?? [];

  if (typeof estimated !== 'number' || typeof stored !== 'number' || estimated <= 0 || stored <= 0) return null;

  return Math.min(1, stored / estimated);
}

/** The read that tells it: the sessions of the application, as many as they stand for and as many as are stored. */
export function keptShareQuery(application: string, environment: string, range: OverviewRange): RumAggregateParams {
  return {
    type: 'sessions',
    from: range.from,
    to: range.to,
    filters: scopeFilters({ application, environment }),
    measures: [
      { fn: 'count', name: 'estimated' },
      { fn: 'stored', name: 'stored' },
    ],
    estimate: true,
  };
}

/**
 * Says that the totals of a page are estimates, when the application keeps only a share of its
 * sessions: each session that was kept counts for as many as the share it was kept with says. Nothing
 * is said while every session is kept.
 */
export function EstimateNote({
  application,
  environment,
  range,
}: {
  application: string;
  environment: string;
  range: OverviewRange;
}) {
  const { t } = useTranslation('rum');
  const read = useAggregate('estimate', keptShareQuery(application, environment, range));
  const share = keptShare(read.data);

  if (share === null || share >= SAID_BELOW) return null;

  const text = formatShare(share);

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          data-rum-estimate=""
          tabIndex={0}
          className="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-md px-1.5 font-sans text-xs text-tx-3 hover:text-tx-1 focus-visible:text-tx-1"
        >
          <Info aria-hidden className="h-3.5 w-3.5" />
          {t('estimate.note', { share: text })}
        </span>
      </TooltipTrigger>
      <TooltipContent side="bottom" className="max-w-xs">
        {t('estimate.hint', { share: text })}
      </TooltipContent>
    </Tooltip>
  );
}
