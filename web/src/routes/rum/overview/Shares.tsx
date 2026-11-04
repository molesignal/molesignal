import type { LucideIcon } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { RumAggregate } from '@/api/rum';

import { type BarRow, BarRows } from './BarRows';
import { clause, explorerPath, type OverviewScope, readShares, SHARE_ROWS, withOthers } from './model';
import { CardState, OverviewCard } from './OverviewCard';

/**
 * One column of the sessions told by its values: the browsers, the operating systems, the kinds of
 * device. The commonest come first, each a link to the sessions it counts; what is not shown, and
 * the sessions that have no value, are counted together as the rest.
 */
export function SharesCard({
  kind,
  field,
  icon,
  tone,
  query,
  total,
  scope,
  language,
}: {
  kind: 'browsers' | 'systems' | 'devices';
  /** The column of a session it is told by. */
  field: string;
  icon: LucideIcon;
  tone: string;
  query: { isPending: boolean; isError: boolean; error: unknown; refetch: () => unknown; data: RumAggregate | undefined };
  total: number;
  scope: OverviewScope;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const { shown, others } = withOthers(readShares(query.data), total, SHARE_ROWS);

  const rows: BarRow[] = [
    ...shown.map((share) => ({
      key: share.value,
      label: kind === 'devices' ? t(`overview.shares.device_types.${share.value}`, { defaultValue: share.value }) : share.value,
      count: share.sessions,
      to: explorerPath('sessions', scope, [clause(field, share.value)]),
    })),
    ...(others > 0 ? [{ key: '__other__', label: t('overview.shares.other'), count: others }] : []),
  ];

  return (
    <OverviewCard icon={icon} tone={tone} title={t(`overview.shares.${kind}`)} testId={kind}>
      <CardState query={query} empty={shown.length === 0} emptyTitle={t('overview.shares.empty')}>
        <BarRows rows={rows} total={total} language={language} testId={kind} />
      </CardState>
    </OverviewCard>
  );
}
