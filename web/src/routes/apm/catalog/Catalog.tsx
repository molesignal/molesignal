import { useInfiniteQuery, useQuery } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { apmApi, apmQueryKeys } from '@/api/apm';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import { useUsers } from '@/shell/useUsers';

import { CatalogFilters, CatalogGroupingSelect } from './CatalogFilters';
import { CatalogSummary } from './CatalogSummary';
import { CatalogTable } from './CatalogTable';
import { DefinitionDrawer, type DefinitionTarget } from './DefinitionDrawer';
import { groupCatalogRows, tagKeys } from './groups';
import { CATALOG_COLUMNS, catalogRows, type CatalogRow, type CatalogSort, DEFAULT_CATALOG_COLUMNS, filterCatalogRows, sortCatalogRows, tagChoices } from './model';
import { catalogParamWriter, readCatalogParams } from './params';
import { useOnCall, useTeams } from './people';
import { ApmFirstRun } from '../frame/Activation';
import { incidentsAbout, useActiveIncidents } from '../frame/alerts';
import { ApmColumnMenu } from '../frame/ColumnMenu';
import { ApmNavigation } from '../frame/Navigation';
import { ApmCard, ApmPage, ApmQueryState } from '../frame/Page';
import { ApmQualityChip } from '../frame/QualityChip';
import { ApmTimeControls } from '../frame/TimeControls';
import { useApmColumns } from '../frame/useApmColumns';
import { useApmFavorites } from '../frame/useApmFavorites';
import { useApmScope } from '../frame/useApmScope';
import { apmQueryParams, serviceKey, servicePath } from '../scope';
import { useApmSettings } from '../settings/useApmSettings';

const PAGE_SIZE = 200;

/**
 * The service catalog, APM's landing page: every service APM sees and every
 * one declared before it reports, who owns each and how each has served in
 * the time in view.
 */
export function ApmCatalog() {
  const { t } = useTranslation('apm');
  const navigate = useNavigate();
  const { scope, setScope, range, orgId, searchParams, setSearchParams } = useApmScope();
  const filters = readCatalogParams(searchParams);
  const setParam = catalogParamWriter(setSearchParams);
  const favorites = useApmFavorites();
  const metadataRules = useApmSettings().metadataRules;
  const [sort, setSort] = React.useState<CatalogSort>({
    key: 'throughput',
    direction: 'desc',
  });
  const [definition, setDefinition] = React.useState<{
    open: boolean;
    target: DefinitionTarget | null;
  }>({ open: false, target: null });
  const columns = useApmColumns('catalog', CATALOG_COLUMNS, DEFAULT_CATALOG_COLUMNS);
  // Every environment is listed: the environment filter narrows the table, not the question.
  const params = apmQueryParams({ ...scope, environment: '' }, range, {
    limit: PAGE_SIZE,
    sort: 'request_count',
    direction: 'desc',
  });

  const services = useInfiniteQuery({
    queryKey: apmQueryKeys.services(orgId, params),
    queryFn: ({ pageParam }) =>
      apmApi.services({
        ...params,
        ...(pageParam ? { cursor: pageParam } : {}),
      }),
    initialPageParam: '',
    getNextPageParam: (page) => (page.hasMore ? (page.nextCursor ?? undefined) : undefined),
    enabled: Boolean(orgId),
    refetchInterval: 60_000,
  });
  const definitions = useQuery({
    queryKey: apmQueryKeys.definitions(orgId),
    queryFn: apmApi.definitions,
    enabled: Boolean(orgId),
  });
  const { byId: teamsById, teams } = useTeams();
  const { nameOf } = useUsers();

  const all = React.useMemo(
    () => catalogRows(services.data?.pages.flatMap((page) => page.items) ?? [], definitions.data ?? []),
    [definitions.data, services.data],
  );
  const incidents = useActiveIncidents();
  const teamIds = React.useMemo(() => [...new Set(all.flatMap((row) => (row.definition?.teamId ? [row.definition.teamId] : [])))], [all]);
  const onCall = useOnCall(teamIds);
  const names = React.useMemo(
    () => ({
      team: (id: string) => teamsById.get(id)?.name ?? id,
      user: nameOf,
      onCall: (teamId: string) => onCall.get(teamId) ?? null,
      alerts: (row: CatalogRow) =>
        row.environment ? incidentsAbout(incidents.data, { name: row.name, environment: row.environment }) : [],
    }),
    [incidents.data, nameOf, onCall, teamsById],
  );
  const choices = React.useMemo(
    () => ({
      environments: [...new Set(all.flatMap((row) => (row.environment ? [row.environment] : [])))].sort(),
      teams: teams.map((entry) => ({ id: entry.id, name: entry.name })),
      namespaces: [...new Set(all.map((row) => row.namespace))].sort(),
      tags: tagChoices(all),
      tagKeys: tagKeys(all),
    }),
    [all, teams],
  );
  const archivedCount = React.useMemo(() => new Set(all.filter((row) => row.definition?.archived).map(serviceKey)).size, [all]);
  const rows = React.useMemo(
    () =>
      sortCatalogRows(
        filterCatalogRows(all, {
          search: scope.search,
          environment: scope.environment,
          team: filters.team,
          namespace: filters.namespace,
          tag: filters.tag,
          archived: filters.archived,
          only: filters.favorites ? favorites.keys : null,
        }),
        sort,
        names,
        favorites.keys,
      ),
    [all, favorites.keys, filters.archived, filters.favorites, filters.namespace, filters.tag, filters.team, names, scope.environment, scope.search, sort],
  );
  const groups = React.useMemo(() => groupCatalogRows(rows, filters.grouping, names), [filters.grouping, names, rows]);
  const meta = services.data?.pages[0]?.meta;
  const nothing = services.isSuccess && definitions.isSuccess && all.length === 0;

  const open = (row: CatalogRow) => {
    if (row.environment) {
      navigate(
        servicePath(
          {
            namespace: row.namespace,
            name: row.name,
            environment: row.environment,
          },
          'overview',
          searchParams,
        ),
      );
    } else {
      setDefinition({
        open: true,
        target: {
          namespace: row.namespace,
          name: row.name,
          definition: row.definition,
        },
      });
    }
  };

  return (
    <ApmPage
      page="catalog"
      title={t('title')}
      subtitle={t('subtitle')}
      breadcrumbs={null}
      navigation={<ApmNavigation end={<ApmTimeControls scope={scope} onScopeChange={setScope} />} />}
    >
      {nothing ? (
        <ApmFirstRun />
      ) : (
        <>
          <CatalogSummary rows={rows} range={range} quality={<ApmQualityChip meta={meta} />} />
          <ApmCard
            testId="catalog"
            title={
              <CatalogFilters
                scope={scope}
                onScopeChange={setScope}
                params={filters}
                onParam={setParam}
                choices={choices}
                favoriteCount={favorites.keys.size}
                archivedCount={archivedCount}
              />
            }
            actions={
              <>
                <CatalogGroupingSelect grouping={filters.grouping} tagKeys={choices.tagKeys} onChange={(value) => setParam('group', value)} />
                <Button variant="outline" size="sm" onClick={() => setDefinition({ open: true, target: null })} data-apm-declare="">
                  <Plus aria-hidden className="h-3.5 w-3.5" />
                  {t('catalog.declare')}
                </Button>
                <ApmColumnMenu columns={columns} label={(id) => t(`catalog.columns.${id}`)} />
              </>
            }
          >
            <ApmQueryState
              query={services}
              empty={rows.length === 0}
              emptyTitle={t('catalog.filtered_empty')}
              emptyDescription={t('catalog.filtered_empty_description')}
            >
              <CatalogTable
                groups={groups}
                grouping={filters.grouping}
                columns={columns.visible}
                sort={sort}
                onSortChange={setSort}
                range={range}
                names={names}
                favorites={favorites}
                asOfMicros={range.to.getTime() * 1_000}
                rules={metadataRules}
                onOpen={open}
              />
              {services.hasNextPage && (
                <div className="flex justify-center border-t border-bd-0 py-2">
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={services.isFetchingNextPage}
                    onClick={() => void services.fetchNextPage()}
                    className={cn(services.isFetchingNextPage && 'opacity-60')}
                  >
                    {t('catalog.more')}
                  </Button>
                </div>
              )}
            </ApmQueryState>
          </ApmCard>
        </>
      )}
      <DefinitionDrawer open={definition.open} target={definition.target} onClose={() => setDefinition((current) => ({ ...current, open: false }))} />
    </ApmPage>
  );
}
