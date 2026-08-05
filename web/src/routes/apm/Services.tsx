import { useQuery } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { Cpu, RadioTower } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';
import { apmApi, apmQueryKeys, type ServiceSummary } from '@/api/apm';
import { CursorPagination } from '@/shell/CursorPagination';

import { ApmPageFrame, HealthDot, QueryBoundary, Section } from './components';
import { ApmFilters } from './Filters';
import { formatCount, formatDuration, formatRate, formatTimestamp } from './format';
import { hasActiveEntityFilters, servicePath } from './model';
import { useApmFilters } from './useApmFilters';

export function ApmServices() {
  const { t } = useTranslation('apm');
  const navigate = useNavigate();
  const { orgId, filters, params, setFilter, clearFilters, pagination } =
    useApmFilters();
  const query = useQuery({
    queryKey: apmQueryKeys.services(orgId, params),
    queryFn: () => apmApi.services(params),
    enabled: Boolean(orgId),
    staleTime: 30_000,
  });
  const search = filters.search.trim().toLowerCase();
  const rows = (query.data?.items ?? []).filter((row) => {
    if (!search) return true;
    return `${row.service.namespace} ${row.service.name} ${row.service.environment} ${row.versions.join(' ')}`
      .toLowerCase()
      .includes(search);
  });
  return (
    <ApmPageFrame
      title={t('services.title')}
      subtitle={t('services.subtitle')}
      meta={query.data?.meta}
      toolbar={
        <ApmFilters
          filters={filters}
          setFilter={setFilter}
          clearFilters={clearFilters}
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
            <ResultControls
              count={rows.length}
              completeThrough={query.data.meta.last_complete_bucket_at}
              sort={filters.sort}
              direction={filters.direction}
              onSort={(value) => setFilter('sort', value)}
              onDirection={(value) => setFilter('direction', value)}
            />
            <Section title={t('services.catalog')}>
              <DataTable
                rows={rows}
                columns={columns(t)}
                rowKey={(row) => row.service.namespace + row.service.name + row.service.environment}
                onRowClick={(row) => navigate(servicePath(row.service))}
                emptyLabel={t('states.no_services')}
              />
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

function ResultControls({
  count,
  completeThrough,
  sort,
  direction,
  onSort,
  onDirection,
}: {
  count: number;
  completeThrough?: number | undefined;
  sort: string;
  direction: string;
  onSort: (value: string) => void;
  onDirection: (value: string) => void;
}) {
  const { t } = useTranslation('apm');
  const control =
    'h-8 rounded-md border border-bd-0 bg-bg-1 px-2.5 text-xs text-tx-1 outline-none hover:bg-bg-2 focus-visible:bg-bg-2';
  return (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <div className="flex flex-wrap gap-x-4 gap-y-1 text-xs text-tx-2">
        <span>{t('services.result_count', { count })}</span>
        <span>
          {t('quality.complete_through', {
            time: formatTimestamp(completeThrough),
          })}
        </span>
      </div>
      <div className="flex items-center gap-2">
        <select
          aria-label={t('filters.sort')}
          value={sort || 'request_count'}
          onChange={(event) => onSort(event.target.value)}
          className={control}
        >
          <option value="request_count">{t('sort.requests')}</option>
          <option value="error_rate">{t('sort.error_rate')}</option>
          <option value="p95">{t('sort.p95')}</option>
          <option value="name">{t('sort.name')}</option>
        </select>
        <select
          aria-label={t('filters.direction')}
          value={direction}
          onChange={(event) => onDirection(event.target.value)}
          className={control}
        >
          <option value="desc">{t('sort.descending')}</option>
          <option value="asc">{t('sort.ascending')}</option>
        </select>
      </div>
    </div>
  );
}

function columns(t: TFunction<'apm'>): DataTableColumn<ServiceSummary>[] {
  return [
    {
      key: 'service',
      header: t('columns.service'),
      cell: (row) => (
        <div className="flex items-center gap-2.5">
          <HealthDot status={row.health} />
          <div className="min-w-0">
            <div className="truncate font-strong text-tx-0">{row.service.name}</div>
            <div className="truncate text-xs text-tx-3">
              {row.service.namespace} / {row.service.environment}
            </div>
          </div>
        </div>
      ),
    },
    {
      key: 'instrumentation',
      header: t('columns.instrumentation'),
      cell: (row) => (
        <div className="flex items-center gap-2 text-xs">
          <Cpu aria-hidden className="h-3.5 w-3.5 text-tx-3" />
          <span>{row.instrumentation.runtime_language ?? t('values.unknown')}</span>
          <RadioTower aria-hidden className="ml-1 h-3.5 w-3.5 text-tx-3" />
          <span>{formatCount(row.instrumentation.recent_instance_count)}</span>
        </div>
      ),
    },
    {
      key: 'versions',
      header: t('columns.versions'),
      cell: (row) =>
        row.versions.length > 0 ? (
          <span className="font-mono text-xs">{row.versions.slice(0, 3).join(', ')}</span>
        ) : (
          '—'
        ),
    },
    { key: 'requests', header: t('columns.requests'), cell: (row) => formatCount(row.red.request_count) },
    { key: 'errors', header: t('columns.error_rate'), cell: (row) => formatRate(row.red.error_rate) },
    { key: 'p95', header: t('columns.p95'), cell: (row) => formatDuration(row.red.p95_micros) },
    { key: 'last_seen', header: t('columns.last_seen'), cell: (row) => formatTimestamp(row.last_seen_at) },
  ];
}
