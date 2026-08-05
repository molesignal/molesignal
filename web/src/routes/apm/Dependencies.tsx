import { useQuery } from '@tanstack/react-query';
import { ArrowRight, Network, Table2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';
import { apmApi, apmQueryKeys, type DependencySummary } from '@/api/apm';
import { CursorPagination } from '@/shell/CursorPagination';

import { ApmPageFrame, QueryBoundary, Section } from './components';
import { ApmFilters } from './Filters';
import { formatCount, formatDuration, formatRate } from './format';
import { hasActiveEntityFilters, servicePath } from './model';
import { useApmFilters } from './useApmFilters';

type DependencyView = 'table' | 'topology';

export function ApmDependencies() {
  const { t } = useTranslation('apm');
  const navigate = useNavigate();
  const [view, setView] = React.useState<DependencyView>('table');
  const { orgId, filters, params, setFilter, clearFilters, pagination } =
    useApmFilters();
  const query = useQuery({
    queryKey: apmQueryKeys.dependencies(orgId, params),
    queryFn: () => apmApi.dependencies(params),
    enabled: Boolean(orgId),
    staleTime: 30_000,
  });
  const needle = filters.search.trim().toLowerCase();
  const rows = (query.data?.items ?? []).filter((row) => {
    if (filters.category && row.dependency.category !== filters.category) return false;
    return needle
      ? `${row.service.name} ${row.dependency.category} ${row.dependency.target} ${row.dependency.operation ?? ''}`
          .toLowerCase()
          .includes(needle)
      : true;
  });

  return (
    <ApmPageFrame
      title={t('dependencies.title')}
      subtitle={t('dependencies.subtitle')}
      meta={query.data?.meta}
      toolbar={
        <ApmFilters
          filters={filters}
          setFilter={setFilter}
          clearFilters={clearFilters}
          showCategory
        />
      }
    >
      <QueryBoundary
        pending={query.isPending}
        error={query.error}
        empty={Boolean(query.data && rows.length === 0)}
        filtered={hasActiveEntityFilters(filters)}
        refetching={query.isFetching && Boolean(query.data)}
        onRetry={() => void query.refetch()}
      >
        {query.data && (
          <div className="space-y-4">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <span className="text-xs text-tx-2">
                {t('dependencies.result_count', { count: rows.length })}
              </span>
              <ViewToggle value={view} onChange={setView} />
            </div>
            <Section
              title={
                view === 'table'
                  ? t('dependencies.ranking')
                  : t('dependencies.topology')
              }
              description={t('dependencies.safe_topology_description')}
            >
              {view === 'table' ? (
                <DataTable
                  rows={rows}
                  columns={columns(t)}
                  rowKey={(row) =>
                    `${row.service.name}:${row.dependency.category}:${row.dependency.target}:${row.version ?? ''}`
                  }
                  onRowClick={(row) => navigate(servicePath(row.service))}
                  emptyLabel={t('states.no_dependencies')}
                />
              ) : (
                <DependencyTopology
                  rows={rows}
                  onCaller={(row) => navigate(servicePath(row.service))}
                />
              )}
            </Section>
            <CursorPagination
              pageSize={pagination.pageSize}
              pageSizeOptions={[20, 50, 100]}
              hasPrevious={Boolean(query.data.previous_cursor)}
              hasNext={Boolean(query.data.next_cursor)}
              pending={query.isFetching}
              ariaLabel={t('actions.pagination')}
              pageSizeAriaLabel={t('actions.page_size')}
              previousLabel={t('actions.previous_page')}
              nextLabel={t('actions.next_page')}
              onPrevious={() => pagination.goPrevious(query.data)}
              onNext={() => pagination.goNext(query.data)}
              onPageSizeChange={pagination.setPageSize}
            />
          </div>
        )}
      </QueryBoundary>
    </ApmPageFrame>
  );
}

function ViewToggle({
  value,
  onChange,
}: {
  value: DependencyView;
  onChange: (value: DependencyView) => void;
}) {
  const { t } = useTranslation('apm');
  return (
    <div className="inline-flex rounded-md border border-bd-0 bg-bg-1 p-0.5">
      {(
        [
          ['table', Table2],
          ['topology', Network],
        ] as const
      ).map(([key, Icon]) => (
        <button
          key={key}
          type="button"
          onClick={() => onChange(key)}
          className={`inline-flex h-7 items-center gap-1.5 rounded px-2.5 text-xs outline-none ${
            value === key
              ? 'bg-bg-3 font-strong text-tx-0'
              : 'text-tx-2 hover:bg-bg-2 focus-visible:bg-bg-2'
          }`}
          aria-pressed={value === key}
        >
          <Icon aria-hidden className="h-3.5 w-3.5" />
          {t(`dependencies.views.${key}`)}
        </button>
      ))}
    </div>
  );
}

function DependencyTopology({
  rows,
  onCaller,
}: {
  rows: DependencySummary[];
  onCaller: (row: DependencySummary) => void;
}) {
  const { t } = useTranslation('apm');
  return (
    <div className="grid gap-px bg-bd-0 md:grid-cols-2 2xl:grid-cols-3">
      {rows.map((row) => (
        <article
          key={`${row.service.name}:${row.dependency.category}:${row.dependency.target}`}
          className="bg-bg-1 p-4"
        >
          <div className="flex items-center gap-3">
            <button
              type="button"
              onClick={() => onCaller(row)}
              className="min-w-0 rounded px-1 py-0.5 text-left outline-none hover:bg-bg-2 focus-visible:bg-bg-2"
            >
              <span className="block truncate text-xs font-strong text-tx-0">
                {row.service.name}
              </span>
              <span className="block truncate text-xs text-tx-3">
                {t('dependencies.caller')}
              </span>
            </button>
            <ArrowRight aria-hidden className="h-4 w-4 shrink-0 text-tx-3" />
            <div className="min-w-0">
              <span className="block truncate text-xs font-strong text-tx-0">
                {row.dependency.target}
              </span>
              <span className="block truncate text-xs text-tx-3">
                {t(`dependency_categories.${row.dependency.category}`)}
              </span>
            </div>
          </div>
          <div className="mt-4 grid grid-cols-3 gap-2 text-xs text-tx-2">
            <span>{formatCount(row.red.request_count)} {t('metrics.requests')}</span>
            <span>{formatRate(row.red.error_rate)}</span>
            <span>{formatDuration(row.red.p95_micros)}</span>
          </div>
        </article>
      ))}
    </div>
  );
}

function columns(t: (key: string) => string): DataTableColumn<DependencySummary>[] {
  return [
    {
      key: 'caller',
      header: t('columns.caller'),
      cell: (row) => (
        <span>
          <span className="block font-strong text-tx-0">{row.service.name}</span>
          <span className="text-xs text-tx-3">{row.service.environment}</span>
        </span>
      ),
    },
    {
      key: 'target',
      header: t('columns.target'),
      cell: (row) => row.dependency.target,
    },
    {
      key: 'category',
      header: t('columns.category'),
      cell: (row) => t(`dependency_categories.${row.dependency.category}`),
    },
    {
      key: 'operation',
      header: t('columns.operation'),
      cell: (row) => row.dependency.operation ?? '—',
    },
    {
      key: 'requests',
      header: t('columns.requests'),
      cell: (row) => formatCount(row.red.request_count),
    },
    {
      key: 'errors',
      header: t('columns.error_rate'),
      cell: (row) => formatRate(row.red.error_rate),
    },
    {
      key: 'p95',
      header: t('columns.p95'),
      cell: (row) => formatDuration(row.red.p95_micros),
    },
    {
      key: 'total',
      header: t('columns.total_time'),
      cell: (row) => formatDuration(row.total_time_micros),
    },
  ];
}
