import { useQuery } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { ArrowLeft, CheckCircle2, Clock3, TriangleAlert } from 'lucide-react';
import type { ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useParams, useSearchParams } from 'react-router-dom';

import type {
  PublicIncidentStatus,
  PublicStatusPageIncident,
  PublicStatusPageIncidentUpdate,
  PublicStatusPageSnapshot,
  StatusPageLanguage,
} from '@/api/statusPages';
import { toApiError } from '@/lib/http';
import { cn } from '@/shell/lib/cn';

import {
  affectedComponentNames,
  formatMicros,
  impactLabel,
  incidentStatusLabel,
  resolveStatusPageLanguage,
  statusPageLanguages,
} from './model';
import { PublicStatusAccess } from './PublicStatusAccess';
import { PublicStatusLayout } from './PublicStatusLayout';
import {
  type PublicStatusSource,
  findPublicIncident,
  publicStatusPageQuery,
  publicStatusPaths,
} from './publicStatusRouting';

const INCIDENT_STATUS_STYLE: Record<PublicIncidentStatus, string> = {
  investigating: 'bg-red-dim text-red-soft',
  identified: 'bg-orange-dim text-orange-soft',
  in_progress: 'bg-yellow-dim text-yellow-soft',
  monitoring: 'bg-blue-dim text-blue-soft',
  resolved: 'bg-green-dim text-green-soft',
  scheduled: 'bg-blue-dim text-blue-soft',
  completed: 'bg-green-dim text-green-soft',
  cancelled: 'bg-bg-2 text-tx-3',
};

export function PublicStatusIncidentPage({
  source = 'slug',
  unmatchedDomain,
}: {
  source?: PublicStatusSource;
  unmatchedDomain?: ReactNode;
}) {
  const { slug = '', incidentId = '' } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const { t, i18n } = useTranslation('status-pages');
  const query = useQuery(publicStatusPageQuery(source, slug));

  if (query.isLoading) {
    return <IncidentLoadState>{t('states.loading')}</IncidentLoadState>;
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
      <IncidentLoadState title={t('public.not_found_title')}>
        {error.status === 404
          ? t('public.not_found_description')
          : t('public.load_error')}
      </IncidentLoadState>
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
  const paths = publicStatusPaths(source, slug, language);
  const homeHref = paths.current;
  const incident = findPublicIncident(snapshot, incidentId);

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
      homeHref={homeHref}
      poweredBy={copy('public.powered_by')}
      footerNavigationLabel={copy('public.archive.navigation_label')}
      footerLinks={[
        { href: paths.current, label: copy('public.archive.current_status') },
        { href: paths.history, label: copy('public.archive.incident_history') },
        { href: paths.uptime, label: copy('public.archive.uptime') },
      ]}
    >
      <Link
        to={homeHref}
        className="inline-flex items-center gap-2 rounded-md px-1 py-1 text-sm font-strong text-tx-2 transition-colors hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0"
      >
        <ArrowLeft aria-hidden className="h-4 w-4" />
        {copy('public.incident_detail.back')}
      </Link>

      {incident ? (
        <IncidentDetail
          incident={incident}
          snapshot={snapshot}
          timezone={page.timezone}
          language={language}
          copy={copy}
        />
      ) : (
        <section className="mt-7 rounded-2xl border border-bd-0 bg-white px-6 py-12 text-center sm:px-8">
          <TriangleAlert
            aria-hidden
            className="mx-auto h-8 w-8 text-tx-3"
            strokeWidth={1.5}
          />
          <h2 className="mt-4 text-xl font-display-strong text-tx-0">
            {copy('public.incident_detail.not_found_title')}
          </h2>
          <p className="mx-auto mt-2 max-w-lg text-sm leading-6 text-tx-2">
            {copy('public.incident_detail.not_found_description')}
          </p>
        </section>
      )}
    </PublicStatusLayout>
  );
}

function IncidentLoadState({
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

function IncidentDetail({
  incident,
  snapshot,
  timezone,
  language,
  copy,
}: {
  incident: PublicStatusPageIncident;
  snapshot: PublicStatusPageSnapshot;
  timezone: string;
  language: StatusPageLanguage;
  copy: TFunction;
}) {
  const components = affectedComponentNames(incident, snapshot.components);
  const updates = [...incident.updates].sort(
    (left, right) => left.created_at - right.created_at,
  );
  const scope = updates[0]?.message ?? impactLabel(copy, incident.impact);

  return (
    <article className="mt-7 overflow-hidden rounded-2xl border border-bd-0 bg-white">
      <header className="px-5 py-7 sm:px-8 sm:py-9">
        <div className="flex flex-col items-start gap-4 sm:flex-row sm:justify-between">
          <div className="min-w-0">
            <p className="text-sm font-strong text-tx-3">
              {incident.kind === 'maintenance'
                ? copy('sections.scheduled_maintenance')
                : copy('public.incident_detail.incident')}
            </p>
            <h2 className="mt-2 text-2xl font-display-strong tracking-[-0.02em] text-tx-0 sm:text-3xl">
              {incident.title}
            </h2>
          </div>
          <span
            className={cn(
              'inline-flex shrink-0 rounded-full px-3 py-1.5 text-sm font-bold',
              INCIDENT_STATUS_STYLE[incident.status],
            )}
          >
            {incidentStatusLabel(copy, incident.status)}
          </span>
        </div>

        <dl className="mt-8 grid gap-6 border-t border-bd-0 pt-7 sm:grid-cols-2 lg:grid-cols-3">
          <DetailField label={copy('public.incident_detail.status')}>
            {incidentStatusLabel(copy, incident.status)}
          </DetailField>
          <DetailField label={copy('public.incident_detail.started_at')}>
            <time dateTime={microsDateTime(incident.started_at)}>
              {formatMicros(incident.started_at, timezone, language)}
            </time>
          </DetailField>
          {incident.ended_at && (
            <DetailField label={copy('public.incident_detail.resolved_at')}>
              <time dateTime={microsDateTime(incident.ended_at)}>
                {formatMicros(incident.ended_at, timezone, language)}
              </time>
            </DetailField>
          )}
          <DetailField label={copy('public.incident_detail.affected_components')}>
            {components.length > 0
              ? components.join(' · ')
              : copy('public.incident_detail.no_affected_components')}
          </DetailField>
          <DetailField label={copy('public.incident_detail.impact_scope')} wide>
            {scope}
          </DetailField>
        </dl>
      </header>

      <section className="border-t border-bd-0 bg-bg-1 px-5 py-7 sm:px-8 sm:py-9">
        <h3 className="text-xl font-display-strong text-tx-0">
          {copy('public.incident_detail.timeline')}
        </h3>
        {updates.length === 0 ? (
          <p className="mt-5 text-sm text-tx-2">
            {copy('public.incident_detail.no_updates')}
          </p>
        ) : (
          <IncidentTimeline
            updates={updates}
            timezone={timezone}
            language={language}
            copy={copy}
          />
        )}
      </section>
    </article>
  );
}

function DetailField({
  label,
  wide = false,
  children,
}: {
  label: string;
  wide?: boolean;
  children: ReactNode;
}) {
  return (
    <div className={cn(wide && 'sm:col-span-2 lg:col-span-3')}>
      <dt className="text-xs font-strong uppercase tracking-[0.1em] text-tx-3">
        {label}
      </dt>
      <dd className="mt-2 text-sm leading-6 text-tx-1">{children}</dd>
    </div>
  );
}

function IncidentTimeline({
  updates,
  timezone,
  language,
  copy,
}: {
  updates: PublicStatusPageIncidentUpdate[];
  timezone: string;
  language: StatusPageLanguage;
  copy: TFunction;
}) {
  return (
    <ol className="mt-6 ml-2 border-l border-bd-1">
      {updates.map((update, index) => (
        <li
          key={update.id}
          className={cn('relative pl-7', index < updates.length - 1 && 'pb-8')}
        >
          <span
            aria-hidden
            className={cn(
              'absolute -left-[5px] top-1 h-2.5 w-2.5 rounded-full border-2 border-bg-1',
              update.status === 'resolved' ? 'bg-green' : 'bg-blue',
            )}
          />
          <div className="flex flex-col gap-1 sm:flex-row sm:items-center sm:justify-between sm:gap-4">
            <h4 className="text-sm font-bold text-tx-0">
              {incidentStatusLabel(copy, update.status)}
            </h4>
            <time
              dateTime={microsDateTime(update.created_at)}
              className="inline-flex shrink-0 items-center gap-1.5 text-xs tabular-nums text-tx-3"
            >
              <Clock3 aria-hidden className="h-3.5 w-3.5" />
              {formatMicros(update.created_at, timezone, language)}
            </time>
          </div>
          <p className="mt-2 whitespace-pre-wrap text-sm leading-6 text-tx-2">
            {update.message}
          </p>
          {update.status === 'resolved' && (
            <CheckCircle2
              aria-hidden
              className="mt-3 h-4 w-4 text-green-soft"
              strokeWidth={1.8}
            />
          )}
        </li>
      ))}
    </ol>
  );
}

function microsDateTime(micros: number): string {
  return new Date(Math.floor(micros / 1_000)).toISOString();
}
