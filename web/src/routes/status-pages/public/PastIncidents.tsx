import type { TFunction } from 'i18next';
import { ArrowRight, CheckCircle2 } from 'lucide-react';
import { Link } from 'react-router-dom';

import type {
  PublicStatusPageIncident,
  PublicStatusPageSnapshot,
  StatusPageLanguage,
} from '@/api/statusPages';

import { zonedDay } from './format';
import { EndedIncidentRow } from './IncidentRow';
import { pastIncidentDays, pastPublicIncidents } from './routing';
import { SectionHeading } from './Section';

interface DayOfIncidents {
  key: string;
  label: string;
  incidents: PublicStatusPageIncident[];
}

/**
 * What happened lately and is over, under the day it ended on. Only the days
 * something happened on are listed: a fortnight of "nothing reported" says
 * the same thing fourteen times, and one line says it once.
 */
export function PastIncidents({
  snapshot,
  hrefOf,
  historyHref,
  timezone,
  language,
  copy,
}: {
  snapshot: PublicStatusPageSnapshot;
  hrefOf: (incident: PublicStatusPageIncident) => string;
  historyHref: string;
  timezone: string;
  language: StatusPageLanguage;
  copy: TFunction;
}) {
  const days = pastIncidentDays(snapshot);
  const groups = groupByDay(pastPublicIncidents(snapshot), timezone, language);

  return (
    <section className="mt-12 sm:mt-14" aria-labelledby="past-incidents-heading">
      <SectionHeading
        id="past-incidents-heading"
        title={copy('public.past.title')}
        meta={copy('public.past.period', { count: days })}
      />

      {groups.length === 0 ? (
        <p className="mt-5 flex items-center gap-2.5 rounded-xl border border-bd-0 bg-white px-5 py-5 text-sm leading-5 text-tx-1 sm:px-6">
          <CheckCircle2 aria-hidden className="h-4 w-4 shrink-0 text-green" strokeWidth={2.2} />
          {copy('public.past.empty', { count: days })}
        </p>
      ) : (
        <div className="mt-1">
          {groups.map((group) => (
            <section key={group.key} data-status-day={group.key} aria-labelledby={`past-day-${group.key}`}>
              <h3
                id={`past-day-${group.key}`}
                className="border-b border-bd-0 pb-2 pt-5 text-sm font-strong text-tx-2"
              >
                {group.label}
              </h3>
              <ul className="divide-y divide-bd-0">
                {group.incidents.map((incident) => (
                  <EndedIncidentRow
                    key={incident.id}
                    incident={incident}
                    snapshot={snapshot}
                    href={hrefOf(incident)}
                    timezone={timezone}
                    language={language}
                    copy={copy}
                    withDate={false}
                  />
                ))}
              </ul>
            </section>
          ))}
        </div>
      )}

      <Link
        to={historyHref}
        className="group mt-4 inline-flex min-h-[44px] items-center gap-1.5 rounded-sm text-sm font-strong text-tx-0 focus-visible:bg-bg-2"
      >
        <span className="underline-offset-4 group-hover:underline">{copy('public.past.history_link')}</span>
        <ArrowRight aria-hidden className="h-3.5 w-3.5 transition-transform group-hover:translate-x-0.5" />
      </Link>
    </section>
  );
}

function groupByDay(
  incidents: PublicStatusPageIncident[],
  timezone: string,
  language: StatusPageLanguage,
): DayOfIncidents[] {
  const groups = new Map<string, DayOfIncidents>();

  incidents.forEach((incident) => {
    const day = zonedDay(incident.ended_at ?? incident.started_at, timezone, language);
    const group = groups.get(day.key) ?? { ...day, incidents: [] };

    group.incidents.push(incident);
    groups.set(day.key, group);
  });

  return [...groups.values()];
}
