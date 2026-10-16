import type { TFunction } from 'i18next';
import { Link } from 'react-router-dom';

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
import { lasted, listNames, microsDateTime } from './format';
import { MetaLine } from './MetaLine';
import { IMPACT_DOT } from './tone';

/**
 * Something that is over, as one line of a record: what it was, how bad, when
 * it ended, what it touched and how long it lasted. Its title is in the
 * page's own ink; how bad it was is the dot before it, not the colour of the
 * words.
 */
export function EndedIncidentRow({
  incident,
  snapshot,
  href,
  timezone,
  language,
  copy,
  withDate,
}: {
  incident: PublicStatusPageIncident;
  snapshot: PublicStatusPageSnapshot;
  href: string;
  timezone: string;
  language: StatusPageLanguage;
  copy: TFunction;
  /** Whether the day is written out; a row under a day's heading has no need of it. */
  withDate: boolean;
}) {
  const components = affectedComponentNames(incident, snapshot.components);
  const endedAt = incident.ended_at ?? incident.started_at;
  // Work that was called off did not last any time.
  const ran = incident.status !== 'cancelled' && incident.ended_at !== null;

  return (
    <li>
      <Link
        to={href}
        className="group grid min-h-[44px] grid-cols-[auto_minmax(0,1fr)_auto] items-start gap-x-3 py-4 transition-colors focus-visible:bg-bg-1 sm:gap-x-4"
      >
        <span aria-hidden className={cn('mt-2 h-2 w-2 shrink-0 rounded-full', IMPACT_DOT[incident.impact])} />
        <span className="min-w-0">
          <span className="block text-base font-bold leading-6 text-tx-0 underline-offset-4 group-hover:underline">
            {incident.title}
          </span>
          <MetaLine className="mt-1 text-sm leading-5 text-tx-2">
            <>{impactLabel(copy, incident.impact)}</>
            <>
              {incidentStatusLabel(copy, incident.status, incident.kind)}{' '}
              <time dateTime={microsDateTime(endedAt)} className="tabular-nums">
                {formatMicros(endedAt, timezone, language, withDate)}
              </time>
            </>
            {components.length > 0 && <>{listNames(components, language)}</>}
            {incident.postmortem && <>{copy('public.incident_detail.postmortem_available')}</>}
          </MetaLine>
        </span>
        {ran && (
          <span className="shrink-0 pt-0.5 text-right text-sm tabular-nums leading-5 text-tx-1">
            <span className="sr-only">{copy('public.incident_detail.duration')} </span>
            {lasted(copy, incident.started_at, endedAt)}
          </span>
        )}
      </Link>
    </li>
  );
}
