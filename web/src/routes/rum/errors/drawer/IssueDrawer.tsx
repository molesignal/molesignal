import { History, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { RumIssueDetail } from '@/api/rum';
import { useTimeFormatter } from '@/lib/time';
import { ProductState } from '@/product/states';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import { Sheet, SheetClose, SheetContent, SheetTitle } from '@/shell/ui/sheet';
import { useUsers } from '@/shell/useUsers';

import { RumInvestigateButton } from '../../agent/InvestigateButton';
import { useApplicationRef } from '../../agent/useApplication';
import { type OverviewRange, type OverviewScope, clause, explorerPath } from '../../overview/model';
import { CardState, OverviewCard } from '../../overview/OverviewCard';
import { IssueState } from '../IssueState';
import {
  peopleQueries,
  readPeople,
  readTrend,
  trendQuery,
  trendRange,
  versionSpan,
  WHERE,
  whereQuery,
} from '../model';
import { useIssueAggregate, useIssueDetail } from '../useErrors';
import { IssueControls } from './Controls';
import { PeopleCard, SamplesCard } from './People';
import { StackCard } from './Stack';
import { TrendChart, TrendLegend } from './Trend';
import { WhereCard } from './Where';

const LINK =
  'inline-flex h-8 items-center rounded-md bg-[var(--control-surface)] px-2.5 font-sans text-xs font-strong text-tx-1 outline-none hover:bg-bg-3 focus-visible:bg-bg-3';

/**
 * One issue in a drawer from the right: what it is, its state, its team and the person who has it,
 * how it went over the last two weeks, where it happens, where it was thrown, and the latest
 * sessions it met. Closing the drawer takes the issue out of the address.
 */
export function IssueDrawer({
  id,
  scope,
  range,
  onClose,
}: {
  id: string;
  scope: OverviewScope;
  range: OverviewRange;
  onClose: () => void;
}) {
  const { t } = useTranslation('rum');

  return (
    <Sheet open={id !== ''} onOpenChange={(open) => !open && onClose()}>
      <SheetContent
        side="right"
        hideClose
        overlayClassName="top-topbar bg-overlay-soft"
        aria-describedby={undefined}
        data-rum-issue-drawer=""
        className={cn('bottom-0 top-topbar flex h-auto max-w-none flex-col gap-0 border-0 bg-[var(--functional-surface)] p-0 shadow-drawer sm:max-w-none')}
        style={{ width: 'min(780px, 100vw - 48px)' }}
      >
        <SheetTitle className="sr-only">{t('errors.drawer.title')}</SheetTitle>
        {id !== '' && <DrawerBody key={id} id={id} scope={scope} range={range} onClose={onClose} />}
      </SheetContent>
    </Sheet>
  );
}

function DrawerBody({ id, scope, range, onClose }: { id: string; scope: OverviewScope; range: OverviewRange; onClose: () => void }) {
  const { t, i18n } = useTranslation('rum');
  const language = i18n.resolvedLanguage ?? i18n.language;
  const detail = useIssueDetail(id, range);
  const issue = detail.data;

  // The issue is of the application it names, whatever application the page is looking at: an
  // address that carries the issue alone still opens it right.
  const own = React.useMemo(
    () => ({ application: issue?.applicationId ?? scope.application, environment: scope.environment }),
    [issue?.applicationId, scope.application, scope.environment],
  );

  const application = useApplicationRef(own.application);
  const trendWindow = React.useMemo(() => trendRange(new Date()), []);
  const trend = useIssueAggregate('trend', trendQuery(own, trendWindow, id));
  const people = peopleQueries(own, range, id);
  const known = useIssueAggregate('known', people.known);
  const anonymous = useIssueAggregate('anonymous', people.anonymous);
  const browser = useIssueAggregate('browser', whereQuery(own, range, id, WHERE[0].field));
  const system = useIssueAggregate('system', whereQuery(own, range, id, WHERE[1].field));
  const country = useIssueAggregate('country', whereQuery(own, range, id, WHERE[2].field));

  const drawn = readTrend(trend.data, trendWindow);

  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
      <header className="flex shrink-0 flex-wrap items-start justify-between gap-x-4 gap-y-3 border-b border-bd-0 px-5 py-4">
        <div className="min-w-0">
          <h2 className="m-0 truncate type-page-title font-sans font-display text-tx-0" data-rum-issue-title="">
            {issue ? issue.errorType || (issue.overflow ? t('errors.overflow.title') : t('errors.drawer.untyped')) : t('errors.drawer.title')}
          </h2>
          <p className="m-0 truncate font-sans text-xs text-tx-3">
            {[own.application, own.environment, issue?.source].filter(Boolean).join(' · ')}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {issue && <IssueControls issue={issue} />}
          {issue && (
            <RumInvestigateButton
              look="label"
              label={t('agent.investigate_short')}
              title={t('agent.investigate_title')}
              // The drawer is over the panel: closing it leaves the question to be reached.
              onStarted={onClose}
              subject={{
                application,
                environment: own.environment,
                range,
                focus: { kind: 'issue', id, type: issue.errorType, message: issue.message },
              }}
            />
          )}
          <Link to={explorerPath('errors', own, [clause('error.issue_id', id)])} className={LINK} data-rum-issue-explore="">
            {t('errors.drawer.explore')}
          </Link>
          <SheetClose asChild>
            <Button variant="ghost" size="icon" className="h-8 w-8" aria-label={t('errors.drawer.close')}>
              <X className="h-4 w-4" />
            </Button>
          </SheetClose>
        </div>
      </header>
      <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-4">
        {detail.isPending ? (
          <ProductState variant="loading" compact />
        ) : detail.isError || !issue ? (
          <ProductState variant="error" error={detail.error} compact />
        ) : (
          <>
            <Facts issue={issue} language={language} />
            <Message issue={issue} />
            {issue.status === 'regressed' && <Banner issue={issue} />}
            <OverviewCard
              icon={History}
              tone="text-[var(--chart-1)]"
              title={t('errors.trend.title')}
              description={t('errors.trend.description', { days: drawn?.days.length ?? 14 })}
              testId="issue-trend"
            >
              <CardState query={trend} empty={drawn === null || drawn.series.length === 0} emptyTitle={t('errors.trend.empty')}>
                {drawn && (
                  <div className="space-y-2 px-3 pb-3 pt-2">
                    <div className="flex">
                      <TrendLegend trend={drawn} />
                    </div>
                    <TrendChart
                      trend={drawn}
                      resolvedAt={issue.resolvedAt}
                      resolvedVersion={issue.resolvedVersion}
                      from={trendWindow.from}
                      language={language}
                    />
                  </div>
                )}
              </CardState>
            </OverviewCard>
            <WhereCard reads={{ browser, system, country }} language={language} />
            <StackCard stack={issue.stack} />
            <div className="flex flex-wrap items-stretch gap-3">
              <SamplesCard samples={issue.samples} />
              <PeopleCard people={readPeople(known.data, anonymous.data)} />
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function Fact({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <span className="inline-flex items-baseline gap-1.5">
      <span className="text-tx-3">{label}</span>
      {children}
    </span>
  );
}

/** What the issue comes to: its state, how many sessions and people it met in the range, when and in which versions. */
function Facts({ issue, language }: { issue: RumIssueDetail; language: string }) {
  const { t } = useTranslation('rum');
  const formatter = useTimeFormatter();

  return (
    <div data-rum-issue-facts="" className="flex flex-wrap gap-x-6 gap-y-1.5 font-sans text-sm">
      <Fact label={t('errors.drawer.facts.state')}>
        <IssueState status={issue.status} />
      </Fact>
      <Fact label={t('errors.drawer.facts.sessions')}>
        <span className="font-strong tabular-nums text-tx-0">{new Intl.NumberFormat(language).format(issue.sessions)}</span>
      </Fact>
      <Fact label={t('errors.drawer.facts.users')}>
        <span className="font-strong tabular-nums text-tx-0">{new Intl.NumberFormat(language).format(issue.users)}</span>
      </Fact>
      <Fact label={t('errors.drawer.facts.first_seen')}>
        <span className="text-tx-0">{formatter.micros(issue.firstSeenAt, false)}</span>
      </Fact>
      <Fact label={t('errors.drawer.facts.last_seen')}>
        <span className="text-tx-0">{formatter.micros(issue.lastSeenAt, false)}</span>
      </Fact>
      {versionSpan(issue) && (
        <Fact label={t('errors.drawer.facts.versions')}>
          <span className="tabular-nums text-tx-0">{versionSpan(issue)}</span>
        </Fact>
      )}
    </div>
  );
}

/** What the error says, with where it comes from, how it was handled, and the id of the issue. */
function Message({ issue }: { issue: RumIssueDetail }) {
  const { t } = useTranslation('rum');
  const message = issue.stack?.message ?? issue.message;
  const meta = [
    issue.source && `${t('errors.drawer.source')} ${issue.source}`,
    issue.stack?.handling && `${t('errors.drawer.handling')} ${issue.stack.handling}`,
    `${t('errors.drawer.issue_id')} ${issue.id}`,
  ].filter(Boolean);

  return (
    <div>
      <p data-rum-issue-message="" className="m-0 break-words font-sans text-sm font-strong text-tx-0">
        {issue.overflow ? t('errors.overflow.description') : message}
      </p>
      <p className="m-0 mt-0.5 break-all font-sans text-xs text-tx-3">{meta.join(' · ')}</p>
    </div>
  );
}

/** Why an issue that was resolved is open again: when it was resolved and in which version, and where it came back. */
function Banner({ issue }: { issue: RumIssueDetail }) {
  const { t } = useTranslation('rum');
  const formatter = useTimeFormatter();
  const { nameOf } = useUsers();

  const who = issue.resolvedBy ? nameOf(issue.resolvedBy) : '';
  const resolved = t(`errors.banner.resolved${who ? '_by' : ''}${issue.resolvedVersion ? '_in' : ''}`, {
    when: issue.resolvedAt === null ? '' : formatter.micros(issue.resolvedAt, false),
    who,
    version: issue.resolvedVersion ?? '',
  });
  const back = t(issue.regressedVersion ? 'errors.banner.back_in' : 'errors.banner.back', {
    version: issue.regressedVersion ?? '',
    when: issue.regressedAt === null ? '' : formatter.micros(issue.regressedAt, false),
  });

  return (
    <div data-rum-issue-banner="" role="note" className="rounded-lg bg-red-dim px-3.5 py-2.5 font-sans text-sm text-red-soft">
      <span className="mr-2 font-strong">{t('errors.state.regressed')}</span>
      {resolved} {back}
      {issue.resolvedVersion && ` ${t('errors.banner.older', { version: issue.resolvedVersion })}`}
    </div>
  );
}
