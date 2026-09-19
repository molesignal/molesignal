import { useQuery } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { RefreshCw } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';
import * as syntheticsApi from '@/api/synthetics';
import type { ProbeLocation, ProbeOutcome, SyntheticResult } from '@/api/synthetics';
import { ChromeButton } from '@/shell/chrome';
import { FormInput, FormSelect } from '@/shell/FormDrawer';
import { ResultPagination } from '@/shell/ResultPagination';

import {
  StatePill,
  SyntheticsCanvas,
  SyntheticsFilterBar,
  SyntheticsListSurface,
  SyntheticsPage,
  WorkspaceBoundary,
} from './components';
import { useSyntheticsWorkspace, type CheckRow } from './data';
import { formatDuration, formatTimestamp, resultDurationMicros } from './model';
import { ResultDrawer } from './ResultDrawer';

const PAGE_SIZE_OPTIONS = [20, 50, 100];
const DEFAULT_PAGE_SIZE = PAGE_SIZE_OPTIONS[0] ?? 20;
const RESULT_OUTCOMES: ProbeOutcome[] = [
  'healthy',
  'flaky',
  'degraded',
  'failing',
  'unknown',
  'skipped',
];

export function Results() {
  const { t, i18n } = useTranslation('synthetics');
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
    queryKey: [
      'synthetics',
      'results',
      deferredQuery,
      outcome,
      locationId,
      page,
      pageSize,
    ],
    queryFn: () =>
      syntheticsApi.listResultPage({
        ...(deferredQuery ? { query: deferredQuery } : {}),
        ...(outcome !== 'all' ? { outcome } : {}),
        ...(locationId !== 'all' ? { location_id: locationId } : {}),
        page,
        per_page: pageSize,
      }),
    placeholderData: (previous) => previous,
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
  React.useEffect(() => {
    if (!resultQuery.data || page <= pageCount) return;
    const next = new URLSearchParams(searchParams);
    if (pageCount <= 1) next.delete('page');
    else next.set('page', String(pageCount));
    setSearchParams(next, { replace: true });
  }, [page, pageCount, resultQuery.data, searchParams, setSearchParams]);
  const refetch = async () => {
    await Promise.all([workspace.refetch(), resultQuery.refetch()]);
  };
  return (
    <SyntheticsPage
      title={t('results.title')}
      subtitle={t('results.subtitle')}
      toolbar={
        <ChromeButton
          onClick={() => void refetch()}
          disabled={workspace.refetching || resultQuery.isFetching}
        >
          <RefreshCw
            aria-hidden
            className={
              workspace.refetching || resultQuery.isFetching
                ? 'h-3.5 w-3.5 animate-spin'
                : 'h-3.5 w-3.5'
            }
          />
          {t('actions.refresh')}
        </ChromeButton>
      }
      bodyClassName="space-y-[12px]"
    >
      <WorkspaceBoundary
        pending={workspace.pending || resultQuery.isPending}
        error={workspace.error ?? resultQuery.error}
        onRetry={() => void refetch()}
        flat
      >
        <SyntheticsCanvas>
          <SyntheticsListSurface>
            <SyntheticsFilterBar
              className="grid gap-2 sm:grid-cols-[minmax(220px,1fr)_180px_200px]"
            >
              <FormInput
                aria-label={t('results.search_placeholder')}
                value={query}
                onChange={(event) => setFilter('query', event.target.value)}
                placeholder={t('results.search_placeholder')}
              />
              <FormSelect
                ariaLabel={t('results.columns.outcome')}
                value={outcome}
                onChange={(value) => setFilter('outcome', value)}
                options={[
                  { value: 'all', label: t('checks.all_states') },
                  ...RESULT_OUTCOMES.map((state) => ({
                    value: state,
                    label: t(`states.${state}`),
                  })),
                ]}
              />
              <FormSelect
                ariaLabel={t('results.columns.location')}
                value={locationId}
                onChange={(value) => setFilter('location', value)}
                options={[
                  { value: 'all', label: t('tabs.locations') },
                  ...workspace.locations.map((location) => ({
                    value: location.id,
                    label: location.name,
                  })),
                ]}
              />
            </SyntheticsFilterBar>
            <div className="overflow-x-auto">
              <DataTable
                rows={results}
                columns={columns(workspace.rows, workspace.locations, i18n.language, t)}
                rowKey={(result) => result.id}
                onRowClick={(result) => navigate({ pathname: `/synthetics/results/${result.id}`, search: searchParams.toString() })}
                emptyLabel={t('states.no_results')}
                className="min-w-[820px] rounded-none border-0 bg-transparent"
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
          </SyntheticsListSurface>
        </SyntheticsCanvas>
      </WorkspaceBoundary>
      <ResultDrawer
        result={selected}
        monitor={selectedRow?.monitor}
        revision={selectedRevision}
        locations={workspace.locations}
        onClose={() => navigate({ pathname: '/synthetics/results', search: searchParams.toString() }, { replace: true })}
      />
    </SyntheticsPage>
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

function columns(
  checks: CheckRow[],
  locations: ProbeLocation[],
  locale: string,
  t: TFunction<'synthetics'>,
): DataTableColumn<SyntheticResult>[] {
  return [
    {
      key: 'check',
      header: t('results.columns.check'),
      width: 220,
      cell: (result) => checks.find((row) => row.monitor.id === result.monitor_id)?.monitor.name ?? result.monitor_id,
    },
    { key: 'outcome', header: t('results.columns.outcome'), width: 110, cell: (result) => <StatePill state={result.outcome} compact /> },
    { key: 'run_type', header: t('results.columns.run_type'), width: 100, cell: (result) => t(result.is_test ? 'results.test_run' : 'results.operational_run') },
    { key: 'location', header: t('results.columns.location'), cell: (result) => locations.find((location) => location.id === result.location_id)?.name ?? result.location_id },
    { key: 'started', header: t('results.columns.started'), width: 170, cell: (result) => formatTimestamp(result.started_at, locale) },
    { key: 'duration', header: t('results.columns.duration'), width: 100, cell: (result) => formatDuration(resultDurationMicros(result)) },
    { key: 'attempts', header: t('results.columns.attempts'), width: 90, cell: (result) => result.attempts.length },
  ];
}
