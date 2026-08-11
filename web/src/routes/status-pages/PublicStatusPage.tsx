import { useQuery } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { CheckCircle2, TriangleAlert } from 'lucide-react';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useParams, useSearchParams } from 'react-router-dom';

import type {
  ComponentStatus,
  StatusPageLanguage,
} from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { cn } from '@/shell/lib/cn';

import {
  formatMicros,
  incidentStatusLabel,
  resolveStatusPageLanguage,
  statusPageHistoryDays,
  statusPageLanguages,
} from './model';
import { PublicComponentStatusList } from './PublicComponentStatusList';
import { PublicStatusAccess } from './PublicStatusAccess';
import { PublicStatusLayout } from './PublicStatusLayout';
import {
  type PublicStatusSource,
  type RecentPublicIncident,
  publicIncidentPath,
  publicStatusPageQuery,
  publicStatusPaths,
  recentPublicIncidentTime,
  recentPublicIncidents,
} from './publicStatusRouting';

const OVERALL_PRESENTATION: Record<
  ComponentStatus,
  { surface: string; accent: string }
> = {
  operational: {
    surface: 'border-green/30 bg-white',
    accent: 'bg-green/20 text-green',
  },
  degraded_performance: {
    surface: 'border-yellow/35 bg-yellow-dim/35',
    accent: 'bg-yellow/20 text-yellow-soft',
  },
  partial_outage: {
    surface: 'border-orange/35 bg-orange-dim/40',
    accent: 'bg-orange/20 text-orange',
  },
  major_outage: {
    surface: 'border-red/35 bg-red-dim/40',
    accent: 'bg-red/20 text-red',
  },
  maintenance: {
    surface: 'border-blue/30 bg-blue-dim/35',
    accent: 'bg-blue/20 text-blue',
  },
};

export function PublicStatusPage({
  source = 'slug',
  unmatchedDomain,
}: {
  source?: PublicStatusSource;
  unmatchedDomain?: ReactNode;
}) {
  const { slug = '' } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const { t, i18n } = useTranslation('status-pages');
  const query = useQuery(publicStatusPageQuery(source, slug));

  if (query.isLoading) {
    return <PublicStatusState>{t('states.loading')}</PublicStatusState>;
  }
  if (source === 'domain' && query.data === null) {
    return <>{unmatchedDomain}</>;
  }
  if (!query.data) {
    const error = toApiError(query.error);
    if (error.status === 401) {
      return <PublicStatusAccess source={source} slug={slug} />;
    }
    return (
      <PublicStatusState title={t('public.not_found_title')}>
        {error.status === 404
          ? t('public.not_found_description')
          : t('public.load_error')}
      </PublicStatusState>
    );
  }

  const snapshot = query.data;
  const { page } = snapshot;
  const languages = statusPageLanguages(page.language, page.languages);
  const language = resolveStatusPageLanguage(
    page.language,
    page.languages,
    searchParams.get('lang'),
  );
  const copy = i18n.getFixedT(language, 'status-pages');
  const languageQuery = `?lang=${encodeURIComponent(language)}`;
  const paths = publicStatusPaths(source, slug, language);
  const historyDays = statusPageHistoryDays(page);

  return (
    <PublicStatusLayout
      page={page}
      language={language}
      languages={languages}
      languageLabel={copy('public.language_switcher')}
      statusPageLabel={copy('public.status_page_label')}
      onLanguageChange={(nextLanguage) => {
        const next = new URLSearchParams(searchParams);
        next.set('lang', nextLanguage);
        setSearchParams(next, { replace: true });
      }}
      homeHref={paths.current}
      poweredBy={copy('public.powered_by')}
      footerNavigationLabel={copy('public.archive.navigation_label')}
      footerLinks={[
        {
          href: paths.current,
          label: copy('public.archive.current_status'),
          current: true,
        },
        { href: paths.history, label: copy('public.archive.incident_history') },
        { href: paths.uptime, label: copy('public.archive.uptime') },
      ]}
    >
      <OverallStatus
        status={snapshot.overall_status}
        pageName={page.name}
        updatedAt={snapshot.updated_at ?? snapshot.generated_at}
        timezone={page.timezone}
        language={language}
        copy={copy}
      />

      <section className="mt-12 sm:mt-16" aria-labelledby="system-status-heading">
        <SectionHeader
          id="system-status-heading"
          title={copy('public.system_status')}
          meta={copy('public.uptime.period')}
        />
        <PublicComponentStatusList
          snapshot={snapshot}
          timezone={page.timezone}
          language={language}
          empty={copy('public.no_components')}
          copy={copy}
        />
      </section>

      <RecentIncidents
        incidents={recentPublicIncidents(snapshot)}
        source={source}
        slug={slug}
        timezone={page.timezone}
        language={language}
        historyDays={historyDays}
        languageQuery={languageQuery}
        copy={copy}
      />
    </PublicStatusLayout>
  );
}

function PublicStatusState({
  title,
  children,
}: {
  title?: string;
  children: ReactNode;
}) {
  return (
    <main
      data-theme="light"
      className="grid h-full min-h-screen place-items-center overflow-y-auto bg-bg-0 px-6 text-center"
    >
      <div className="max-w-md">
        {title && (
          <>
            <TriangleAlert
              aria-hidden
              className="mx-auto h-9 w-9 text-tx-3"
              strokeWidth={1.5}
            />
            <h1 className="mt-5 text-2xl font-display-strong text-tx-0">{title}</h1>
          </>
        )}
        <p className={cn('text-sm leading-relaxed text-tx-2', title && 'mt-2')}>
          {children}
        </p>
      </div>
    </main>
  );
}

function OverallStatus({
  status,
  pageName,
  updatedAt,
  timezone,
  language,
  copy,
}: {
  status: ComponentStatus;
  pageName: string;
  updatedAt: number;
  timezone: string;
  language: StatusPageLanguage;
  copy: TFunction;
}) {
  const presentation = OVERALL_PRESENTATION[status];
  const StatusIcon = status === 'operational' ? CheckCircle2 : TriangleAlert;

  return (
    <section
      aria-live="polite"
      className={cn(
        'grid gap-5 rounded-xl border px-5 py-5 sm:px-6 md:grid-cols-[minmax(0,1fr)_auto] md:items-center md:gap-8',
        presentation.surface,
      )}
    >
      <div className="flex items-start gap-3.5 sm:items-center">
        <span
          className={cn(
            'grid h-12 w-12 shrink-0 place-items-center rounded-xl',
            presentation.accent,
          )}
        >
          <StatusIcon aria-hidden className="h-7 w-7" strokeWidth={2.4} />
        </span>
        <div className="min-w-0">
          <h2 className="text-lg font-display-strong tracking-[-0.015em] text-tx-0 sm:text-xl">
            {copy(`public.overall.${status}`)}
          </h2>
          <p className="mt-1 max-w-2xl text-sm leading-5 text-tx-2">
            {copy(`public.overall_description.${status}`, { name: pageName })}
          </p>
        </div>
      </div>
      <div className="border-t border-bd-0 pt-4 md:border-l md:border-t-0 md:pl-7 md:pt-0">
        <div className="text-xs font-strong uppercase tracking-[0.1em] text-tx-3">
          {copy('public.last_updated_label')}
        </div>
        <time
          dateTime={microsDateTime(updatedAt)}
          className="mt-2 block whitespace-nowrap text-sm font-strong tabular-nums text-tx-1"
        >
          {formatMicros(updatedAt, timezone, language)}
        </time>
      </div>
    </section>
  );
}

function SectionHeader({
  id,
  title,
  meta,
}: {
  id: string;
  title: string;
  meta?: string;
}) {
  return (
    <div className="flex items-end justify-between gap-4">
      <h2
        id={id}
        className="text-xl font-display-strong tracking-[-0.015em] text-tx-0 sm:text-2xl"
      >
        {title}
      </h2>
      {meta && <span className="shrink-0 text-sm text-tx-3">{meta}</span>}
    </div>
  );
}

function RecentIncidents({
  incidents,
  source,
  slug,
  timezone,
  language,
  historyDays,
  languageQuery,
  copy,
}: {
  incidents: RecentPublicIncident[];
  source: PublicStatusSource;
  slug: string;
  timezone: string;
  language: StatusPageLanguage;
  historyDays: number;
  languageQuery: string;
  copy: TFunction;
}) {
  const groups = groupRecentIncidents(incidents, timezone, language);

  return (
    <section className="mt-12 sm:mt-16" aria-labelledby="recent-incidents-heading">
      <SectionHeader
        id="recent-incidents-heading"
        title={copy('public.recent_incidents')}
        meta={copy('public.recent_incidents_period', { count: historyDays })}
      />
      {incidents.length === 0 ? (
        <div className="mt-5 border-t border-bd-0 py-10 text-center">
          <CheckCircle2
            aria-hidden
            className="mx-auto h-7 w-7 text-green-soft"
            strokeWidth={1.7}
          />
          <p className="mt-3 text-sm text-tx-2">
            {copy('public.no_recent_incidents', { count: historyDays })}
          </p>
        </div>
      ) : (
        <div className="mt-6 space-y-9 sm:mt-7 sm:space-y-11">
          {groups.map((group) => (
            <section
              key={group.key}
              data-testid="incident-date-group"
              aria-labelledby={`incident-date-${group.key}`}
            >
              <div className="flex items-center gap-4 border-b border-bd-0 pb-2.5">
                <h3
                  id={`incident-date-${group.key}`}
                  className="shrink-0 text-base font-display-strong tracking-[-0.01em] text-tx-0 sm:text-lg"
                >
                  {group.label}
                </h3>
              </div>
              <div className="divide-y divide-bd-0">
                {group.incidents.map((row) => (
                  <RecentIncidentRow
                    key={row.incident.id}
                    row={row}
                    href={`${publicIncidentPath(source, slug, row.incident.id)}${languageQuery}`}
                    timezone={timezone}
                    language={language}
                    copy={copy}
                  />
                ))}
              </div>
            </section>
          ))}
        </div>
      )}
    </section>
  );
}

function RecentIncidentRow({
  row,
  href,
  timezone,
  language,
  copy,
}: {
  row: RecentPublicIncident;
  href: string;
  timezone: string;
  language: StatusPageLanguage;
  copy: TFunction;
}) {
  const { incident, kind } = row;
  const shownAt = recentPublicIncidentTime(row);
  const label = kind === 'resolved'
    ? copy('public.completed_at')
    : kind === 'scheduled'
      ? copy('public.scheduled_for')
      : copy('public.started_at');

  return (
    <Link
      to={href}
      className={cn(
        'group block min-h-11 py-4 transition-colors hover:text-tx-0 focus-visible:bg-bg-1 sm:py-5',
      )}
    >
      <div className="flex min-w-0 items-start gap-3 sm:gap-4">
        <span
          aria-hidden
          className={cn(
            'mt-2 h-2 w-2 shrink-0 rounded-full',
            kind === 'active' && 'bg-orange',
            kind === 'scheduled' && 'bg-blue',
            kind === 'resolved' && 'bg-green',
          )}
        />
        <div className="min-w-0">
          <h4
            className={cn(
              'text-base font-bold leading-6 transition-colors',
              kind === 'active' && 'text-orange-soft',
              kind === 'scheduled' && 'text-blue-soft',
              kind === 'resolved' && 'text-tx-0 group-hover:text-[var(--status-brand)]',
            )}
          >
            {incident.title}
          </h4>
          <div className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-sm leading-5 text-tx-2">
            <span
              className={cn(
                'font-strong',
                kind === 'active' && 'text-orange-soft',
                kind === 'scheduled' && 'text-blue-soft',
                kind === 'resolved' && 'text-green-soft',
              )}
            >
              {incidentStatusLabel(copy, incident.status)}
            </span>
            <span aria-hidden className="text-tx-4">·</span>
            <span>{label}</span>
            <time dateTime={microsDateTime(shownAt)} className="tabular-nums">
              {formatMicros(shownAt, timezone, language, false)}
            </time>
          </div>
        </div>
      </div>
    </Link>
  );
}

interface RecentIncidentGroup {
  key: string;
  label: string;
  incidents: RecentPublicIncident[];
}

function groupRecentIncidents(
  incidents: RecentPublicIncident[],
  timezone: string,
  language: StatusPageLanguage,
): RecentIncidentGroup[] {
  const keyFormatter = new Intl.DateTimeFormat('en-US', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    timeZone: timezone,
  });
  const labelFormatter = new Intl.DateTimeFormat(language, {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
    timeZone: timezone,
  });
  const groups = new Map<string, RecentIncidentGroup>();

  incidents.forEach((incident) => {
    const date = new Date(Math.floor(recentPublicIncidentTime(incident) / 1_000));
    const parts = Object.fromEntries(
      keyFormatter.formatToParts(date).map(({ type, value }) => [type, value]),
    );
    const key = `${parts.year}-${parts.month}-${parts.day}`;
    const group = groups.get(key) ?? {
      key,
      label: labelFormatter.format(date),
      incidents: [],
    };
    group.incidents.push(incident);
    groups.set(key, group);
  });

  return [...groups.values()];
}

function microsDateTime(micros: number): string {
  return new Date(Math.floor(micros / 1_000)).toISOString();
}
