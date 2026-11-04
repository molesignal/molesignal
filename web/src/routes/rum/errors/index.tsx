import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as rumApi from '@/api/rum';
import type { RumApplication, RumFilter, RumIssueListParams, RumIssueSort } from '@/api/rum';
import { widenTimeWindow } from '@/investigation/timeRangeRecovery';
import { ProductState } from '@/product/states';
import { useTeams } from '@/routes/apm/catalog/people';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';
import { useUsers } from '@/shell/useUsers';
import { TimeRangeControl } from '@/time/TimePicker';

import { RumInvestigateButton } from '../agent/InvestigateButton';
import { byName } from '../applications/model';
import { RumFirstRun } from '../explorer/FirstRun';
import { useRumTimeWindow } from '../explorer/timeWindow';
import { CONTROL, ScopeRow } from '../overview/ScopeRow';
import { useAggregate, useEnvironments, useOverviewRange, useOverviewScope } from '../overview/useOverview';
import { RumListPage } from '../RumLayout';
import { IssueDrawer } from './drawer/IssueDrawer';
import { IssueTable } from './IssueTable';
import { ListHead } from './ListHead';
import { DEFAULT_SORT, defaultOrder, DIMENSIONS, type DimensionKey, LIST_LIMIT, pickedFilters, spreadQuery } from './model';
import { SpreadCard } from './Spread';
import { useErrorsView, useIssueList, useSources } from './useErrors';

/**
 * The errors of RUM as issues: the errors of an application that are alike are one thing people can
 * act on, with a state, a team and a person, and the versions and the time it was seen in. The page
 * lists the issues with errors in the range, says where they happen, and opens one in a drawer.
 *
 * Until the first application exists there is nothing to look at, and the page says how to start.
 */
export function RumErrors() {
  const applications = useQuery({
    queryKey: ['rum', 'applications'],
    queryFn: () => rumApi.listApplications(),
  });

  if (applications.isPending) return <RumListPage state={{ variant: 'loading' }} />;

  if (applications.isError) return <RumListPage state={{ variant: 'error', error: applications.error }} />;

  if (applications.data.length === 0) {
    return (
      <RumListPage>
        <RumFirstRun embedded />
      </RumListPage>
    );
  }

  return <ErrorsPage applications={byName(applications.data)} />;
}

function ErrorsPage({ applications }: { applications: readonly RumApplication[] }) {
  const { t, i18n } = useTranslation('rum');
  const language = i18n.resolvedLanguage ?? i18n.language;
  const { params, application, environment, setApplication, setEnvironment } = useOverviewScope(applications);
  const { effectiveTimeWindow, setTimeWindow } = useRumTimeWindow(params);
  const range = useOverviewRange(effectiveTimeWindow);
  const { view, set } = useErrorsView();
  const { users, nameOf } = useUsers();
  const { byId: teamsById } = useTeams();

  // The list is not empty, and the scope falls back to the first of it.
  const current = application as RumApplication;
  const scope = React.useMemo(() => ({ application: current.id, environment }), [current.id, environment]);
  const environments = useEnvironments(current.id, range);
  const sources = useSources(current.id, range);

  const narrowed = React.useMemo<RumFilter[]>(() => pickedFilters(view.picked), [view.picked]);
  const sourceFilter = React.useMemo<RumFilter[]>(
    () => (view.source ? [{ field: 'source', op: '=', value: view.source }] : []),
    [view.source],
  );

  const listParams = React.useMemo<RumIssueListParams>(
    () => ({
      application: current.id,
      from: range.from,
      to: range.to,
      status: view.status,
      sort: view.sort,
      limit: LIST_LIMIT,
      ...(environment && { env: environment }),
      ...(view.source && { source: view.source }),
      ...(view.assignee && { assignee: view.assignee }),
      ...(view.search.trim() && { search: view.search.trim() }),
      ...(view.order && { order: view.order }),
      ...(narrowed.length > 0 && { filters: narrowed }),
    }),
    [current.id, range, view.status, view.sort, view.source, view.assignee, view.search, view.order, environment, narrowed],
  );

  const list = useIssueList(listParams);
  const rows = React.useMemo(() => list.data?.issues ?? [], [list.data]);
  const ids = React.useMemo(() => rows.map((row) => row.id), [rows]);

  const spread = (dimension: (typeof DIMENSIONS)[number]) =>
    ids.length > 0 ? spreadQuery(scope, range, ids, view.picked, dimension, sourceFilter) : null;
  const spreadReads = {
    version: useAggregate('errors-version', spread(DIMENSIONS[0])),
    page: useAggregate('errors-page', spread(DIMENSIONS[1])),
    browser: useAggregate('errors-browser', spread(DIMENSIONS[2])),
  };

  const onPick = (key: DimensionKey, value: string | null) => {
    const param = DIMENSIONS.find((dimension) => dimension.key === key)?.param;

    if (param) set({ [param]: value });
  };

  const onSort = (next: RumIssueSort) => {
    if (next === view.sort) {
      set({ order: (view.order ?? defaultOrder(view.sort)) === 'desc' ? 'asc' : 'desc' });
    } else {
      set({ sort: next === DEFAULT_SORT ? null : next, order: null });
    }
  };

  const scopeRow = (
    <ScopeRow
      applications={applications}
      application={current.id}
      environment={environment}
      environments={(environments.data?.values ?? []).map((entry) => entry.value)}
      range={range}
      onApplication={setApplication}
      onEnvironment={setEnvironment}
    />
  );

  const unavailable = list.data?.unavailable;
  const now = Date.now() * 1_000;

  const empty = (
    <ProductState
      variant="empty"
      compact
      title={t(view.status === 'unresolved' ? 'errors.empty.unresolved' : 'errors.empty.none')}
      description={t('errors.empty.description')}
      action={
        <span className="flex flex-wrap items-center justify-center gap-2">
          {view.status !== 'all' && <ChromeButton onClick={() => set({ status: 'all' })}>{t('errors.empty.show_all')}</ChromeButton>}
          <ChromeButton onClick={() => setTimeWindow(widenTimeWindow(effectiveTimeWindow))}>{t('overview.empty.widen')}</ChromeButton>
        </span>
      }
    />
  );

  return (
    <RumListPage
      toolbar={
        <RumInvestigateButton
          subject={{ application: { id: current.id, name: current.name }, environment, range, focus: { kind: 'errors' } }}
        />
      }
      navigationEnd={<TimeRangeControl align="end" className={cn(CONTROL, 'max-w-[220px]')} />}
      navigationScope={scopeRow}
    >
      <div data-rum-errors="" className="space-y-[12px]">
        {list.isPending ? (
          <ProductState variant="loading" />
        ) : list.isError ? (
          <ProductState variant="error" error={list.error} />
        ) : (
          <>
            {rows.length > 0 && <SpreadCard reads={spreadReads} picked={view.picked} onPick={onPick} language={language} />}
            <section data-rum-overview-card="issues" className={cn(surfacePanelClass, 'min-w-0 overflow-hidden')}>
              <ListHead
                view={view}
                onChange={set}
                onPick={onPick}
                users={users}
                total={unavailable ? null : (list.data?.total ?? 0)}
                sources={sources.data ?? []}
                source={view.source}
                onSource={(source) => set({ source: source || null })}
              />
              {unavailable ? (
                <ProductState variant="empty" compact title={t(`overview.unavailable.${unavailable}`)} description="" />
              ) : rows.length === 0 ? (
                empty
              ) : (
                <IssueTable
                  rows={rows}
                  sort={view.sort}
                  order={view.order}
                  onSort={onSort}
                  onOpen={(id) => set({ issue: id }, { push: true })}
                  names={{ user: nameOf, team: (id) => teamsById.get(id)?.name ?? id }}
                  now={now}
                  language={language}
                />
              )}
            </section>
          </>
        )}
      </div>
      <IssueDrawer id={view.issue} scope={scope} range={range} onClose={() => set({ issue: null })} />
    </RumListPage>
  );
}
