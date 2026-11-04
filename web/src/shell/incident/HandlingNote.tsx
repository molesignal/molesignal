import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { formatMicrosActive } from '@/lib/time';
import { useUsers } from '@/shell/useUsers';
import type { Incident } from '@/types/alerting';

/**
 * How people are handling an incident, said under its assignees: snoozed until when, merged into
 * which incident, severity set by whom.
 */
export function IncidentHandlingNote({ incident }: { incident: Incident }) {
  const { t } = useTranslation('alerts');
  const { nameOf } = useUsers();
  const lines: React.ReactNode[] = [];

  if (incident.snoozedUntil) {
    lines.push(
      t('incident_timeline.handling.snoozed', {
        until: formatMicrosActive(incident.snoozedUntil),
        actor: incident.snoozedBy ? nameOf(incident.snoozedBy) : '',
      }),
    );
  }

  if (incident.mergedInto) {
    lines.push(
      <>
        {t('incident_timeline.handling.merged')}{' '}
        <Link
          className="text-tx-0 underline-offset-2 hover:underline"
          to={`/alerts/incidents/${incident.mergedInto}`}
        >
          {t('incident_timeline.merged_target')}
        </Link>
      </>,
    );
  }

  if (incident.severitySetBy) {
    lines.push(t('incident_timeline.handling.severity_set', { actor: nameOf(incident.severitySetBy) }));
  }

  if (incident.inhibitionLiftedBy) {
    lines.push(t('incident_timeline.handling.inhibition_lifted', { actor: nameOf(incident.inhibitionLiftedBy) }));
  }

  if (lines.length === 0) return null;

  return (
    <ul data-incident-handling className="mt-2 flex flex-col gap-0.5 font-sans text-xs text-tx-2">
      {lines.map((line, index) => (
        <li key={index}>{line}</li>
      ))}
    </ul>
  );
}
