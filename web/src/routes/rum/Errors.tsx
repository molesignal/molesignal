import { useQuery } from '@tanstack/react-query';
import { ArrowDownRight, ArrowRight, ArrowUpRight, Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate } from 'react-router-dom';

import * as rumApi from '@/api/rum';
import { useCursorPagination } from '@/pagination/useCursorPagination';
import { productStateFor } from '@/product/states';
import { ChromeButton, Pill, TimeRangeChip } from '@/shell/chrome';
import { CursorPagination } from '@/shell/CursorPagination';
import { queryStateFor } from '@/shell/query/State';
import { useAuthStore } from '@/stores/auth';
import { formatWindowSummary, useTimeStore } from '@/stores/useTimeStore';

import { formatMicros, windowToMicros } from './_helpers';
import { RumFilterSelect, RumListPage, useRumBasePath } from './RumLayout';

const ALL = '__all__';

export function Errors() {
  const { t } = useTranslation('rum');
  const navigate = useNavigate();
  const basePath = useRumBasePath();
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const window = useTimeStore((state) => state.window);
  const [rangeRefreshAt, setRangeRefreshAt] = React.useState(() => Date.now());
  const range = React.useMemo(
    () => windowToMicros(window, new Date(rangeRefreshAt)),
    [rangeRefreshAt, window],
  );
  const [search, setSearch] = React.useState('');
  const [status, setStatus] = React.useState(ALL);
  const paginationContext = JSON.stringify({
    orgId,
    from: range.from_micros,
    to: range.to_micros,
    search: search.trim(),
    status,
  });
  const pagination = useCursorPagination({ contextKey: paginationContext });

  const query = useQuery({
    queryKey: [
      'rum',
      'errors',
      paginationContext,
      pagination.pageSize,
      pagination.cursor,
    ],
    queryFn: () =>
      rumApi.listErrors({
        org_id: orgId,
        ...range,
        limit: pagination.pageSize,
        ...(search.trim() ? { q: search.trim() } : {}),
        ...(status !== ALL ? { status: status as 'new' | 'ongoing' } : {}),
        ...(pagination.cursor ? { cursor: pagination.cursor } : {}),
      }),
    enabled: !!orgId,
  });

  const allRows = React.useMemo(() => query.data?.items ?? [], [query.data]);
  const rows = React.useMemo(() => {
    const needle = search.trim().toLowerCase();
    return allRows.filter(
      (row) =>
        (status === ALL || row.status === status) &&
        (needle.length === 0 ||
          [row.message, row.page, row.version, row.error_type, row.fingerprint]
            .filter((value): value is string => !!value)
            .some((value) => value.toLowerCase().includes(needle))),
    );
  }, [allRows, search, status]);

  const state = queryStateFor({
    isLoading: query.isLoading,
    isError: query.isError,
    data: allRows,
  });
  const pageState = productStateFor(state, {
    error: query.error,
    emptyTitle: t('errors.empty_title'),
    emptyDescription: t('errors.empty_description'),
  });
  const totalErrors = allRows.reduce((sum, row) => sum + row.count, 0);
  const uniqueUsers = new Set(allRows.flatMap((row) => row.recent_users)).size;
  const affectedSessions = new Set(allRows.flatMap((row) => row.recent_sessions)).size;
  const newIssues = allRows.filter((row) => row.status === 'new').length;

  return (
    <RumListPage
      title={t('errors.title')}
      subtitle={t('errors.subtitle') as string}
      toolbar={
        <>
          <TimeRangeChip value={formatWindowSummary(window)} />
          <ChromeButton
            onClick={() => {
              if (pagination.cursor) {
                pagination.reset();
              } else if (window.mode === 'absolute') {
                void query.refetch();
              }
              if (window.mode === 'relative') {
                setRangeRefreshAt((current) => Math.max(Date.now(), current + 1));
              }
            }}
          >
            {t('refresh')}
          </ChromeButton>
        </>
      }
      kpis={
        allRows.length > 0
          ? [
              { label: t('errors.kpi.total_errors'), value: String(totalErrors) },
              { label: t('errors.kpi.unique_users'), value: String(uniqueUsers) },
              { label: t('errors.kpi.affected_sessions'), value: String(affectedSessions) },
              {
                label: t('errors.kpi.new_issues'),
                value: String(newIssues),
                tone: newIssues > 0 ? 'danger' : 'good',
              },
            ]
          : undefined
      }
      filterBar={
        <>
          <label className="grid min-w-[280px] flex-1 gap-1">
            <span className="type-caption font-sans font-strong text-tx-3">
              {t('errors.filters.search')}
            </span>
            <span className="relative">
              <Search className="pointer-events-none absolute left-2.5 top-2 h-3.5 w-3.5 text-tx-3" />
              <input
                value={search}
                onChange={(event) => setSearch(event.target.value)}
                placeholder={t('errors.search_placeholder') ?? ''}
                className="h-8 w-full rounded-md border border-bd-1 bg-bg-1 pl-8 pr-2.5 font-sans text-xs text-tx-0 outline-none placeholder:text-tx-3 focus-visible:bg-bg-2"
              />
            </span>
          </label>
          <RumFilterSelect
            label={t('errors.filters.status')}
            value={status}
            options={[
              { value: ALL, label: t('errors.filters.all_statuses') },
              { value: 'new', label: t('errors.status.new') },
              { value: 'ongoing', label: t('errors.status.ongoing') },
            ]}
            onChange={setStatus}
          />
        </>
      }
      state={pageState}
    >
      <div className="border-b border-bd-0">
        <div className="hidden min-h-10 grid-cols-[minmax(320px,1.4fr)_minmax(180px,.7fr)_120px_160px_120px_24px] items-center gap-5 border-b border-bd-0 text-xs font-strong text-tx-3 lg:grid">
          <span>{t('errors.columns.issue')}</span>
          <span>{t('errors.columns.impact')}</span>
          <span>{t('errors.columns.trend')}</span>
          <span>{t('errors.columns.last_seen')}</span>
          <span>{t('errors.columns.status')}</span>
          <span />
        </div>
        <div className="divide-y divide-bd-0">
          {rows.map((row) => (
            <div
              key={row.fingerprint}
              className="grid gap-4 py-4 lg:grid-cols-[minmax(320px,1.4fr)_minmax(180px,.7fr)_120px_160px_120px_24px] lg:items-center lg:gap-5"
            >
              <div className="min-w-0">
                <button
                  type="button"
                  onClick={() =>
                    navigate(`${basePath}/errors/view/${encodeURIComponent(row.fingerprint)}`)
                  }
                  className="block max-w-full truncate rounded text-left text-sm font-display text-tx-0 outline-none hover:bg-bg-2 hover:text-blue-soft focus-visible:bg-bg-2 focus-visible:text-blue-soft"
                >
                  {row.message || row.fingerprint}
                </button>
                <div className="mt-1.5 flex min-w-0 flex-wrap items-center gap-2 text-xs text-tx-3">
                  <span className="truncate">{row.page ?? t('errors.unknown_page')}</span>
                  {row.error_type && (
                    <>
                      <span aria-hidden>·</span>
                      <span>{row.error_type}</span>
                    </>
                  )}
                  {row.version && (
                    <>
                      <span aria-hidden>·</span>
                      <span>{row.version}</span>
                    </>
                  )}
                </div>
                {row.recent_sessions[0] && (
                  <Link
                    to={`${basePath}/sessions/view/${encodeURIComponent(row.recent_sessions[0])}`}
                    className="mt-2 inline-flex items-center gap-1 text-xs font-strong text-blue-soft hover:text-tx-0"
                  >
                    {t('errors.view_related_replays', { count: row.sessions })}
                    <ArrowRight className="h-3 w-3" />
                  </Link>
                )}
              </div>

              <div>
                <div className="text-sm font-strong text-tx-0">
                  {t('errors.impact_summary', {
                    sessions: row.sessions,
                    users: row.users,
                  })}
                </div>
                <div className="mt-1 text-xs text-tx-3">
                  {t('errors.occurrence_count', { count: row.count })}
                </div>
              </div>

              <Trend value={row.trend_pct} />

              <div className="text-xs text-tx-2">
                <span className="block font-strong text-tx-1">
                  {formatMicros(row.last_seen_micros)}
                </span>
                <span className="mt-1 block text-tx-3">
                  {t('errors.first_seen')}: {formatMicros(row.first_seen_micros)}
                </span>
              </div>

              <span>
                <Pill tone={row.status === 'new' ? 'red' : 'yellow'}>
                  {t(`errors.status.${row.status}`)}
                </Pill>
              </span>

              <button
                type="button"
                onClick={() =>
                  navigate(`${basePath}/errors/view/${encodeURIComponent(row.fingerprint)}`)
                }
                aria-label={t('errors.open_issue', { issue: row.message })}
                className="hidden rounded text-tx-3 outline-none hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0 lg:block"
              >
                <ArrowRight className="h-4 w-4" />
              </button>
            </div>
          ))}
        </div>
        {rows.length === 0 && (
          <div className="grid min-h-52 place-items-center text-sm text-tx-3">
            {t('errors.no_filter_results')}
          </div>
        )}
      </div>
      <CursorPagination
        pageSize={pagination.pageSize}
        pageSizeOptions={[20, 50, 100]}
        hasPrevious={Boolean(query.data?.previous_cursor)}
        hasNext={Boolean(query.data?.next_cursor)}
        pending={query.isFetching}
        ariaLabel={t('pagination.aria_label')}
        pageSizeAriaLabel={t('pagination.page_size')}
        previousLabel={t('pagination.previous')}
        nextLabel={t('pagination.next')}
        onPrevious={() => pagination.goPrevious(query.data)}
        onNext={() => pagination.goNext(query.data)}
        onPageSizeChange={pagination.setPageSize}
      />
    </RumListPage>
  );
}

function Trend({ value }: { value: number }) {
  const { t } = useTranslation('rum');
  if (value === 0) {
    return <span className="text-xs font-strong text-tx-3">{t('errors.trend_flat')}</span>;
  }
  const rising = value > 0;
  return (
    <span
      className={`inline-flex items-center gap-1 text-xs font-strong ${
        rising ? 'text-red-soft' : 'text-green-soft'
      }`}
    >
      {rising ? (
        <ArrowUpRight className="h-3.5 w-3.5" />
      ) : (
        <ArrowDownRight className="h-3.5 w-3.5" />
      )}
      {Math.abs(value)}%
    </span>
  );
}
