import type { TFunction } from 'i18next';
import { ArrowRight } from 'lucide-react';
import { Link } from 'react-router-dom';

import type {
  PublicIncidentStatus,
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
import { listNames, microsDateTime, relativeTime, zonedDayOfMonth } from './format';
import { MetaLine } from './MetaLine';
import { latestFirst } from './routing';
import { SectionHeading } from './Section';
import { IMPACT_DOT, INCIDENT_STATUS_DOT } from './tone';

/** Where an incident stands, as a word with its dot. The word is never in a colour of its own. */
export function IncidentStatusChip({
  status,
  kind,
  copy,
}: {
  status: PublicIncidentStatus;
  kind: PublicStatusPageIncident['kind'];
  copy: TFunction;
}) {
  return (
    <span className="inline-flex shrink-0 items-center gap-2 rounded-full bg-bg-2 px-3 py-1 text-sm font-strong text-tx-1">
      <span aria-hidden className={cn('h-2 w-2 rounded-full', INCIDENT_STATUS_DOT[status])} />
      {incidentStatusLabel(copy, status, kind)}
    </span>
  );
}

/**
 * Something that is affecting the service now: an incident, or maintenance
 * that has begun. It stands right under the page's verdict, with the one thing
 * a reader arriving in the middle of it wants: the latest word on it.
 */
export function ActiveNotice({
  incident,
  snapshot,
  href,
  timezone,
  language,
  copy,
}: {
  incident: PublicStatusPageIncident;
  snapshot: PublicStatusPageSnapshot;
  href: string;
  timezone: string;
  language: StatusPageLanguage;
  copy: TFunction;
}) {
  const components = affectedComponentNames(incident, snapshot.components);
  const updates = latestFirst(incident);
  const latest = updates[0];

  return (
    <article data-status-notice={incident.kind} className="border-t border-bd-0 px-5 py-6 sm:px-7">
      <div className="flex flex-col items-start gap-3 sm:flex-row sm:justify-between sm:gap-6">
        <div className="min-w-0">
          <p className="flex items-center gap-2 text-xs font-strong text-tx-3">
            <span aria-hidden className={cn('h-2 w-2 shrink-0 rounded-full', IMPACT_DOT[incident.impact])} />
            {incident.kind === 'maintenance'
              ? copy('public.notice.maintenance_under_way')
              : impactLabel(copy, incident.impact)}
          </p>
          <h3 className="mt-1.5 text-lg font-display-strong tracking-[-0.01em] text-tx-0 sm:text-xl">
            <Link to={href} className="rounded-sm underline-offset-4 hover:underline focus-visible:bg-bg-2">
              {incident.title}
            </Link>
          </h3>
          <MetaLine className="mt-1.5 text-sm leading-5 text-tx-2">
            {components.length > 0 && <>{listNames(components, language)}</>}
            <>
              {copy('public.started_at')}{' '}
              <time dateTime={microsDateTime(incident.started_at)} className="tabular-nums">
                {formatMicros(incident.started_at, timezone, language)}
              </time>
            </>
            <>{relativeTime(incident.started_at, snapshot.generated_at, language)}</>
          </MetaLine>
        </div>
        <IncidentStatusChip status={incident.status} kind={incident.kind} copy={copy} />
      </div>

      {latest && (
        <div data-status-latest-update className="mt-4 border-l-2 border-bd-1 pl-4">
          <MetaLine className="text-xs leading-5 text-tx-2">
            <span className="font-strong text-tx-1">{copy('public.notice.latest_update')}</span>
            <time dateTime={microsDateTime(latest.created_at)} className="tabular-nums">
              {formatMicros(latest.created_at, timezone, language)}
            </time>
          </MetaLine>
          <p className="mt-1.5 max-w-3xl whitespace-pre-wrap text-pretty text-sm leading-6 text-tx-1">
            {latest.message}
          </p>
          {incident.next_update_at != null && (
            <p data-status-next-update className="mt-1.5 text-xs leading-5 text-tx-2">
              {copy('public.notice.next_update', {
                time: formatMicros(incident.next_update_at, timezone, language),
              })}
            </p>
          )}
        </div>
      )}

      <Link
        to={href}
        className="group mt-4 inline-flex min-h-[44px] items-center gap-1.5 rounded-sm text-sm font-strong text-tx-0 focus-visible:bg-bg-2 sm:min-h-0"
      >
        <span className="underline-offset-4 group-hover:underline">
          {updates.length > 1
            ? copy('public.notice.all_updates', { count: updates.length })
            : copy('public.view_details')}
        </span>
        <ArrowRight aria-hidden className="h-3.5 w-3.5 transition-transform group-hover:translate-x-0.5" />
      </Link>
    </article>
  );
}

/**
 * Maintenance that is announced and has not begun. It affects nothing yet, so
 * it comes after what is going on: a short calendar of what to expect.
 */
export function UpcomingMaintenance({
  items,
  snapshot,
  hrefOf,
  timezone,
  language,
  copy,
}: {
  items: PublicStatusPageIncident[];
  snapshot: PublicStatusPageSnapshot;
  hrefOf: (incident: PublicStatusPageIncident) => string;
  timezone: string;
  language: StatusPageLanguage;
  copy: TFunction;
}) {
  if (items.length === 0) return null;

  const month = new Intl.DateTimeFormat(language, { month: 'short', timeZone: timezone });

  return (
    <section className="mt-12 sm:mt-14" aria-labelledby="upcoming-maintenance-heading">
      <SectionHeading id="upcoming-maintenance-heading" title={copy('sections.scheduled_maintenance')} />
      <ul className="mt-5 divide-y divide-bd-0 overflow-hidden rounded-xl border border-bd-0 bg-white">
        {items.map((incident) => {
          const components = affectedComponentNames(incident, snapshot.components);
          const date = new Date(Math.floor(incident.started_at / 1_000));
          const announcement = latestFirst(incident)[0]?.message;

          return (
            <li key={incident.id}>
              <Link
                to={hrefOf(incident)}
                className="group flex items-start gap-4 px-5 py-5 transition-colors hover:bg-bg-1 focus-visible:bg-bg-1 sm:gap-5 sm:px-6"
              >
                {/* The day it is on, as a calendar leaf: the first thing to read of something planned. */}
                <span
                  aria-hidden
                  className="grid w-14 shrink-0 justify-items-center rounded-lg bg-blue/[0.08] px-2 py-2 text-blue"
                >
                  <span className="text-xs font-strong uppercase">{month.format(date)}</span>
                  <span className="text-xl font-display-strong leading-6 tabular-nums">
                    {zonedDayOfMonth(incident.started_at, timezone)}
                  </span>
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block text-base font-bold text-tx-0 underline-offset-4 group-hover:underline">
                    {incident.title}
                  </span>
                  <MetaLine className="mt-1 text-sm leading-5 text-tx-2">
                    <>
                      {copy('public.scheduled_for')}{' '}
                      <time dateTime={microsDateTime(incident.started_at)} className="tabular-nums">
                        {formatMicros(incident.started_at, timezone, language)}
                      </time>
                    </>
                    <>{relativeTime(incident.started_at, snapshot.generated_at, language)}</>
                    {components.length > 0 && <>{listNames(components, language)}</>}
                  </MetaLine>
                  {announcement && (
                    <span className="mt-2 block max-w-3xl text-pretty text-sm leading-6 text-tx-1">{announcement}</span>
                  )}
                </span>
              </Link>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
