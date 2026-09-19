import * as statusPagesApi from '@/api/statusPages';
import type {
  PublicStatusPageIncident,
  PublicStatusPageSnapshot,
} from '@/api/statusPages';

import { isIncidentWithinHistory, statusPageHistoryDays } from './model';

export type PublicStatusSource = 'slug' | 'domain';

export function publicStatusPageQuery(source: PublicStatusSource, slug: string) {
  return {
    queryKey: ['public-status-page', source, source === 'slug' ? slug : 'current'] as const,
    queryFn: () =>
      source === 'domain'
        ? statusPagesApi.getPublicByDomain()
        : statusPagesApi.getPublic(slug),
    enabled: source === 'domain' || Boolean(slug),
    refetchInterval: 60_000,
  };
}

export function publicStatusHomePath(source: PublicStatusSource, slug: string): string {
  return source === 'domain' ? '/' : `/status/${encodeURIComponent(slug)}`;
}

export function publicStatusHistoryPath(source: PublicStatusSource, slug: string): string {
  return source === 'domain'
    ? '/history'
    : `/status/${encodeURIComponent(slug)}/history`;
}

export function publicStatusUptimePath(source: PublicStatusSource, slug: string): string {
  return source === 'domain'
    ? '/uptime'
    : `/status/${encodeURIComponent(slug)}/uptime`;
}

export function publicStatusPaths(
  source: PublicStatusSource,
  slug: string,
  language: string,
): { current: string; history: string; uptime: string } {
  const languageQuery = `?lang=${encodeURIComponent(language)}`;
  return {
    current: `${publicStatusHomePath(source, slug)}${languageQuery}`,
    history: `${publicStatusHistoryPath(source, slug)}${languageQuery}`,
    uptime: `${publicStatusUptimePath(source, slug)}${languageQuery}`,
  };
}

export function publicIncidentPath(
  source: PublicStatusSource,
  slug: string,
  incidentId: string,
): string {
  const encodedIncidentId = encodeURIComponent(incidentId);
  return source === 'domain'
    ? `/incidents/${encodedIncidentId}`
    : `/status/${encodeURIComponent(slug)}/incidents/${encodedIncidentId}`;
}

export function findPublicIncident(
  snapshot: PublicStatusPageSnapshot,
  incidentId: string,
): PublicStatusPageIncident | undefined {
  return [
    ...snapshot.active_incidents,
    ...snapshot.scheduled_maintenance,
    ...snapshot.history,
  ].find((incident) => incident.id === incidentId);
}

export interface RecentPublicIncident {
  incident: PublicStatusPageIncident;
  kind: 'active' | 'scheduled' | 'resolved';
}

export function recentPublicIncidentTime(row: RecentPublicIncident): number {
  return row.kind === 'resolved' && row.incident.ended_at
    ? row.incident.ended_at
    : row.incident.started_at;
}

export function recentPublicIncidents(
  snapshot: PublicStatusPageSnapshot,
): RecentPublicIncident[] {
  const rows = new Map<string, RecentPublicIncident>();
  const historyDays = statusPageHistoryDays(snapshot.page);
  snapshot.active_incidents.forEach((incident) => {
    rows.set(incident.id, { incident, kind: 'active' });
  });
  snapshot.scheduled_maintenance.forEach((incident) => {
    if (!rows.has(incident.id)) rows.set(incident.id, { incident, kind: 'scheduled' });
  });
  snapshot.history.forEach((incident) => {
    if (
      !rows.has(incident.id)
      && isIncidentWithinHistory(incident, snapshot.generated_at, historyDays)
    ) {
      rows.set(incident.id, { incident, kind: 'resolved' });
    }
  });

  return [...rows.values()].sort(
    (left, right) => recentPublicIncidentTime(right) - recentPublicIncidentTime(left),
  );
}
