import { ArrowDown, ArrowUp } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { RumIssue, RumIssueRow, RumIssueSort } from '@/api/rum';
import { useTimeFormatter } from '@/lib/time';
import { cn } from '@/shell/lib/cn';

import { IssueState, Sparkline } from './IssueState';
import { defaultOrder, formatAgo, STATE_INK, versionSpan } from './model';
import { formatCount } from '../overview/format';

const TRACKS = 'minmax(220px,1fr) 84px 112px 72px 72px 88px 120px 84px';

const HEADER = 'grid h-8 items-center gap-x-3 border-b border-bd-0 bg-[var(--table-head-surface)] px-3 font-sans text-xs font-strong text-tx-0';

const ROW =
  'relative grid min-h-11 items-center gap-x-3 border-b border-[var(--row-rule)] px-3 py-1.5 font-sans text-xs hover:bg-bg-2 focus-within:bg-bg-2';

/** What a person or a team is called: its name, or its id when the name is not known. */
export interface Names {
  user: (id: string) => string;
  team: (id: string) => string;
}

/** The title of an issue: its type, and what its error says. The rest of an application is told as what it is. */
export function IssueTitle({ issue }: { issue: Pick<RumIssue, 'overflow' | 'errorType' | 'message'> }) {
  const { t } = useTranslation('rum');

  if (issue.overflow) return <span className="text-tx-0">{t('errors.overflow.title')}</span>;

  return (
    <>
      {issue.errorType && <span className="text-[var(--red-ink)]">{issue.errorType}</span>}
      {issue.errorType && issue.message ? ' ' : ''}
      {issue.message}
    </>
  );
}

/**
 * The issues of the range as a table: what each is, the state and who has it, how many sessions and
 * people it met, how it went over the range, which versions it was seen in and when last. A column
 * of numbers or of times orders the rows; a row opens the issue in a drawer.
 */
export function IssueTable({
  rows,
  sort,
  order,
  onSort,
  onOpen,
  names,
  now,
  language,
}: {
  rows: readonly RumIssueRow[];
  sort: RumIssueSort;
  order: 'asc' | 'desc' | undefined;
  onSort: (sort: RumIssueSort) => void;
  onOpen: (id: string) => void;
  names: Names;
  now: number;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const formatter = useTimeFormatter();
  const direction = order ?? defaultOrder(sort);

  const header = (column: RumIssueSort, label: string, align: 'left' | 'right' = 'right') => {
    const active = sort === column;
    const Arrow = direction === 'desc' ? ArrowDown : ArrowUp;

    return (
      <span role="columnheader" aria-sort={active ? (direction === 'desc' ? 'descending' : 'ascending') : 'none'} className={align === 'right' ? 'text-right' : ''}>
        <button
          type="button"
          onClick={() => onSort(column)}
          data-rum-sort={column}
          className={cn(
            'inline-flex items-center gap-1 border-0 bg-transparent p-0 font-sans text-xs font-strong outline-none hover:text-tx-0 focus-visible:text-tx-0',
            active ? 'text-tx-0' : 'text-tx-2',
          )}
        >
          {label}
          {active && <Arrow aria-hidden className="h-3 w-3" />}
        </button>
      </span>
    );
  };

  return (
    <div className="overflow-x-auto">
      <div role="table" aria-label={t('errors.table.label')} data-rum-issues="" className="min-w-[960px]">
        <div role="row" className={HEADER} style={{ gridTemplateColumns: TRACKS }}>
          <span role="columnheader">{t('errors.table.issue')}</span>
          {header('status', t('errors.table.status'), 'left')}
          <span role="columnheader">{t('errors.table.holder')}</span>
          {header('sessions', t('errors.table.sessions'))}
          {header('users', t('errors.table.users'))}
          <span role="columnheader">{t('errors.table.trend')}</span>
          <span role="columnheader">{t('errors.table.versions')}</span>
          {header('last_seen', t('errors.table.last_seen'))}
        </div>
        {rows.map((issue) => {
          const holder = issue.assigneeId ? names.user(issue.assigneeId) : issue.teamId ? names.team(issue.teamId) : '';
          const quiet = issue.status === 'resolved' || issue.status === 'ignored';

          return (
            <div key={issue.id} role="row" data-rum-issue={issue.id} data-rum-issue-state={issue.status} className={ROW} style={{ gridTemplateColumns: TRACKS }}>
              <span role="cell" className="min-w-0">
                <button
                  type="button"
                  onClick={() => onOpen(issue.id)}
                  className="block w-full truncate border-0 bg-transparent p-0 text-left font-sans text-sm font-strong text-tx-0 outline-none after:absolute after:inset-0 after:content-['']"
                >
                  <IssueTitle issue={issue} />
                </button>
                <span className="block truncate text-xs text-tx-3">
                  {issue.overflow ? t('errors.overflow.description') : issue.location}
                </span>
              </span>
              <span role="cell">
                <IssueState status={issue.status} />
              </span>
              <span role="cell" className="min-w-0">
                {holder ? (
                  <span className="inline-flex min-w-0 items-center gap-1.5">
                    <span aria-hidden className="grid h-5 w-5 shrink-0 place-items-center rounded-full bg-bg-3 text-xs font-strong text-tx-1">
                      {holder.slice(0, 1).toUpperCase()}
                    </span>
                    <span className="truncate text-tx-1">{holder}</span>
                  </span>
                ) : (
                  <span className="text-tx-4">—</span>
                )}
              </span>
              <span role="cell" className="text-right tabular-nums text-tx-0">
                {formatCount(issue.sessions, language)}
              </span>
              <span role="cell" className="text-right tabular-nums text-tx-1">
                {formatCount(issue.users, language)}
              </span>
              <span role="cell">
                <Sparkline values={issue.trend} className={quiet ? 'text-tx-3' : STATE_INK.regressed} />
              </span>
              <span role="cell" className="truncate tabular-nums text-tx-1">
                {versionSpan(issue)}
              </span>
              <span role="cell" className="text-right text-tx-3" title={formatter.micros(issue.lastSeenAt)}>
                {formatAgo(issue.lastSeenAt, now, language)}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
