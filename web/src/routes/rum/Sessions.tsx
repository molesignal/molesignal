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
import { cn } from '@/shell/lib/cn';
import { queryStateFor } from '@/shell/query/State';
import { useAuthStore } from '@/stores/auth';
import { useTimeStore } from '@/stores/useTimeStore';

import { formatDurationMs, windowToMicros } from './_helpers';
import {
  RumFilterSelect,
  RumListPage,
  RumSurface,
  useRumBasePath,
} from './RumLayout';

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
      toolbar={
        <>
          <TimeRangeChip className="border-0" />
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
          <span className="relative min-w-[240px] flex-1">
            <Search className="pointer-events-none absolute left-2.5 top-2 h-3.5 w-3.5 text-tx-3" />
            <input
              type="search"
              value={search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder={t('sessions.search_placeholder') ?? ''}
              aria-label={t('sessions.filters.search')}
              className="h-8 w-full rounded-md border-0 bg-[var(--control-surface)] pl-8 pr-2.5 font-sans text-xs text-tx-0 outline-none placeholder:text-tx-3 hover:bg-bg-2 focus-visible:bg-bg-2"
              data-ui="search-control"
            />
          </span>
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
      <RumSurface className="overflow-hidden p-4">
        <div className="flex flex-wrap items-center gap-4">
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
            className="ml-auto text-xs font-strong text-[var(--blue-ink)] hover:text-tx-0"
          >
            {t('sessions.filters.clear')}
          </button>
        )}
      </div>

        <div className="overflow-hidden">
          <div className={cn('hidden min-h-8 items-center gap-4 border-b border-bd-0 text-xs font-strong text-tx-3 xl:grid', SESSION_COLUMNS)}>
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
        className="border-t-0 bg-transparent"
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
      </RumSurface>
    </RumListPage>
  );
}

export function SessionReplay() {
  return <Sessions mode="replay" />;
}

/** The columns of the list, shared by its head and its rows. */
const SESSION_COLUMNS =
  'xl:grid-cols-[minmax(260px,1.25fr)_minmax(180px,.9fr)_minmax(220px,1fr)_170px_130px_20px]';

/**
 * One session, in two lines: how it went and what it was, who it was, where
 * they went, what went wrong, and when. A row is short enough that a screen
 * holds a dozen of them.
 */
function SessionRowItem({
  session,
  onOpen,
}: {
  session: SessionRow;
  onOpen: () => void;
}) {
  const { t, i18n } = useTranslation('rum');
  const grade = experiencePresentation(session.experience);
  const title =
    primaryBehavior(session, t) ??
    session.last_page ??
    session.journey[0] ??
    t('sessions.unknown_page');
  const journey = (session.journey.length > 0 ? session.journey : [session.last_page ?? '—']).slice(0, 3);
  // Where the visit ended is said under its path only where the path shown does not end there.
  const endsElsewhere = Boolean(session.last_page) && journey.at(-1) !== session.last_page;

  return (
    <button
      type="button"
      data-rum-session={session.session_id}
      onClick={onOpen}
      className={cn(
        'group grid w-full gap-x-4 gap-y-1 py-[10px] text-left outline-none transition-colors duration-fast hover:bg-bg-2 focus-visible:bg-bg-2 xl:items-center',
        SESSION_COLUMNS,
      )}
      aria-label={t('sessions.open_replay', { session: session.session_id })}
    >
      <span className="min-w-0">
        <span className="flex min-w-0 items-center gap-2">
          <Pill tone={grade.tone}>{t(`experience.${session.experience}`)}</Pill>
          <span className="min-w-0 truncate text-sm font-strong text-tx-0">{title}</span>
        </span>
        <span className="mt-1 flex min-w-0 items-center gap-1.5 text-xs text-tx-3">
          {session.replay_available && (
            // A replay was recorded: said by a sign, not by a picture that every row would share.
            <PlayCircle
              data-rum-session-replay
              aria-label={t('sessions.has_replay')}
              className="h-3.5 w-3.5 shrink-0 text-indigo-soft"
            />
          )}
          <span className="min-w-0 truncate">{shortSessionId(session.session_id)}</span>
          <span aria-hidden="true">·</span>
          <span className="shrink-0 tabular-nums">{formatDurationMs(session.duration_ms)}</span>
        </span>
      </span>

      <span className="min-w-0">
        <span className="block truncate text-sm font-strong text-tx-0">
          {session.user_id ?? t('sessions.anonymous_user')}
        </span>
        <span className="mt-1 block truncate text-xs text-tx-2">
          {[session.browser, session.os ?? session.device, session.country, session.version, session.environment]
            .filter(Boolean)
            .join(' · ') || '—'}
        </span>
      </span>

      <span className="min-w-0">
        <span className="flex min-w-0 items-center gap-1.5 overflow-hidden">
          {journey.map((page, index) => (
            <React.Fragment key={`${page}-${index}`}>
              {index > 0 && <ChevronRight className="h-3 w-3 shrink-0 text-tx-3" />}
              <span className="min-w-0 truncate text-xs font-strong text-tx-1">{page}</span>
            </React.Fragment>
          ))}
        </span>
        {endsElsewhere && (
          <span className="mt-1 block truncate text-xs text-tx-3">
            {t('sessions.last_page')}: {session.last_page}
          </span>
        )}
      </span>

      <span className="flex min-w-0 flex-wrap gap-1.5">
        <IssuePills session={session} />
      </span>

      <span className="text-xs text-tx-2">
        <span className="block font-strong text-tx-1">
          {relativeTime(session.started_at_micros, i18n.resolvedLanguage ?? i18n.language)}
        </span>
        <span className="mt-1 block truncate text-tx-3">
          {session.application ?? t('scope.unknown_app')}
        </span>
      </span>
      <ChevronRight className="hidden h-4 w-4 text-tx-3 transition-transform group-hover:translate-x-0.5 group-hover:text-tx-1 xl:block" />
    </button>
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

/**
 * How long ago a session started, in the language of the product: the browser's
 * own may be another, and "47 minutes ago" then stood in a page written in Chinese.
 */
export function relativeTime(
  micros: number | undefined,
  language: string | undefined,
  now: number = Date.now(),
): string {
  if (!micros) return '—';
  const deltaMinutes = Math.round((micros / 1_000 - now) / 60_000);
  const formatter = new Intl.RelativeTimeFormat(language, { numeric: 'auto' });
  if (Math.abs(deltaMinutes) < 60) return formatter.format(deltaMinutes, 'minute');
  const deltaHours = Math.round(deltaMinutes / 60);
  if (Math.abs(deltaHours) < 24) return formatter.format(deltaHours, 'hour');
  return formatter.format(Math.round(deltaHours / 24), 'day');
}
