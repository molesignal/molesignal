import { useQuery } from '@tanstack/react-query';
import { Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';

import { DataTable } from '@/admin';
import * as syntheticsApi from '@/api/synthetics';
import type { ProbeOutcome } from '@/api/synthetics';
import { formatMicrosActive } from '@/lib/time';
import { FormInput, FormSelect } from '@/shell/FormDrawer';
import { ResultPagination } from '@/shell/ResultPagination';

import { SYNTHETICS_REFRESH_MILLIS, useSyntheticsWorkspace } from '../data';
import { SyntheticsPage, useLoadState } from '../Frame';
import { OutcomeMark } from '../marks';
import { formatDuration, resultDurationMicros } from '../model';
import { ResultDrawer } from './Drawer';

const PAGE_SIZE_OPTIONS = [20, 50, 100];
const DEFAULT_PAGE_SIZE = PAGE_SIZE_OPTIONS[0] ?? 20;
const RESULT_OUTCOMES: ProbeOutcome[] = [
  'failing',
  'flaky',
  'degraded',
  'healthy',
  'unknown',
  'skipped',
];

/**
 * Every run of every check, the latest first: the record a person searches
 * when they want the evidence of one run. The search, the outcome and the
 * location narrow it on the server, so the count is of all that match and not
 * of the page in view.
 */
export function Results() {
  const { t } = useTranslation('synthetics');
  const navigate = useNavigate();
  const { resultId } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const workspace = useSyntheticsWorkspace({ includeArchived: true, includeResults: false });
  const query = searchParams.get('query') ?? '';
  const deferredQuery = React.useDeferredValue(query.trim());
  const outcome = resultOutcomeFrom(searchParams.get('outcome'));
  const locationId = searchParams.get('location') ?? 'all';
  const page = positiveInteger(searchParams.get('page'), 1);
  const pageSize = pageSizeFrom(searchParams.get('page_size'));
  const resultQuery = useQuery({
    queryKey: ['synthetics', 'results', deferredQuery, outcome, locationId, page, pageSize],
    queryFn: () =>
      syntheticsApi.listResultPage({
        ...(deferredQuery ? { query: deferredQuery } : {}),
        ...(outcome !== 'all' ? { outcome } : {}),
        ...(locationId !== 'all' ? { location_id: locationId } : {}),
        page,
        per_page: pageSize,
      }),
    placeholderData: (previous) => previous,
    refetchInterval: SYNTHETICS_REFRESH_MILLIS,
  });
  const results = resultQuery.data?.items ?? [];
  const total = resultQuery.data?.total ?? 0;
  const pageCount = Math.max(1, Math.ceil(total / pageSize));
  const currentPage = Math.min(page, pageCount);
  const selected = results.find((result) => result.id === resultId);
  const selectedRow = workspace.rows.find((row) => row.monitor.id === selected?.monitor_id);
  const selectedRevision = selectedRow?.detail?.revisions.find(
    (revision) => revision.id === selected?.monitor_revision_id,
  );
  const setFilter = (key: 'query' | 'outcome' | 'location', value: string) => {
    const next = new URLSearchParams(searchParams);
    if (value === 'all' || !value.trim()) next.delete(key);
    else next.set(key, value);
    next.delete('page');
    setSearchParams(next, { replace: true });
  };
  const setPage = (nextPage: number) => {
    const next = new URLSearchParams(searchParams);
    if (nextPage <= 1) next.delete('page');
    else next.set('page', String(nextPage));
    setSearchParams(next, { replace: true });
  };
  const setPageSize = (nextPageSize: number) => {
    const next = new URLSearchParams(searchParams);
    if (nextPageSize === DEFAULT_PAGE_SIZE) next.delete('page_size');
    else next.set('page_size', String(nextPageSize));
    next.delete('page');
    setSearchParams(next, { replace: true });
  };
  // A filter can leave fewer pages than the one being read: step back to the last that exists.
  React.useEffect(() => {
    if (!resultQuery.data || page <= pageCount) return;
    const next = new URLSearchParams(searchParams);
    if (pageCount <= 1) next.delete('page');
    else next.set('page', String(pageCount));
    setSearchParams(next, { replace: true });
  }, [page, pageCount, resultQuery.data, searchParams, setSearchParams]);

  const state = useLoadState({
    pending: workspace.pending || resultQuery.isPending,
    error: workspace.error ?? resultQuery.error,
    onRetry: () => {
      void workspace.refetch();
      void resultQuery.refetch();
    },
  });
  const checkName = (monitorId: string) =>
    workspace.rows.find((row) => row.monitor.id === monitorId)?.monitor.name ?? monitorId;
  const locationName = (id: string) =>
    workspace.locations.find((location) => location.id === id)?.name ?? id;

  return (
    <>
      <SyntheticsPage
        state={state}
        listHead={
          <>
            <label className="relative w-full min-w-0 sm:w-[300px]">
              <span className="sr-only">{t('results.search_placeholder')}</span>
              <Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-tx-3" />
              <FormInput
                value={query}
                onChange={(event) => setFilter('query', event.target.value)}
                placeholder={t('results.search_placeholder')}
                className="pl-9"
              />
            </label>
            <FormSelect
              ariaLabel={t('results.columns.outcome')}
              className="w-full sm:w-[170px]"
              value={outcome}
              onChange={(value) => setFilter('outcome', value)}
              options={[
                { value: 'all', label: t('results.all_outcomes') },
                ...RESULT_OUTCOMES.map((item) => ({ value: item, label: t(`states.${item}`) })),
              ]}
            />
            <FormSelect
              ariaLabel={t('results.columns.location')}
              className="w-full sm:w-[200px]"
              value={locationId}
              onChange={(value) => setFilter('location', value)}
              options={[
                { value: 'all', label: t('results.all_locations') },
                ...workspace.locations.map((location) => ({
                  value: location.id,
                  label: location.name,
                })),
              ]}
            />
          </>
        }
      >
        <div className="overflow-x-auto">
          <DataTable
            rows={results}
            rowKey={(result) => result.id}
            onRowClick={(result) =>
              navigate({
                pathname: `/synthetics/results/${result.id}`,
                search: searchParams.toString(),
              })
            }
            emptyLabel={t('states.no_results')}
            className="min-w-[820px] rounded-none border-0 bg-transparent"
            columns={[
              {
                key: 'check',
                header: t('results.columns.check'),
                cell: (result) => (
                  <span className="block truncate font-strong text-tx-0">
                    {checkName(result.monitor_id)}
                  </span>
                ),
              },
              {
                key: 'outcome',
                header: t('results.columns.outcome'),
                width: 190,
                cell: (result) => (
                  <span className="inline-flex min-w-0 items-center gap-[10px]">
                    <OutcomeMark outcome={result.outcome} />
                    {/* Most runs are scheduled; only the ones a person asked for say so. */}
                    {result.is_test && (
                      <span className="shrink-0 text-xs text-tx-3">{t('results.test_run')}</span>
                    )}
                  </span>
                ),
              },
              {
                key: 'location',
                header: t('results.columns.location'),
                width: 180,
                cell: (result) => (
                  <span className="block truncate text-tx-1">{locationName(result.location_id)}</span>
                ),
              },
              {
                key: 'started',
                header: t('results.columns.started'),
                width: 190,
                cell: (result) => (
                  <span className="tabular-nums text-tx-1">
                    {formatMicrosActive(result.started_at)}
                  </span>
                ),
              },
              {
                key: 'duration',
                header: t('results.columns.duration'),
                width: 110,
                cell: (result) => (
                  <span className="tabular-nums text-tx-1">
                    {formatDuration(resultDurationMicros(result))}
                  </span>
                ),
              },
              {
                key: 'attempts',
                header: t('results.columns.attempts'),
                width: 100,
                cell: (result) => (
                  <span className="tabular-nums text-tx-1">{result.attempts.length}</span>
                ),
              },
            ]}
          />
        </div>
        <ResultPagination
          page={currentPage}
          pageCount={pageCount}
          pageSize={pageSize}
          pageSizeOptions={PAGE_SIZE_OPTIONS}
          pageLabel={t('results.pagination.page', {
            page: currentPage,
            pages: pageCount,
            total,
          })}
          ariaLabel={t('results.pagination.label')}
          pageSizeAriaLabel={t('results.pagination.page_size')}
          firstAriaLabel={t('results.pagination.first')}
          previousAriaLabel={t('results.pagination.previous')}
          nextAriaLabel={t('results.pagination.next')}
          lastAriaLabel={t('results.pagination.last')}
          onPageChange={setPage}
          onPageSizeChange={setPageSize}
        />
      </SyntheticsPage>
      <ResultDrawer
        result={selected}
        monitor={selectedRow?.monitor}
        revision={selectedRevision}
        locations={workspace.locations}
        onClose={() =>
          navigate(
            { pathname: '/synthetics/results', search: searchParams.toString() },
            { replace: true },
          )
        }
      />
    </>
  );
}

function positiveInteger(value: string | null, fallback: number): number {
  const parsed = Number(value);
  return Number.isInteger(parsed) && parsed > 0 ? parsed : fallback;
}

function pageSizeFrom(value: string | null): number {
  const parsed = Number(value);
  return PAGE_SIZE_OPTIONS.includes(parsed) ? parsed : DEFAULT_PAGE_SIZE;
}

function resultOutcomeFrom(value: string | null): ProbeOutcome | 'all' {
  return RESULT_OUTCOMES.find((outcome) => outcome === value) ?? 'all';
}
