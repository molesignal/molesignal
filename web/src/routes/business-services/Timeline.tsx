import { useTranslation } from 'react-i18next';

import type { BusinessServiceHealthPeriod } from '@/api/businessServices';
import { formatMicrosActive } from '@/lib/time';
import { cn } from '@/shell/lib/cn';

import { STATUS_FILL, operationalShare, timelineSegments } from './model';

/**
 * The last seven days of a business service as one bar: each period in the
 * colour of its status, blank before the service was first evaluated.
 */
export function Timeline({
  history,
  nowMicros,
  compact = false,
}: {
  history: readonly BusinessServiceHealthPeriod[];
  nowMicros: number;
  /** A row's bar: thinner, with the share under it and no ends named. */
  compact?: boolean;
}) {
  const { t } = useTranslation('business-services');
  const segments = timelineSegments(history, nowMicros);
  const share = operationalShare(segments);
  const shareLabel = share === null
    ? t('timeline.no_data')
    : t('timeline.operational_share', { share: (share * 100).toFixed(share === 1 ? 0 : 2) });

  return (
    <div className="min-w-0">
      <div
        role="img"
        aria-label={`${t('timeline.label')}: ${shareLabel}`}
        className={cn('relative w-full overflow-hidden rounded-full bg-bg-3', compact ? 'h-1.5' : 'h-3')}
      >
        {segments.map((segment) => (
          <span
            key={segment.startedAt}
            title={`${t(`statuses.${segment.status}`)} · ${formatMicrosActive(segment.startedAt, false)} – ${formatMicrosActive(segment.endedAt, false)}`}
            className={cn('absolute inset-y-0', STATUS_FILL[segment.status])}
            style={{ left: `${segment.offset * 100}%`, width: `${Math.max(segment.width * 100, 0.4)}%` }}
          />
        ))}
      </div>
      <div className="mt-1 flex justify-between text-xs text-tx-3">
        {!compact && <span>{t('timeline.seven_days_ago')}</span>}
        <span className={cn(compact && 'ml-auto')}>{shareLabel}</span>
        {!compact && <span>{t('timeline.now')}</span>}
      </div>
    </div>
  );
}
