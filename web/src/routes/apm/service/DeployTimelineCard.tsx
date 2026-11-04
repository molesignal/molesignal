import { useQuery } from '@tanstack/react-query';
import { GitCommitHorizontal, GitPullRequest, Rocket, Workflow } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { VersionSummary } from '@/api/apm';
import { apmDeploymentsApi, apmDeploymentsQueryKeys, type DeploymentEvent } from '@/api/apmDeployments';
import { useTimeFormatter } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import { formatRelativeMicros } from '@/time/relative';

import { commitLink, deployTimeline, type TimelineEntry, type TimelineKind } from './deployTimeline';
import type { ServiceAddress } from './useService';
import { formatCount, formatDuration, formatRate } from '../format';
import { ApmCard } from '../frame/Page';
import { VerdictWord } from '../frame/VersionVerdict';

const GRID = 'grid-cols-[96px_minmax(120px,1fr)_minmax(96px,150px)_minmax(0,1.3fr)_72px_72px_72px_minmax(0,180px)]';

const KIND_TEXT: Record<TimelineKind, string> = {
  deployed: 'text-[var(--green-ink)]',
  failed: 'text-[var(--red-ink)]',
  rolled_back: 'text-[var(--orange-ink)]',
  returned: 'text-[var(--orange-ink)]',
  first_seen: 'text-tx-2',
  configuration: 'text-[var(--purple-ink)]',
  feature_flag: 'text-[var(--purple-ink)]',
};

const LINK = 'inline-flex min-w-0 items-center gap-1 rounded text-tx-1 outline-none hover:text-tx-0 hover:underline focus-visible:underline';

/**
 * The service's deployments over the time in view, the newest first: what
 * its pipelines reported beside the versions its spans carried, rollbacks
 * told as such, where each came from, and how each version served against
 * the one before it. A verdict opens its two versions in the comparison.
 */
export function DeployTimelineCard({
  address,
  versions,
  onCompare,
}: {
  address: ServiceAddress;
  versions: readonly VersionSummary[];
  onCompare: (previous: string, version: string) => void;
}) {
  const { t } = useTranslation('apm');
  const params = {
    service: address.name,
    namespace: address.namespace || undefined,
    environment: address.scope.environment || undefined,
    from: address.range.from.getTime() * 1_000,
    to: address.range.to.getTime() * 1_000,
  };
  const events = useQuery({
    queryKey: apmDeploymentsQueryKeys.list(address.orgId, params),
    queryFn: () => apmDeploymentsApi.list(params),
    enabled: Boolean(address.orgId && address.name),
    refetchInterval: 60_000,
  });
  // Without the reports the line still has what spans saw.
  const entries = React.useMemo(() => deployTimeline(events.data ?? [], versions), [events.data, versions]);
  const current = versions.reduce<VersionSummary | null>((latest, version) => (!latest || version.lastSeenAt > latest.lastSeenAt ? version : latest), null);
  const reported = (events.data ?? []).length;

  return (
    <ApmCard
      icon={Rocket}
      tone="text-[var(--purple-ink)]"
      title={t('deployments.timeline')}
      testId="deployments"
      actions={
        reported === 0 && (
          <Link to="/datasource/applications/github-actions" className="font-sans text-xs font-strong text-indigo-soft hover:underline">
            {t('deployments.report')}
          </Link>
        )
      }
    >
      {entries.length === 0 ? (
        <p className="px-3 py-6 text-center font-sans text-xs text-tx-3">{t('deployments.none')}</p>
      ) : (
        <div role="table" aria-label={t('deployments.timeline')} className="overflow-x-auto">
          <div role="row" className={cn('grid h-8 min-w-[900px] items-center gap-3 bg-[var(--table-head-surface)] px-3 font-sans text-xs font-strong text-tx-0', GRID)}>
            <span role="columnheader">{t('deployments.columns.time')}</span>
            <span role="columnheader">{t('deployments.columns.version')}</span>
            <span role="columnheader">{t('deployments.columns.event')}</span>
            <span role="columnheader">{t('deployments.columns.change')}</span>
            <span role="columnheader" className="text-right">
              {t('deployments.columns.requests')}
            </span>
            <span role="columnheader" className="text-right">
              {t('deployments.columns.error_rate')}
            </span>
            <span role="columnheader" className="text-right">
              {t('deployments.columns.p95')}
            </span>
            <span role="columnheader">{t('deployments.columns.verdict')}</span>
          </div>
          {entries.map((entry) => (
            <TimelineRow key={entry.key} entry={entry} current={entry.version === current?.version && entry.observed !== undefined} onCompare={onCompare} />
          ))}
        </div>
      )}
    </ApmCard>
  );
}

function TimelineRow({ entry, current, onCompare }: { entry: TimelineEntry; current: boolean; onCompare: (previous: string, version: string) => void }) {
  const { t, i18n } = useTranslation('apm');
  const time = useTimeFormatter();
  const red = entry.observed?.red;
  const verdict = entry.observed?.verdict;

  return (
    <div
      role="row"
      data-apm-deployment={entry.version}
      data-kind={entry.kind}
      className={cn('grid h-8 min-w-[900px] items-center gap-3 border-b border-bd-0 px-3 font-sans text-xs last:border-b-0', GRID)}
    >
      <span className="truncate text-tx-2" title={time.micros(entry.at, false)}>
        {formatRelativeMicros(entry.at, i18n.language)}
      </span>
      <span className="flex min-w-0 items-center gap-2">
        <span className={cn('truncate', entry.version ? 'font-strong text-tx-0' : 'text-tx-3')} title={entry.version}>
          {entry.version || '—'}
        </span>
        {current && <span className="shrink-0 font-strong text-[var(--blue-ink)]">{t('deployments.current')}</span>}
      </span>
      <span className="min-w-0 truncate" title={entry.event?.description}>
        <span className={cn('font-strong', KIND_TEXT[entry.kind])}>{t(`deployments.kind.${entry.kind}`)}</span>
        {entry.from && <span className="text-tx-3"> {t('deployments.from', { version: entry.from })}</span>}
      </span>
      {entry.event ? <Change event={entry.event} /> : <span className="text-tx-3">—</span>}
      <span className="text-right tabular-nums text-tx-1">{red ? formatCount(red.requestCount) : '—'}</span>
      <span className={cn('text-right tabular-nums', red && red.errorCount > 0 ? 'text-[var(--red-ink)]' : 'text-tx-3')}>{red ? formatRate(red.errorRate) : '—'}</span>
      <span className="text-right tabular-nums text-tx-1">{red ? formatDuration(red.p95Time) : '—'}</span>
      {verdict ? (
        <button
          type="button"
          onClick={() => onCompare(verdict.previous, entry.version)}
          data-apm-compare-pair={`${verdict.previous}:${entry.version}`}
          className="-mx-1 flex h-6 min-w-0 items-center rounded px-1 text-left outline-none hover:bg-bg-2 focus-visible:bg-bg-2"
        >
          <VerdictWord verdict={verdict} detailed />
        </button>
      ) : (
        <span className="text-tx-3">—</span>
      )}
    </div>
  );
}

/** Where a reported deployment came from: its commit, pull request, CI run and author. */
function Change({ event }: { event: DeploymentEvent }) {
  const { t } = useTranslation('apm');
  const commit = event.commit?.slice(0, 7);
  const commitHref = event.commit ? commitLink(event.repositoryUrl, event.commit) : null;

  return (
    <span className="flex min-w-0 items-center gap-3 text-tx-2">
      {commit &&
        (commitHref ? (
          <a href={commitHref} target="_blank" rel="noreferrer" className={cn(LINK, 'tabular-nums')} title={event.commit}>
            <GitCommitHorizontal aria-hidden className="h-3.5 w-3.5 shrink-0 text-tx-3" />
            {commit}
          </a>
        ) : (
          <span className="inline-flex items-center gap-1 tabular-nums text-tx-1" title={event.repositoryUrl ? `${event.repositoryUrl} ${event.commit}` : event.commit}>
            <GitCommitHorizontal aria-hidden className="h-3.5 w-3.5 shrink-0 text-tx-3" />
            {commit}
          </span>
        ))}
      {event.pullRequestUrl && (
        <a href={event.pullRequestUrl} target="_blank" rel="noreferrer" className={LINK} title={event.pullRequestUrl}>
          <GitPullRequest aria-hidden className="h-3.5 w-3.5 shrink-0 text-tx-3" />
          {t('deployments.pull_request')}
        </a>
      )}
      {event.ciUrl && (
        <a href={event.ciUrl} target="_blank" rel="noreferrer" className={LINK} title={event.ciUrl}>
          <Workflow aria-hidden className="h-3.5 w-3.5 shrink-0 text-tx-3" />
          {t('deployments.ci_run')}
        </a>
      )}
      {event.author && <span className="min-w-0 truncate text-tx-2">{event.author}</span>}
      {/* What a change other than a deployment changed, as its tool said it. */}
      {event.kind && event.kind !== 'deployment' && event.description && (
        <span className="min-w-0 truncate text-tx-1" title={event.description}>
          {event.description}
        </span>
      )}
    </span>
  );
}
