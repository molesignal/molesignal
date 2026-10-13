import * as statusPagesApi from '@/api/statusPages';
import type {
  PublicStatusPageIncident,
  PublicStatusPageSnapshot,
} from '@/api/statusPages';

import { statusPageHistoryDays } from '../model';

/** A page is reached by its slug under the app's own host, or as the root of its own domain. */
export type PublicStatusSource = 'slug' | 'domain';

/** The views a public status page has. */
export type PublicStatusView = 'status' | 'history' | 'uptime' | 'incident';

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

export interface PublicStatusPaths {
  current: string;
  history: string;
  uptime: string;
}

export function publicStatusPaths(
  source: PublicStatusSource,
  slug: string,
  language: string,
): PublicStatusPaths {
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

const DAY_MICROS = 24 * 60 * 60 * 1_000_000;

/** How far back the status page itself looks; the history view has the rest. */
export const PAST_INCIDENT_DAYS = 14;

/** The days of past incidents the status page lists: two weeks, or all the page keeps if less. */
export function pastIncidentDays(snapshot: PublicStatusPageSnapshot): number {
  return Math.min(PAST_INCIDENT_DAYS, statusPageHistoryDays(snapshot.page));
}

/**
 * What is over, newest first: the incidents and maintenance that ended within
 * the days the status page lists. What is still going on is not here; it is
 * said at the top of the page.
 */
export function pastPublicIncidents(snapshot: PublicStatusPageSnapshot): PublicStatusPageIncident[] {
  const open = new Set(
    [...snapshot.active_incidents, ...snapshot.scheduled_maintenance].map((incident) => incident.id),
  );
  const since = snapshot.generated_at - pastIncidentDays(snapshot) * DAY_MICROS;
  const endOf = (incident: PublicStatusPageIncident) => incident.ended_at ?? incident.started_at;

  return snapshot.history
    .filter((incident) => !open.has(incident.id) && endOf(incident) >= since)
    .sort((left, right) => endOf(right) - endOf(left));
}

const IMPACT_RANK: Record<PublicStatusPageIncident['impact'], number> = {
  critical: 3,
  major: 2,
  minor: 1,
  maintenance: 0,
};

/**
 * What is affecting the service now: the incidents under way and the
 * maintenance that has begun. The worst comes first, and among equals the one
 * that began last.
 */
export function ongoingPublicIncidents(snapshot: PublicStatusPageSnapshot): PublicStatusPageIncident[] {
  return [...snapshot.active_incidents].sort(
    (left, right) =>
      IMPACT_RANK[right.impact] - IMPACT_RANK[left.impact] || right.started_at - left.started_at,
  );
}

/** Maintenance that is announced and has not begun, the soonest first. */
export function upcomingMaintenance(snapshot: PublicStatusPageSnapshot): PublicStatusPageIncident[] {
  return [...snapshot.scheduled_maintenance].sort((left, right) => left.started_at - right.started_at);
}

/** An incident's updates, the latest first: what a reader arriving now wants to read first. */
export function latestFirst(incident: PublicStatusPageIncident): PublicStatusPageIncident['updates'] {
  return [...incident.updates].sort((left, right) => right.created_at - left.created_at);
}
