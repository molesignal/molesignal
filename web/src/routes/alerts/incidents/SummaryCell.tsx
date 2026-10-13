import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { Incident } from '@/types/alerting';
import { SeverityRail } from '@/viz/SeverityRail';

import { incidentService } from './model';

/**
 * An incident as it is named in a list: what happened, then how bad, where,
 * and anything the list adds. The severity is the rail at the row's edge and a
 * word in the line under the title, the same in every list of the module.
 */
export function IncidentSummaryCell({
  incident,
  extra,
  onOpen,
}: {
  incident: Incident;
  /** What this list knows besides: the rule's name, the incident's id. */
  extra?: React.ReactNode;
  /** Makes the title the way into the incident, for a list whose rows are not. */
  onOpen?: (() => void) | undefined;
}) {
  const { t } = useTranslation('alerts');
  const service = incidentService(incident);

  return (
    <div className="flex min-w-0 items-stretch gap-[12px]">
      <SeverityRail severity={incident.severity} className="w-[3px] shrink-0 rounded-full" />
      <div className="min-w-0 py-1">
        {onOpen ? (
          <button
            type="button"
            onClick={onOpen}
            data-testid="open-incident"
            className="block max-w-full truncate text-left font-strong text-tx-0 underline-offset-4 outline-none hover:underline focus-visible:underline"
          >
            {incident.summary}
          </button>
        ) : (
          <div className="truncate font-strong text-tx-0">{incident.summary}</div>
        )}
        <div className="mt-0.5 flex min-w-0 items-center gap-[6px] truncate text-xs text-tx-2">
          <span className="truncate">
            {[t(`severity.${incident.severity}`), service].filter(Boolean).join(' · ')}
          </span>
          {extra && (
            <>
              <span aria-hidden="true">·</span>
              <span className="truncate">{extra}</span>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
