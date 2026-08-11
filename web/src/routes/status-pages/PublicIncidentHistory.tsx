import type { TFunction } from 'i18next';
import * as React from 'react';
import { Link } from 'react-router-dom';

import type {
  IncidentImpact,
  PublicStatusPageIncident,
  PublicStatusPageSnapshot,
  StatusPageLanguage,
} from '@/api/statusPages';
import { cn } from '@/shell/lib/cn';

import { formatMicros, statusPageHistoryDays } from './model';
import {
  formatArchiveMonthKey,
  incidentHistoryMonths,
} from './publicStatusHistory';
import {
  type PublicStatusSource,
  publicIncidentPath,
} from './publicStatusRouting';

const MONTH_PREVIEW_COUNT = 3;

const IMPACT_TITLE: Record<IncidentImpact, string> = {
  critical: 'text-red-soft',
  major: 'text-orange-soft',
  minor: 'text-yellow-soft',
  maintenance: 'text-blue-soft',
};

export function PublicIncidentHistory({
  snapshot,
  source,
  slug,
  timezone,
  language,
  languageQuery,
  monthKeys,
  copy,
}: {
  snapshot: PublicStatusPageSnapshot;
  source: PublicStatusSource;
  slug: string;
  timezone: string;
  language: StatusPageLanguage;
  languageQuery: string;
  monthKeys: string[];
  copy: TFunction;
}) {
  const incidentMonths = new Map(
    incidentHistoryMonths(snapshot, timezone, language).map((month) => [
      month.key,
      month,
    ]),
  );
  const months = [...monthKeys].reverse().map(
    (key) =>
      incidentMonths.get(key) ?? {
        key,
        label: formatArchiveMonthKey(key, language),
        incidents: [],
      },
  );
  const historyDays = statusPageHistoryDays(snapshot.page);
  const [expandedMonths, setExpandedMonths] = React.useState<Set<string>>(
    () => new Set(),
  );

  return (
    <section className="mt-8 sm:mt-10" aria-labelledby="incident-history-heading">
      <header className="border-b border-bd-0 pb-5">
        <h2
          id="incident-history-heading"
          className="text-2xl font-display-strong tracking-[-0.02em] text-tx-0 sm:text-3xl"
        >
          {copy('public.archive.history_title')}
        </h2>
        <p className="mt-2 text-sm leading-6 text-tx-2">
          {copy('public.archive.history_description', { count: historyDays })}
        </p>
      </header>

      {months.length > 0 && (
        <div className="space-y-12 pt-8 sm:space-y-14 sm:pt-10">
          {months.map((month) => {
            const expanded = expandedMonths.has(month.key);
            const incidents = expanded
              ? month.incidents
              : month.incidents.slice(0, MONTH_PREVIEW_COUNT);
            return (
              <section key={month.key} aria-labelledby={`history-month-${month.key}`}>
                <div className="flex items-end justify-between gap-4 border-b border-bd-0 pb-2.5">
                  <h3
                    id={`history-month-${month.key}`}
                    className="text-xl font-display-strong tracking-[-0.015em] text-tx-0 sm:text-2xl"
                  >
                    {month.label}
                  </h3>
                  <span className="text-sm tabular-nums text-tx-3">
                    {copy('public.archive.incident_count', {
                      count: month.incidents.length,
                    })}
                  </span>
                </div>

                {incidents.length === 0 ? (
                  <p className="border-b border-bd-0 py-7 text-sm text-tx-3">
                    {copy('public.archive.no_incidents_in_month')}
                  </p>
                ) : (
                  <div className="divide-y divide-bd-0">
                    {incidents.map((incident) => (
                    <HistoryIncidentRow
                      key={incident.id}
                      incident={incident}
                      href={`${publicIncidentPath(source, slug, incident.id)}${languageQuery}`}
                      timezone={timezone}
                      language={language}
                      copy={copy}
                    />
                    ))}
                  </div>
                )}

                {month.incidents.length > MONTH_PREVIEW_COUNT && (
                  <button
                    type="button"
                    className="mt-2 min-h-11 w-full border border-bd-0 px-4 text-sm font-strong text-tx-2 transition-colors hover:bg-bg-1 hover:text-tx-0 focus-visible:bg-bg-1 focus-visible:text-tx-0"
                    onClick={() => {
                      setExpandedMonths((current) => {
                        const next = new Set(current);
                        if (expanded) next.delete(month.key);
                        else next.add(month.key);
                        return next;
                      });
                    }}
                  >
                    {expanded
                      ? copy('public.archive.show_top_incidents', {
                          count: MONTH_PREVIEW_COUNT,
                        })
                      : copy('public.archive.show_all_incidents', {
                          count: month.incidents.length,
                        })}
                  </button>
                )}
              </section>
            );
          })}
        </div>
      )}
    </section>
  );
}

function HistoryIncidentRow({
  incident,
  href,
  timezone,
  language,
  copy,
}: {
  incident: PublicStatusPageIncident;
  href: string;
  timezone: string;
  language: StatusPageLanguage;
  copy: TFunction;
}) {
  return (
    <Link
      to={href}
      className="group block py-5 transition-colors focus-visible:bg-bg-1 sm:py-6"
    >
      <h4
        className={cn(
          'text-lg font-bold leading-7 transition-colors sm:text-xl',
          IMPACT_TITLE[incident.impact],
        )}
      >
        {incident.title}
      </h4>
      <p className="mt-1 text-sm font-strong text-tx-1">
        {copy('public.archive.resolved')}
      </p>
      <p className="mt-1 flex flex-wrap items-center gap-x-1.5 text-sm tabular-nums text-tx-3">
        <time dateTime={microsDateTime(incident.started_at)}>
          {formatMicros(incident.started_at, timezone, language)}
        </time>
        {incident.ended_at && (
          <>
            <span aria-hidden>–</span>
            <time dateTime={microsDateTime(incident.ended_at)}>
              {formatMicros(incident.ended_at, timezone, language)}
            </time>
          </>
        )}
      </p>
    </Link>
  );
}

function microsDateTime(micros: number): string {
  return new Date(Math.floor(micros / 1_000)).toISOString();
}
