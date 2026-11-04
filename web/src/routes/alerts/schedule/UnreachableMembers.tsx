import { TriangleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { Reachability } from '@/api/notify';

/**
 * The members of a schedule who cannot get alerts at all: no method of their own and no team or
 * organization group to fall back on. The next person to take over comes first when they are one.
 */
export function UnreachableMembers({
  memberIds,
  reach,
  nameOf,
  nextUserId,
}: {
  memberIds: string[];
  reach: Record<string, Reachability>;
  nameOf: (id: string) => string;
  nextUserId: string | null;
}) {
  const { t, i18n } = useTranslation('alerts');
  const unreachable = memberIds.filter((id) => reach[id] === 'none');
  if (unreachable.length === 0) return null;
  const nextIsOne = nextUserId !== null && unreachable.includes(nextUserId);

  return (
    <div
      role="status"
      data-schedule-unreachable
      className="flex items-start gap-2 rounded-md bg-orange-dim px-3 py-2.5 font-sans text-sm text-tx-0"
    >
      <TriangleAlert aria-hidden="true" className="mt-0.5 h-4 w-4 shrink-0 text-[var(--orange-ink)]" />
      <span className="min-w-0">
        {nextIsOne
          ? t('schedules.unreachable.next', { name: nameOf(nextUserId) })
          : t('schedules.unreachable.members', {
              count: unreachable.length,
              names: new Intl.ListFormat(i18n.language, { type: 'conjunction' }).format(unreachable.map(nameOf)),
            })}{' '}
        <Link className="text-indigo-soft underline-offset-2 hover:underline" to="/account/settings/notify">
          {t('schedules.unreachable.fix')}
        </Link>
      </span>
    </div>
  );
}
