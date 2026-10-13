import * as React from 'react';

import type { PublicStatusContext } from './Frame';
import { formatArchiveMonthKey, incidentHistoryMonths } from './history';
import { EndedIncidentRow } from './IncidentRow';
import { publicIncidentPath } from './routing';

/** How many of a month's incidents are listed before the rest is asked for. */
const MONTH_PREVIEW_COUNT = 6;

/**
 * Everything that happened and is over, month by month and newest first. A
 * month nothing happened in is still listed, and says so: in a record of
 * reliability, the quiet months are part of what is being read.
 */
export function PublicIncidentHistory({
  context,
  monthKeys,
}: {
  context: PublicStatusContext;
  /** The months being looked at, oldest first. */
  monthKeys: string[];
}) {
  const { snapshot, page, source, slug, language, languageQuery, copy } = context;
  const recorded = new Map(
    incidentHistoryMonths(snapshot, page.timezone, language).map((month) => [month.key, month]),
  );
  const months = [...monthKeys].reverse().map(
    (key) => recorded.get(key) ?? { key, label: formatArchiveMonthKey(key, language), incidents: [] },
  );
  const [expandedMonths, setExpandedMonths] = React.useState<Set<string>>(() => new Set());

  return (
    <div className="mt-2">
      {months.map((month) => {
        const expanded = expandedMonths.has(month.key);
        const incidents = expanded ? month.incidents : month.incidents.slice(0, MONTH_PREVIEW_COUNT);

        return (
          <section key={month.key} aria-labelledby={`history-month-${month.key}`} className="pt-8 sm:pt-10">
            <div className="flex items-baseline justify-between gap-4 border-b border-bd-0 pb-2.5">
              <h3
                id={`history-month-${month.key}`}
                className="text-lg font-display-strong tracking-[-0.01em] text-tx-0 sm:text-xl"
              >
                {month.label}
              </h3>
              {month.incidents.length > 0 && (
                <span className="shrink-0 text-sm tabular-nums text-tx-2">
                  {copy('public.archive.incident_count', { count: month.incidents.length })}
                </span>
              )}
            </div>

            {incidents.length === 0 ? (
              <p className="py-5 text-sm leading-5 text-tx-2">{copy('public.archive.no_incidents_in_month')}</p>
            ) : (
              <ul className="divide-y divide-bd-0">
                {incidents.map((incident) => (
                  <EndedIncidentRow
                    key={incident.id}
                    incident={incident}
                    snapshot={snapshot}
                    href={`${publicIncidentPath(source, slug, incident.id)}${languageQuery}`}
                    timezone={page.timezone}
                    language={language}
                    copy={copy}
                    withDate
                  />
                ))}
              </ul>
            )}

            {month.incidents.length > MONTH_PREVIEW_COUNT && (
              <button
                type="button"
                aria-expanded={expanded}
                className="mt-2 min-h-[44px] rounded-md bg-bg-2 px-4 text-sm font-strong text-tx-1 transition-colors hover:text-tx-0 focus-visible:text-tx-0 sm:min-h-9"
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
                  ? copy('public.archive.show_fewer_incidents')
                  : copy('public.archive.show_all_incidents', { count: month.incidents.length })}
              </button>
            )}
          </section>
        );
      })}
    </div>
  );
}
