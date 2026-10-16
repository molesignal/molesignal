import type { TFunction } from 'i18next';
import { ArrowLeft, TriangleAlert } from 'lucide-react';
import type { ReactNode } from 'react';
import { Link, useParams } from 'react-router-dom';

import type {
  PublicStatusPageIncident,
  PublicStatusPageSnapshot,
  StatusPageLanguage,
} from '@/api/statusPages';
import { cn } from '@/shell/lib/cn';

import {
  affectedComponentNames,
  formatMicros,
  impactLabel,
  incidentStatusLabel,
} from '../model';
import { lasted, listNames, microsDateTime, relativeTime } from './format';
import { type PublicStatusContext, PublicStatusFrame } from './Frame';
import { IncidentStatusChip } from './Notices';
import { IncidentPostmortem } from './Postmortem';
import {
  type PublicStatusSource,
  findPublicIncident,
  latestFirst,
} from './routing';
import { IMPACT_DOT, INCIDENT_STATUS_DOT } from './tone';

/** One incident or one piece of maintenance of a public status page, with everything said about it. */
export function PublicStatusIncidentPage({
  source = 'slug',
  unmatchedDomain,
}: {
  source?: PublicStatusSource;
  unmatchedDomain?: ReactNode;
}) {
  const { incidentId = '' } = useParams();

  return (
    <PublicStatusFrame source={source} view="incident" unmatchedDomain={unmatchedDomain}>
      {(context) => <IncidentView context={context} incidentId={incidentId} />}
    </PublicStatusFrame>
  );
}

function IncidentView({ context, incidentId }: { context: PublicStatusContext; incidentId: string }) {
  const { snapshot, page, language, copy, paths } = context;
  const incident = findPublicIncident(snapshot, incidentId);

  return (
    <>
      <Link
        to={paths.current}
        className="inline-flex min-h-[44px] items-center gap-2 rounded-md text-sm font-strong text-tx-2 transition-colors hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0 sm:min-h-0"
      >
        <ArrowLeft aria-hidden className="h-4 w-4" />
        {copy('public.incident_detail.back')}
      </Link>

      {incident ? (
        <IncidentRecord
          incident={incident}
          snapshot={snapshot}
          timezone={page.timezone}
          language={language}
          copy={copy}
        />
      ) : (
        <section className="mt-5 rounded-2xl border border-bd-0 bg-white px-6 py-12 text-center sm:px-8">
          <TriangleAlert aria-hidden className="mx-auto h-8 w-8 text-tx-3" strokeWidth={1.5} />
          <h2 className="mt-4 text-xl font-display-strong text-tx-0">
            {copy('public.incident_detail.not_found_title')}
          </h2>
          <p className="mx-auto mt-2 max-w-lg text-sm leading-6 text-tx-2">
            {copy('public.incident_detail.not_found_description')}
          </p>
        </section>
      )}
    </>
  );
}

function IncidentRecord({
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
  const updates = latestFirst(incident);
  const isMaintenance = incident.kind === 'maintenance';
  // Maintenance that is only announced has a time it is planned for, not one it started at.
  const announced = snapshot.scheduled_maintenance.some((candidate) => candidate.id === incident.id);
  const over = incident.ended_at !== null;

  return (
    <article className="mt-5 overflow-hidden rounded-2xl border border-bd-0 bg-white">
      <header className="px-5 py-7 sm:px-8 sm:py-9">
        <div className="flex flex-col items-start gap-4 sm:flex-row sm:justify-between sm:gap-6">
          <div className="min-w-0">
            <p className="flex items-center gap-2 text-sm font-strong text-tx-2">
              <span aria-hidden className={cn('h-2 w-2 shrink-0 rounded-full', IMPACT_DOT[incident.impact])} />
              {isMaintenance ? copy('sections.scheduled_maintenance') : impactLabel(copy, incident.impact)}
            </p>
            <h2 className="mt-2 text-balance text-2xl font-display-strong tracking-[-0.02em] text-tx-0 sm:text-3xl">
              {incident.title}
            </h2>
          </div>
          <IncidentStatusChip status={incident.status} kind={incident.kind} copy={copy} />
        </div>

        <dl className="mt-7 flex flex-col gap-x-12 gap-y-5 border-t border-bd-0 pt-6 sm:flex-row sm:flex-wrap">
          <Fact label={announced ? copy('public.scheduled_for') : copy('public.incident_detail.started_at')}>
            <time dateTime={microsDateTime(incident.started_at)} className="tabular-nums">
              {formatMicros(incident.started_at, timezone, language)}
            </time>
            {!over && (
              <span className="mt-0.5 block text-sm text-tx-2">
                {relativeTime(incident.started_at, snapshot.generated_at, language)}
              </span>
            )}
          </Fact>
          {incident.ended_at !== null && (
            <Fact label={incidentStatusLabel(copy, incident.status, incident.kind)}>
              <time dateTime={microsDateTime(incident.ended_at)} className="tabular-nums">
                {formatMicros(incident.ended_at, timezone, language)}
              </time>
            </Fact>
          )}
          {incident.ended_at !== null && incident.status !== 'cancelled' && (
            <Fact label={copy('public.incident_detail.duration')}>
              <span className="tabular-nums">{lasted(copy, incident.started_at, incident.ended_at)}</span>
            </Fact>
          )}
          {components.length > 0 && (
            <Fact label={copy('public.incident_detail.affected_components')}>{listNames(components, language)}</Fact>
          )}
          {/* The publisher's word on when they will say more, while there is more to say. */}
          {!over && incident.next_update_at != null && (
            <Fact label={copy('public.incident_detail.next_update')}>
              <time dateTime={microsDateTime(incident.next_update_at)} className="tabular-nums">
                {formatMicros(incident.next_update_at, timezone, language)}
              </time>
              <span className="mt-0.5 block text-sm text-tx-2">
                {relativeTime(incident.next_update_at, snapshot.generated_at, language)}
              </span>
            </Fact>
          )}
        </dl>
      </header>

      <section className="border-t border-bd-0 px-5 py-7 sm:px-8 sm:py-9" aria-labelledby="incident-updates-heading">
        <h3 id="incident-updates-heading" className="text-lg font-display-strong tracking-[-0.01em] text-tx-0 sm:text-xl">
          {copy('public.incident_detail.updates')}
        </h3>
        {updates.length === 0 ? (
          <p className="mt-4 text-sm leading-6 text-tx-2">{copy('public.incident_detail.no_updates')}</p>
        ) : (
          // The latest word first: it is what a reader who arrives in the middle came for.
          <ol className="mt-6">
            {updates.map((update, index) => {
              const last = index === updates.length - 1;

              return (
                <li key={update.id} className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-4">
                  <span aria-hidden className="flex flex-col items-center pt-[7px]">
                    <span className={cn('h-2.5 w-2.5 shrink-0 rounded-full', INCIDENT_STATUS_DOT[update.status])} />
                    {!last && <span className="mt-2 w-px flex-1 bg-bd-1" />}
                  </span>
                  <div className={cn('min-w-0', !last && 'pb-7')}>
                    <div className="flex flex-wrap items-baseline gap-x-3 gap-y-0.5">
                      <h4 className="text-base font-bold leading-6 text-tx-0">
                        {incidentStatusLabel(copy, update.status, incident.kind)}
                      </h4>
                      <time dateTime={microsDateTime(update.created_at)} className="text-sm tabular-nums text-tx-2">
                        {formatMicros(update.created_at, timezone, language)}
                      </time>
                    </div>
                    <p className="mt-1.5 max-w-3xl whitespace-pre-wrap text-pretty text-base leading-6 text-tx-1">
                      {update.message}
                    </p>
                  </div>
                </li>
              );
            })}
          </ol>
        )}
      </section>

      {incident.postmortem && (
        <IncidentPostmortem
          postmortem={incident.postmortem}
          timezone={timezone}
          language={language}
          copy={copy}
        />
      )}
    </article>
  );
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="text-xs font-strong text-tx-3">{label}</dt>
      <dd className="mt-1.5 text-base leading-6 text-tx-0">{children}</dd>
    </div>
  );
}
