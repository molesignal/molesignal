import { useQuery } from '@tanstack/react-query';
import {
  AlertTriangle,
  ChevronRight,
  CircleX,
  Gauge,
  MousePointerClick,
  PlayCircle,
  Search,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import * as rumApi from '@/api/rum';
import type { ExperienceGrade, SessionRow } from '@/api/rum';
import { useCursorPagination } from '@/pagination/useCursorPagination';
import { productStateFor } from '@/product/states';
import { ChromeButton, Pill, TimeRangeChip, type PillTone } from '@/shell/chrome';
import { CursorPagination } from '@/shell/CursorPagination';
import { queryStateFor } from '@/shell/query/State';
import { useAuthStore } from '@/stores/auth';
import { formatWindowSummary, useTimeStore } from '@/stores/useTimeStore';

import { formatDurationMs, windowToMicros } from './_helpers';
import { RumFilterSelect, RumListPage, useRumBasePath } from './RumLayout';

const ALL = '__all__';

export function Sessions({
  mode = 'sessions',
}: {
  mode?: 'sessions' | 'replay';
} = {}) {
  const { t } = useTranslation('rum');
  const navigate = useNavigate();
  const basePath = useRumBasePath();
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const window = useTimeStore((state) => state.window);
  const [search, setSearch] = React.useState('');
  const [experience, setExperience] = React.useState(ALL);
  const [issue, setIssue] = React.useState(ALL);
  const [country, setCountry] = React.useState(ALL);
  const [browser, setBrowser] = React.useState(ALL);
  const [rangeRefreshAt, setRangeRefreshAt] = React.useState(() => Date.now());
  const range = React.useMemo(
    () => windowToMicros(window, new Date(rangeRefreshAt)),
    [rangeRefreshAt, window],
  );
  const paginationContext = JSON.stringify({
    orgId,
    from: range.from_micros,
    to: range.to_micros,
    search: search.trim(),
    experience,
    issue,
    country,
    browser,
    mode,
  });
  const pagination = useCursorPagination({ contextKey: paginationContext });

  const query = useQuery({
    queryKey: [
      'rum',
      'sessions',
      paginationContext,
      pagination.pageSize,
      pagination.cursor,
    ],
    queryFn: () =>
      rumApi.listSessions({
        org_id: orgId,
        ...range,
        limit: pagination.pageSize,
        ...(search.trim() ? { q: search.trim() } : {}),
        ...(country !== ALL ? { country } : {}),
        ...(browser !== ALL ? { browser } : {}),
        ...(mode === 'replay' ? { replay_available: true } : {}),
        ...(pagination.cursor ? { cursor: pagination.cursor } : {}),
      }),
    enabled: !!orgId,
  });

  const allRows = React.useMemo(() => query.data?.items ?? [], [query.data]);
  const rows = React.useMemo(() => {
    const needle = search.trim().toLowerCase();
    return allRows.filter((session) => {
      const matchesSearch =
        needle.length === 0 ||
        [
          session.session_id,
          session.user_id,
          session.country,
          session.browser,
          session.last_page,
          session.application,
          session.version,
          ...session.journey,
        ]
          .filter((value): value is string => !!value)
          .some((value) => value.toLowerCase().includes(needle));
      return (
        matchesSearch &&
        (experience === ALL || session.experience === experience) &&
        (issue === ALL || sessionMatchesIssue(session, issue)) &&
        (country === ALL || session.country === country) &&
        (browser === ALL || session.browser === browser)
      );
    });
  }, [allRows, browser, country, experience, issue, search]);

  const state = queryStateFor({
    isLoading: query.isLoading,
    isError: query.isError,
    data: allRows,
  });
  const pageState = productStateFor(state, {
    error: query.error,
    emptyTitle: t('sessions.empty_title'),
    emptyDescription: t('sessions.empty_description'),
  });
  const abnormalCount = allRows.filter((session) => session.experience === 'poor').length;

  return (
    <RumListPage
      title={t(mode === 'replay' ? 'session_replay.title' : 'sessions.title')}
      subtitle={
        t(
          mode === 'replay' ? 'session_replay.subtitle' : 'sessions.subtitle',
        ) as string
      }
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
      filterBar={
        <>
          <label className="grid min-w-[240px] flex-1 gap-1">
            <span className="type-caption font-sans font-strong text-tx-3">
              {t('sessions.filters.search')}
            </span>
            <span className="relative">
              <Search className="pointer-events-none absolute left-2.5 top-2 h-3.5 w-3.5 text-tx-3" />
              <input
                value={search}
                onChange={(event) => setSearch(event.target.value)}
                placeholder={t('sessions.search_placeholder') ?? ''}
                className="h-8 w-full rounded-md border border-bd-1 bg-bg-1 pl-8 pr-2.5 font-sans text-xs text-tx-0 outline-none placeholder:text-tx-3 focus-visible:bg-bg-2"
              />
            </span>
          </label>
          <RumFilterSelect
            label={t('sessions.filters.experience')}
            value={experience}
            options={[
              { value: ALL, label: t('sessions.filters.all_experiences') },
              { value: 'poor', label: t('experience.poor') },
              { value: 'needs_improvement', label: t('experience.needs_improvement') },
              { value: 'good', label: t('experience.good') },
              { value: 'unknown', label: t('experience.unknown') },
            ]}
            onChange={setExperience}
          />
          <RumFilterSelect
            label={t('sessions.filters.problem')}
            value={issue}
            options={[
              { value: ALL, label: t('sessions.filters.all_problems') },
              { value: 'error', label: t('sessions.issue.error') },
              { value: 'rage_click', label: t('sessions.issue.rage_click') },
              { value: 'dead_click', label: t('sessions.issue.dead_click') },
              { value: 'slow', label: t('sessions.issue.slow') },
              { value: 'crash', label: t('sessions.issue.crash') },
            ]}
            onChange={setIssue}
          />
          <RumFilterSelect
            label={t('sessions.filters.country')}
            value={country}
            options={valueOptions(allRows, 'country', t('sessions.filters.all_countries'))}
            onChange={setCountry}
          />
          <RumFilterSelect
            label={t('sessions.filters.browser')}
            value={browser}
            options={valueOptions(allRows, 'browser', t('sessions.filters.all_browsers'))}
            onChange={setBrowser}
          />
        </>
      }
      state={pageState}
    >
      <div className="flex flex-wrap items-center gap-4 border-b border-bd-0 pb-3">
        <span className="text-sm font-strong text-tx-0">
          {t('sessions.result_count', { count: rows.length })}
        </span>
        <span className="text-xs text-tx-3">
          {t('sessions.abnormal_count', { count: abnormalCount })}
        </span>
        {(search || experience !== ALL || issue !== ALL || country !== ALL || browser !== ALL) && (
          <button
            type="button"
            onClick={() => {
              setSearch('');
              setExperience(ALL);
              setIssue(ALL);
              setCountry(ALL);
              setBrowser(ALL);
            }}
            className="ml-auto text-xs font-strong text-blue-soft hover:text-tx-0"
          >
            {t('sessions.filters.clear')}
          </button>
        )}
      </div>

      <div className="overflow-hidden border-b border-bd-0">
        <div className="hidden min-h-10 grid-cols-[minmax(300px,1.25fr)_minmax(150px,.75fr)_minmax(250px,1fr)_170px_130px_20px] items-center gap-4 border-b border-bd-0 text-xs font-strong text-tx-3 xl:grid">
          <span>{t('sessions.columns.experience')}</span>
          <span>{t('sessions.columns.user_device')}</span>
          <span>{t('sessions.columns.journey')}</span>
          <span>{t('sessions.columns.problems')}</span>
          <span>{t('sessions.columns.started')}</span>
          <span />
        </div>
        <div className="divide-y divide-bd-0">
          {rows.map((session) => (
            <SessionRowItem
              key={session.session_id}
              session={session}
              onOpen={() =>
                navigate(`${basePath}/sessions/view/${encodeURIComponent(session.session_id)}`)
              }
            />
          ))}
        </div>
        {rows.length === 0 && (
          <div className="grid min-h-52 place-items-center text-sm text-tx-3">
            {t('sessions.no_filter_results')}
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

export function SessionReplay() {
  return <Sessions mode="replay" />;
}

function SessionRowItem({
  session,
  onOpen,
}: {
  session: SessionRow;
  onOpen: () => void;
}) {
  const { t } = useTranslation('rum');
  const grade = experiencePresentation(session.experience);
  const title =
    primaryBehavior(session, t) ??
    session.last_page ??
    session.journey[0] ??
    t('sessions.unknown_page');
  return (
    <button
      type="button"
      onClick={onOpen}
      className="group grid w-full gap-4 py-4 text-left outline-none transition-colors duration-fast hover:bg-bg-2 focus-visible:bg-bg-2 xl:grid-cols-[minmax(300px,1.25fr)_minmax(150px,.75fr)_minmax(250px,1fr)_170px_130px_20px] xl:items-center"
      aria-label={t('sessions.open_replay', { session: session.session_id })}
    >
      <span className="flex min-w-0 items-center gap-3">
        <ReplayThumbnail session={session} />
        <span className="min-w-0">
          <span className="flex items-center gap-2">
            <Pill tone={grade.tone}>{t(`experience.${session.experience}`)}</Pill>
            <span className="text-xs text-tx-3">{formatDurationMs(session.duration_ms)}</span>
          </span>
          <span className="mt-1.5 block truncate text-sm font-strong text-tx-0">{title}</span>
          <span className="mt-1 block truncate font-mono text-xs text-tx-3">
            {shortSessionId(session.session_id)}
          </span>
        </span>
      </span>

      <span className="min-w-0">
        <span className="block truncate text-sm font-strong text-tx-0">
          {session.user_id ?? t('sessions.anonymous_user')}
        </span>
        <span className="mt-1 block truncate text-xs text-tx-2">
          {[session.browser, session.os ?? session.device, session.country]
            .filter(Boolean)
            .join(' · ') || '—'}
        </span>
        {(session.version || session.environment) && (
          <span className="mt-1 block truncate text-xs text-tx-3">
            {[session.version, session.environment].filter(Boolean).join(' · ')}
          </span>
        )}
      </span>

      <span className="min-w-0">
        <span className="flex min-w-0 items-center gap-1.5 overflow-hidden">
          {(session.journey.length > 0 ? session.journey : [session.last_page ?? '—'])
            .slice(0, 3)
            .map((page, index) => (
              <React.Fragment key={`${page}-${index}`}>
                {index > 0 && <ChevronRight className="h-3 w-3 shrink-0 text-tx-3" />}
                <span className="min-w-0 truncate text-xs font-strong text-tx-1">{page}</span>
              </React.Fragment>
            ))}
        </span>
        <span className="mt-1.5 block truncate text-xs text-tx-3">
          {t('sessions.last_page')}: {session.last_page ?? '—'}
        </span>
      </span>

      <span className="flex min-w-0 flex-wrap gap-1.5">
        <IssuePills session={session} />
      </span>

      <span className="text-xs text-tx-2">
        <span className="block font-strong text-tx-1">
          {relativeTime(session.started_at_micros)}
        </span>
        <span className="mt-1 block text-tx-3">
          {session.application ?? t('scope.unknown_app')}
        </span>
      </span>
      <ChevronRight className="hidden h-4 w-4 text-tx-3 transition-transform group-hover:translate-x-0.5 group-hover:text-tx-1 xl:block" />
    </button>
  );
}

function ReplayThumbnail({ session }: { session: SessionRow }) {
  return (
    <span className="relative grid h-16 w-28 shrink-0 place-items-center overflow-hidden rounded-md border border-bd-1 bg-indigo-dim text-indigo-soft">
      <PlayCircle className="h-7 w-7 transition-transform duration-fast group-hover:scale-105" />
      {(session.rage_click_count > 0 || session.dead_click_count > 0) && (
        <span className="absolute bottom-1.5 right-2">
          <span className="absolute -inset-1 animate-ping rounded-full bg-red/30 motion-reduce:animate-none" />
          <MousePointerClick className="relative h-3.5 w-3.5 text-red-soft" />
        </span>
      )}
    </span>
  );
}

function IssuePills({ session }: { session: SessionRow }) {
  const { t } = useTranslation('rum');
  const pills: React.ReactNode[] = [];
  const errorCount = (session.error_count ?? 0) + session.failed_request_count;
  if (errorCount > 0) {
    pills.push(
      <Pill key="error" tone="red">
        <AlertTriangle className="h-3 w-3" />
        {errorCount} {t('sessions.issue.error')}
      </Pill>,
    );
  }
  if (session.rage_click_count > 0) {
    pills.push(
      <Pill key="rage" tone="yellow">
        <MousePointerClick className="h-3 w-3" />
        {t('sessions.issue.rage_click')}
      </Pill>,
    );
  }
  if (session.dead_click_count > 0) {
    pills.push(
      <Pill key="dead" tone="yellow">
        <CircleX className="h-3 w-3" />
        {t('sessions.issue.dead_click')}
      </Pill>,
    );
  }
  if (session.slow_resource_count > 0) {
    pills.push(
      <Pill key="slow" tone="yellow">
        <Gauge className="h-3 w-3" />
        {t('sessions.issue.slow')}
      </Pill>,
    );
  }
  if (session.crash_count > 0) {
    pills.push(
      <Pill key="crash" tone="red">
        {t('sessions.issue.crash')}
      </Pill>,
    );
  }
  return pills.length > 0 ? pills : <span className="text-xs text-tx-3">{t('sessions.no_issues')}</span>;
}

function experiencePresentation(grade: ExperienceGrade): { tone: PillTone } {
  if (grade === 'good') return { tone: 'green' };
  if (grade === 'needs_improvement') return { tone: 'yellow' };
  if (grade === 'poor') return { tone: 'red' };
  return { tone: 'dim' };
}

function primaryBehavior(
  session: SessionRow,
  t: ReturnType<typeof useTranslation>['t'],
): string | undefined {
  if ((session.error_count ?? 0) > 0 || session.failed_request_count > 0) {
    return session.last_page
      ? t('sessions.behavior.error', { page: session.last_page })
      : undefined;
  }
  if (session.rage_click_count > 0) {
    return session.last_page
      ? t('sessions.behavior.rage_click', { page: session.last_page })
      : undefined;
  }
  if (session.dead_click_count > 0) {
    return session.last_page
      ? t('sessions.behavior.dead_click', { page: session.last_page })
      : undefined;
  }
  return undefined;
}

function sessionMatchesIssue(session: SessionRow, issue: string): boolean {
  if (issue === 'error') return (session.error_count ?? 0) > 0 || session.failed_request_count > 0;
  if (issue === 'rage_click') return session.rage_click_count > 0;
  if (issue === 'dead_click') return session.dead_click_count > 0;
  if (issue === 'slow') return session.slow_resource_count > 0;
  if (issue === 'crash') return session.crash_count > 0;
  return true;
}

function valueOptions(
  rows: SessionRow[],
  field: 'country' | 'browser',
  allLabel: string,
) {
  const values = Array.from(
    new Set(rows.map((row) => row[field]).filter((value): value is string => !!value)),
  ).sort();
  return [
    { value: ALL, label: allLabel },
    ...values.map((value) => ({ value, label: value })),
  ];
}

function shortSessionId(value: string) {
  return value.length > 22 ? `${value.slice(0, 20)}…` : value;
}

function relativeTime(micros: number | undefined): string {
  if (!micros) return '—';
  const deltaMinutes = Math.round((micros / 1_000 - Date.now()) / 60_000);
  const formatter = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' });
  if (Math.abs(deltaMinutes) < 60) return formatter.format(deltaMinutes, 'minute');
  const deltaHours = Math.round(deltaMinutes / 60);
  if (Math.abs(deltaHours) < 24) return formatter.format(deltaHours, 'hour');
  return formatter.format(Math.round(deltaHours / 24), 'day');
}
