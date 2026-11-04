import { useQuery } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { useTranslation } from 'react-i18next';

import * as teamsApi from '@/api/teams';
import type { Incident, IncidentRouting } from '@/types/alerting';

/** The organization's teams: who is in each, and the names incidents are routed to. */
export function useTeams() {
  return useQuery({ queryKey: ['teams'], queryFn: () => teamsApi.list(), staleTime: 60_000, retry: false });
}

/** A catalog service by its name, with its namespace only when it is not the default one. */
export function serviceName(key: string): string {
  return key.startsWith('default/') ? key.slice('default/'.length) : key;
}

/** Why an incident went where it went, in a few words. */
export function routingReason(t: TFunction<'alerts'>, routing: IncidentRouting): string {
  return t(`routing.reasons.${routing.reason}`, {
    name: routing.ruleName ?? '',
    service: routing.service ? serviceName(routing.service) : '',
  });
}

/**
 * The team routing gave an incident and why, under the people who have it. An incident opened
 * before routing says nothing here.
 */
export function IncidentTeam({ incident }: { incident: Incident }) {
  const { t } = useTranslation('alerts');
  const teams = useTeams();

  if (!incident.teamId && !incident.routing) return null;

  const team = incident.teamId
    ? (teams.data?.find((item) => item.id === incident.teamId)?.name ?? incident.teamId)
    : null;

  return (
    <dl
      data-incident-team
      className="m-0 mt-2.5 grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-1.5 font-sans text-xs"
    >
      <dt className="text-tx-2">{t('drawer.routing.team')}</dt>
      <dd className="m-0 min-w-0 truncate text-tx-0">{team ?? t('drawer.routing.no_team')}</dd>
      {incident.routing && (
        <>
          <dt className="text-tx-2">{t('drawer.routing.reason')}</dt>
          <dd className="m-0 min-w-0 truncate text-tx-1">{routingReason(t, incident.routing)}</dd>
        </>
      )}
    </dl>
  );
}
