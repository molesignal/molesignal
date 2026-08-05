import { useQuery } from '@tanstack/react-query';
import { ExternalLink } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';
import { apmApi, apmQueryKeys, type TransactionSummary } from '@/api/apm';
import { CursorPagination } from '@/shell/CursorPagination';

import { ApmPageFrame, QueryBoundary, Section } from '../components';
import { ApmFilters } from '../Filters';
import { formatCount, formatDuration, formatRate } from '../format';
import { hasActiveEntityFilters, signalHref, transactionPath } from '../model';
import { useApmFilters } from '../useApmFilters';

export function ApmTransactions() {
  const { t } = useTranslation('apm');
  const navigate = useNavigate();
  const { orgId, filters, params, setFilter, clearFilters, pagination } =
    useApmFilters();
  const query = useQuery({
    queryKey: apmQueryKeys.transactions(orgId, params),
    queryFn: () => apmApi.transactions(params),
    enabled: Boolean(orgId),
    staleTime: 30_000,
  });
  const needle = filters.search.trim().toLowerCase();
  const rows = (query.data?.items ?? []).filter((row) =>
    needle
      ? `${row.service.name} ${row.transaction.name} ${row.transaction.kind}`
          .toLowerCase()
          .includes(needle)
      : true,
  );

  return (
    <ApmPageFrame
      title={t('transactions.title')}
      subtitle={t('transactions.subtitle')}
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
              sort={filters.sort}
              direction={filters.direction}
              onSort={(value) => setFilter('sort', value)}
              onDirection={(value) => setFilter('direction', value)}
            />
            <Section
              title={t('transactions.ranking')}
              description={t('transactions.ranking_description')}
            >
              <DataTable
                rows={rows}
                columns={columns(t)}
                rowKey={(row) =>
                  `${row.service.namespace}:${row.service.name}:${row.version ?? ''}:${row.transaction.name}`
                }
                onRowClick={(row) => navigate(transactionPath(row))}
                emptyLabel={t('states.no_transactions')}
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
  sort,
  direction,
  onSort,
  onDirection,
}: {
  count: number;
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
      <span className="text-xs text-tx-2">
        {t('transactions.result_count', { count })}
      </span>
      <div className="flex items-center gap-2">
        <select
          aria-label={t('filters.sort')}
          value={sort || 'total_time'}
          onChange={(event) => onSort(event.target.value)}
          className={control}
        >
          <option value="total_time">{t('sort.total_time')}</option>
          <option value="request_count">{t('sort.requests')}</option>
          <option value="error_rate">{t('sort.error_rate')}</option>
          <option value="p95">{t('sort.p95')}</option>
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

function columns(t: (key: string) => string): DataTableColumn<TransactionSummary>[] {
  return [
    {
      key: 'transaction',
      header: t('columns.transaction'),
      cell: (row) => (
        <span>
          <span className="block font-strong text-tx-0">{row.transaction.name}</span>
          <span className="text-xs text-tx-3">{row.transaction.kind}</span>
        </span>
      ),
    },
    { key: 'service', header: t('columns.service'), cell: (row) => row.service.name },
    { key: 'version', header: t('columns.version'), cell: (row) => row.version ?? '—' },
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
    {
      key: 'trace',
      header: '',
      cell: (row) => (
        <Link
          to={signalHref('traces', row.traces, { traceSort: 'duration_desc' })}
          onClick={(event) => event.stopPropagation()}
          className="inline-flex items-center gap-1 rounded px-1.5 py-1 text-xs font-strong text-indigo-soft outline-none hover:bg-bg-2 focus-visible:bg-bg-2"
          aria-label={t('actions.filtered_traces')}
        >
          {t('signals.traces')}
          <ExternalLink aria-hidden className="h-3 w-3" />
        </Link>
      ),
    },
  ];
}
