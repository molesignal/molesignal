import type {
  BusinessService,
  BusinessServiceDependencyKind,
  BusinessServiceHealthPeriod,
  BusinessServiceStatus,
} from '@/api/businessServices';
import type { PillTone } from '@/shell/chrome';

export const BUSINESS_SERVICE_STATUSES: readonly BusinessServiceStatus[] = [
  'operational',
  'degraded_performance',
  'partial_outage',
  'major_outage',
];

export const DEPENDENCY_KINDS: readonly BusinessServiceDependencyKind[] = [
  'technical_service',
  'synthetic_monitor',
  'alert_rule',
  'business_service',
];

export const STATUS_TONE: Record<BusinessServiceStatus, PillTone> = {
  operational: 'green',
  degraded_performance: 'yellow',
  partial_outage: 'orange',
  major_outage: 'red',
};

/** The fill a period in this status takes on the timeline. */
export const STATUS_FILL: Record<BusinessServiceStatus, string> = {
  operational: 'bg-green',
  degraded_performance: 'bg-yellow',
  partial_outage: 'bg-orange',
  major_outage: 'bg-red',
};

/** How far back the timeline reaches: the seven days the server keeps. */
export const TIMELINE_MICROS = 7 * 24 * 60 * 60 * 1_000_000;

/** Where the thing a dependency names is looked at in the product. */
export function dependencyHref(kind: BusinessServiceDependencyKind, target: string): string {
  const id = encodeURIComponent(target);
  switch (kind) {
    case 'technical_service':
      return `/apm/services/${id}`;
    case 'synthetic_monitor':
      return `/synthetics/checks/${id}`;
    case 'alert_rule':
      return `/alerts/rules/${id}/edit`;
    case 'business_service':
      return `/business-services/${id}`;
  }
}

export function statusRank(status: BusinessServiceStatus): number {
  return BUSINESS_SERVICE_STATUSES.indexOf(status);
}

/** The services that need a look first: the worst health, then by name. */
export function sortServices(services: readonly BusinessService[]): BusinessService[] {
  return [...services].sort(
    (left, right) =>
      statusRank(right.health.status) - statusRank(left.health.status)
      || left.name.localeCompare(right.name),
  );
}

export interface TimelineSegment {
  status: BusinessServiceStatus;
  /** Where the segment starts, as a share of the window. */
  offset: number;
  /** How much of the window it covers. */
  width: number;
  startedAt: number;
  endedAt: number;
}

/**
 * The periods of the last seven days as shares of the bar, oldest first. Time
 * before the first evaluation is not a status, so it is left blank.
 */
export function timelineSegments(
  history: readonly BusinessServiceHealthPeriod[],
  now: number,
  span = TIMELINE_MICROS,
): TimelineSegment[] {
  const from = now - span;

  return history.flatMap((period) => {
    const start = Math.max(period.started_at, from);
    const end = Math.min(period.ended_at ?? now, now);
    if (end <= start) return [];

    return [{
      status: period.status,
      offset: (start - from) / span,
      width: (end - start) / span,
      startedAt: start,
      endedAt: end,
    }];
  });
}

/** The share of the evaluated time the service was operational, or null before any evaluation. */
export function operationalShare(segments: readonly TimelineSegment[]): number | null {
  const evaluated = segments.reduce((total, segment) => total + segment.width, 0);
  if (evaluated <= 0) return null;

  const operational = segments
    .filter((segment) => segment.status === 'operational')
    .reduce((total, segment) => total + segment.width, 0);

  return operational / evaluated;
}
