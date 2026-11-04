import { useTranslation } from 'react-i18next';

import type { ErrorSummary } from '@/api/apm';
import { useTimeFormatter } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import { LONG_LIST_ROW_CLASS, longListRowStyle } from '@/shell/list/longList';
import { useUsers } from '@/shell/useUsers';
import { formatRelativeMicros } from '@/time/relative';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

import { useTeams } from '../catalog/people';
import { formatCount } from '../format';
import { IssueBadge } from './IssueBadge';
import { gridMinWidth } from '../frame/gridWidth';

const ROW_STYLE = { height: 32, ...longListRowStyle(31) };

/**
 * Error groups as Datadog's issue list has them, a line each: the error and
 * where it was thrown, its state and who has it (a member, a team or both),
 * how often it happened, and when it was first and last seen. A line opens
 * the group.
 */
export function ErrorTable({ errors, showService, onOpen }: { errors: readonly ErrorSummary[]; showService: boolean; onOpen: (group: ErrorSummary) => void }) {
  const { t, i18n } = useTranslation('apm');
  const time = useTimeFormatter();
  const { nameOf } = useUsers();
  const { byId: teamsById } = useTeams();
  const tracks = ['minmax(260px,1fr)', ...(showService ? ['140px'] : []), '84px', '120px', '88px', '104px', '104px'];
  const grid = tracks.join(' ');
  const minWidth = gridMinWidth(tracks);

  return (
    <div role="table" aria-label={t('errors.table')} data-apm-error-table="" className="overflow-x-auto">
      <div
        role="row"
        className="sticky top-0 grid h-8 items-center gap-3 border-b border-bd-0 bg-[var(--table-head-surface)] px-3 font-sans text-xs font-strong text-tx-0"
        style={{ gridTemplateColumns: grid, minWidth }}
      >
        <span role="columnheader">{t('errors.columns.error')}</span>
        {showService && <span role="columnheader">{t('errors.columns.service')}</span>}
        <span role="columnheader">{t('errors.columns.state')}</span>
        <span role="columnheader">{t('errors.columns.assignee')}</span>
        <span role="columnheader" className="text-right">
          {t('errors.columns.occurrences')}
        </span>
        <span role="columnheader">{t('errors.columns.last_seen')}</span>
        <span role="columnheader">{t('errors.columns.first_seen')}</span>
      </div>
      {errors.map((group) => (
        <button
          key={group.error.fingerprint}
          type="button"
          role="row"
          data-apm-error-row={group.error.fingerprint}
          onClick={() => onOpen(group)}
          style={{ ...ROW_STYLE, gridTemplateColumns: grid, minWidth }}
          className={cn(
            'grid w-full items-center gap-3 border-b border-[var(--row-rule)] px-3 text-left font-sans text-xs outline-none hover:bg-bg-2 focus-visible:bg-bg-2',
            LONG_LIST_ROW_CLASS,
          )}
        >
          <span role="cell" className="flex min-w-0 items-center gap-2">
            <span className="shrink-0 font-strong text-[var(--red-ink)]">{group.error.errorType}</span>
            <span className="truncate text-tx-0" title={group.representativeMessage ?? ''}>
              {group.representativeMessage ?? group.error.applicationFrame ?? ''}
            </span>
            {group.error.transactionName && (
              <span className="hidden max-w-[40%] shrink-0 text-tx-3 xl:inline-flex">
                <TraceOperationName mono={false} operation={group.error.transactionName} />
              </span>
            )}
          </span>
          {showService && (
            <span role="cell" className="truncate text-tx-0">
              {group.service.name}
            </span>
          )}
          <span role="cell">
            <IssueBadge issue={group.issue} />
          </span>
          <span role="cell" className="truncate text-tx-0">
            {group.issue?.assigneeId || group.issue?.teamId ? (
              [group.issue.assigneeId && nameOf(group.issue.assigneeId), group.issue.teamId && (teamsById.get(group.issue.teamId)?.name ?? group.issue.teamId)]
                .filter(Boolean)
                .join(' · ')
            ) : (
              <span className="text-tx-4">—</span>
            )}
          </span>
          <span role="cell" className="text-right tabular-nums text-tx-0">
            {formatCount(group.occurrenceCount)}
          </span>
          <span role="cell" className="truncate text-tx-0" title={time.micros(group.lastSeenAt, false)}>
            {formatRelativeMicros(group.lastSeenAt, i18n.language)}
          </span>
          <span role="cell" className="truncate text-tx-0" title={time.micros(group.firstSeenAt, false)}>
            {formatRelativeMicros(group.firstSeenAt, i18n.language)}
          </span>
        </button>
      ))}
    </div>
  );
}
