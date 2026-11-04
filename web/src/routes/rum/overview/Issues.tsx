import { Bug } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { RumIssueRow } from '@/api/rum';

import { formatCount } from './format';
import type { OverviewRange, OverviewScope } from './model';
import { CardState, OverviewCard } from './OverviewCard';
import { IssueState } from '../errors/IssueState';
import { IssueTitle } from '../errors/IssueTable';
import { formatAgo } from '../errors/model';
import { useIssueList } from '../errors/useErrors';

/** How many issues the card names. */
const SHOWN = 5;

const ROW =
  'relative grid min-h-11 items-center gap-3 border-b border-[var(--row-rule)] px-3 py-1.5 font-sans hover:bg-bg-2 focus-within:bg-bg-2';
const TRACKS = 'minmax(0,1fr) minmax(72px,auto) minmax(56px,auto)';

/** Where an issue opens: the errors page, on the same application and environment, with the issue in its drawer. */
function issuePath(scope: OverviewScope, id: string): string {
  const params = new URLSearchParams({ app: scope.application, issue: id });

  if (scope.environment) params.set('env', scope.environment);

  return `/rum/errors?${params}`;
}

function Line({ issue, now, language }: { issue: RumIssueRow; now: number; language: string }) {
  const { t } = useTranslation('rum');
  const version = issue.lastVersion ?? issue.firstVersion ?? '';

  const told =
    issue.status === 'regressed'
      ? t(issue.resolvedVersion && issue.regressedVersion ? 'overview.issues.regressed' : 'overview.issues.regressed_plain', {
          resolved: issue.resolvedVersion ?? '',
          version: issue.regressedVersion ?? '',
          ago: formatAgo(issue.regressedAt ?? issue.lastSeenAt, now, language),
        })
      : t(version ? 'overview.issues.first_seen' : 'overview.issues.first_seen_plain', {
          ago: formatAgo(issue.firstSeenAt, now, language),
          version,
        });

  return (
    <>
      <span className="min-w-0">
        <span className="block truncate text-sm font-strong text-tx-0">
          <IssueTitle issue={issue} />
        </span>
        <span className="block truncate text-xs text-tx-3">{[issue.overflow ? '' : issue.location, told].filter(Boolean).join(' · ')}</span>
      </span>
      <span className="text-right text-sm tabular-nums text-tx-1">{t('overview.issues.sessions', { count: issue.sessions, formatted: formatCount(issue.sessions, language) })}</span>
      <IssueState status={issue.status} />
    </>
  );
}

/**
 * The issues that are new in the range or went back in it, by how many sessions they met: the ones
 * a reader would want to open first. A row opens the issue on the errors page.
 */
export function IssuesCard({
  scope,
  range,
  language,
}: {
  scope: OverviewScope;
  range: OverviewRange;
  language: string;
}) {
  const { t } = useTranslation('rum');
  const list = useIssueList({
    application: scope.application,
    from: range.from,
    to: range.to,
    status: 'unresolved',
    fresh: true,
    limit: SHOWN,
    ...(scope.environment && { env: scope.environment }),
  });
  const rows = list.data?.issues ?? [];
  const now = Date.now() * 1_000;

  return (
    <OverviewCard
      icon={Bug}
      tone="text-[var(--red-ink)]"
      title={t('overview.issues.title')}
      description={t('overview.issues.description')}
      testId="issues"
      action={
        <Link
          to={`/rum/errors?${new URLSearchParams({ app: scope.application, ...(scope.environment && { env: scope.environment }) })}`}
          className="shrink-0 font-sans text-xs font-strong text-[var(--blue-ink)] outline-none hover:underline focus-visible:underline"
        >
          {t('overview.issues.all')} →
        </Link>
      }
    >
      <CardState query={list} empty={rows.length === 0} emptyTitle={t('overview.issues.empty')}>
        <ul data-rum-latest-issues="" className="m-0 list-none p-0">
          {rows.map((issue) => (
            <li key={issue.id} data-rum-latest-issue={issue.id} className={ROW} style={{ gridTemplateColumns: TRACKS }}>
              <Line issue={issue} now={now} language={language} />
              <Link
                to={issuePath(scope, issue.id)}
                aria-label={issue.overflow ? t('errors.overflow.title') : `${issue.errorType} ${issue.message}`}
                className="absolute inset-0 outline-none"
              />
            </li>
          ))}
        </ul>
      </CardState>
    </OverviewCard>
  );
}
