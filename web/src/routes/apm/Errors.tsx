import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { useLocation, useNavigate } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';
import { apmApi, apmQueryKeys, type ErrorSummary } from '@/api/apm';
import { CursorPagination } from '@/shell/CursorPagination';

import { ApmPageFrame, QueryBoundary, Section } from './components';
import { ApmFilters } from './Filters';
import { formatCount, formatRate, formatTimestamp } from './format';
import { hasActiveEntityFilters } from './model';
import { useApmFilters } from './useApmFilters';

export function ApmErrors() {
  const { t } = useTranslation('apm');
  const location = useLocation();
  const navigate = useNavigate();
  const { orgId, filters, params, setFilter, clearFilters, pagination } =
    useApmFilters();
  const query = useQuery({
    queryKey: apmQueryKeys.errors(orgId, params),
    queryFn: () => apmApi.errors(params),
    enabled: Boolean(orgId),
    staleTime: 30_000,
  });
  const needle = filters.search.trim().toLowerCase();
  const rows = (query.data?.items ?? []).filter((row) =>
    needle
      ? `${row.error.error_type} ${row.representative_message ?? ''} ${row.service.name}`
          .toLowerCase()
          .includes(needle)
      : true,
  );

  return (
    <ApmPageFrame
      title={t('errors.title')}
      subtitle={t('errors.subtitle')}
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
            <div className="text-xs text-tx-2">
              {t('errors.result_count', { count: rows.length })}
            </div>
            <Section
              title={t('errors.groups')}
              description={t('errors.groups_description')}
            >
              <DataTable
                rows={rows}
                columns={columns(t)}
                rowKey={(row) => row.error.fingerprint}
                onRowClick={(row) =>
                  navigate(
                    `/apm/errors/${encodeURIComponent(row.error.fingerprint)}${location.search}`,
                  )
                }
                emptyLabel={t('states.no_errors')}
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

function columns(t: (key: string) => string): DataTableColumn<ErrorSummary>[] {
  return [
    {
      key: 'error',
      header: t('columns.error'),
      cell: (row) => (
        <span>
          <span className="block font-strong text-tx-0">{row.error.error_type}</span>
          <span className="block max-w-[380px] truncate text-xs text-tx-3">
            {row.representative_message ?? '—'}
          </span>
        </span>
      ),
    },
    {
      key: 'service',
      header: t('columns.service'),
      cell: (row) => row.service.name,
    },
    {
      key: 'transaction',
      header: t('columns.transaction'),
      cell: (row) => row.error.transaction_name ?? '—',
    },
    {
      key: 'occurrences',
      header: t('columns.occurrences'),
      cell: (row) => formatCount(row.occurrence_count),
    },
    {
      key: 'rate',
      header: t('columns.error_rate'),
      cell: (row) => formatRate(row.red.error_rate),
    },
    {
      key: 'last_seen',
      header: t('columns.last_seen'),
      cell: (row) => formatTimestamp(row.last_seen_at),
    },
  ];
}
