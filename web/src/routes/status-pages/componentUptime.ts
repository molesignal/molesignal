import type {
  ComponentStatus,
  PublicStatusPageComponent,
  PublicStatusPageIncident,
  PublicStatusPageSnapshot,
} from '@/api/statusPages';

const DAY_MICROS = 24 * 60 * 60 * 1_000_000;
const DEFAULT_DAYS = 90;

const STATUS_RANK: Record<ComponentStatus, number> = {
  operational: 0,
  maintenance: 1,
  degraded_performance: 2,
  partial_outage: 3,
  major_outage: 4,
};

export interface ComponentUptimeDay {
  startAt: number;
  endAt: number;
  status: ComponentStatus;
  hasData: boolean;
  downtimeMicros: number;
  eligibleMicros: number;
  unavailableMicros: number;
  incidents: Array<{
    id: string;
    title: string;
  }>;
}

export interface ComponentUptime {
  days: ComponentUptimeDay[];
  percentage: number | null;
}

interface StatusInterval {
  startAt: number;
  endAt: number;
  status: ComponentStatus;
  incident?: { id: string; title: string };
}

interface StatusSegment {
  startAt: number;
  endAt: number;
  status: ComponentStatus;
  hasData: boolean;
  incidents: Array<{ id: string; title: string }>;
}

export function componentUptime(
  component: PublicStatusPageComponent,
  snapshot: PublicStatusPageSnapshot,
  dayCount = DEFAULT_DAYS,
): ComponentUptime {
  const endAt = snapshot.generated_at;
  const startAt = endAt - dayCount * DAY_MICROS;
  const segments = componentStatusSegments(component, snapshot, startAt, endAt);
  const days = Array.from({ length: dayCount }, (_, index) => {
    const dayStart = startAt + index * DAY_MICROS;
    const dayEnd = dayStart + DAY_MICROS;
    return uptimeDay(segments, dayStart, dayEnd);
  });

  return { days, percentage: availabilityPercentage(segments) };
}

/** Builds calendar-day uptime cells in the status page's configured timezone. */
export function componentCalendarUptime(
  component: PublicStatusPageComponent,
  snapshot: PublicStatusPageSnapshot,
  dayCount: number,
  timezone: string,
): ComponentUptime {
  const endAt = snapshot.generated_at;
  const count = Math.max(1, Math.floor(dayCount));
  const today = zonedDateParts(endAt, timezone);
  const todayOrdinal = Date.UTC(today.year, today.month - 1, today.day);
  const dayStarts = Array.from({ length: count }, (_, index) => {
    const ordinal = todayOrdinal - (count - index - 1) * DAY_MICROS / 1_000;
    const date = new Date(ordinal);
    return zonedMidnightMicros(
      {
        year: date.getUTCFullYear(),
        month: date.getUTCMonth() + 1,
        day: date.getUTCDate(),
      },
      timezone,
    );
  });
  const startAt = dayStarts[0] ?? endAt;
  const segments = componentStatusSegments(component, snapshot, startAt, endAt);
  const days = dayStarts.map((dayStart, index) =>
    uptimeDay(
      segments,
      dayStart,
      index < dayStarts.length - 1 ? dayStarts[index + 1] ?? endAt : endAt,
    ),
  );

  return {
    days,
    percentage: availabilityPercentage(segments),
  };
}

function uptimeDay(
  segments: StatusSegment[],
  dayStart: number,
  dayEnd: number,
): ComponentUptimeDay {
  const daySegments = segments.filter(
    (segment) => segment.startAt < dayEnd && segment.endAt > dayStart,
  );
  let status: ComponentStatus = 'operational';
  const incidents = new Map<string, { id: string; title: string }>();

  daySegments.forEach((segment) => {
    if (segment.hasData && STATUS_RANK[segment.status] > STATUS_RANK[status]) {
      status = segment.status;
    }
    segment.incidents.forEach((incident) => incidents.set(incident.id, incident));
  });

  return {
    startAt: dayStart,
    endAt: dayEnd,
    status,
    hasData: daySegments.some((segment) => segment.hasData),
    downtimeMicros: daySegments.reduce((duration, segment) => {
      if (!segment.hasData || segment.status === 'operational') return duration;
      return duration
        + Math.max(
          0,
          Math.min(segment.endAt, dayEnd) - Math.max(segment.startAt, dayStart),
        );
    }, 0),
    eligibleMicros: daySegments.reduce((duration, segment) => {
      if (!segment.hasData || segment.status === 'maintenance') return duration;
      return duration
        + Math.max(
          0,
          Math.min(segment.endAt, dayEnd) - Math.max(segment.startAt, dayStart),
        );
    }, 0),
    unavailableMicros: daySegments.reduce((duration, segment) => {
      if (
        !segment.hasData
        || segment.status === 'operational'
        || segment.status === 'maintenance'
      ) {
        return duration;
      }
      return duration
        + Math.max(
          0,
          Math.min(segment.endAt, dayEnd) - Math.max(segment.startAt, dayStart),
        );
    }, 0),
    incidents: [...incidents.values()],
  };
}

function componentStatusSegments(
  component: PublicStatusPageComponent,
  snapshot: PublicStatusPageSnapshot,
  windowStart: number,
  windowEnd: number,
): StatusSegment[] {
  const intervals: StatusInterval[] = (snapshot.component_status_events ?? [])
    .filter((event) => event.component_id === component.id)
    .map((event) => ({
      startAt: Math.max(event.started_at, windowStart),
      endAt: Math.min(event.ended_at ?? windowEnd, windowEnd),
      status: event.status,
    }))
    .filter((interval) => interval.endAt > interval.startAt);

  // Compatibility with snapshots produced before component history existed.
  // New servers always return at least the component's initial status event.
  if (intervals.length === 0) {
    intervals.push({
      startAt: windowStart,
      endAt: windowEnd,
      status: component.status,
    });
  }

  const incidents = new Map<string, PublicStatusPageIncident>();
  [
    ...snapshot.active_incidents,
    ...snapshot.history,
    ...snapshot.scheduled_maintenance.filter(
      (incident) => incident.started_at <= snapshot.generated_at,
    ),
  ].forEach((incident) => incidents.set(incident.id, incident));

  // Published incidents overlay the persisted manual baseline and keep their
  // identity so affected days can link back to the public timeline.
  intervals.push(
    ...[...incidents.values()]
      .filter((incident) => incident.component_ids.includes(component.id))
      .map((incident) => ({
        startAt: Math.max(incident.started_at, windowStart),
        endAt: Math.min(incident.ended_at ?? windowEnd, windowEnd),
        status: incidentComponentStatus(incident),
        incident: { id: incident.id, title: incident.title },
      }))
      .filter((interval) => interval.endAt > interval.startAt),
  );

  const boundaries = new Set<number>([windowStart, windowEnd]);
  intervals.forEach((interval) => {
    boundaries.add(interval.startAt);
    boundaries.add(interval.endAt);
  });
  const sorted = [...boundaries].sort((left, right) => left - right);

  return sorted.slice(0, -1).map((startAt, index) => {
    const endAt = sorted[index + 1] ?? windowEnd;
    const active = intervals.filter(
      (interval) => interval.startAt < endAt && interval.endAt > startAt,
    );
    const status = active.reduce<ComponentStatus>(
      (current, interval) =>
        STATUS_RANK[interval.status] > STATUS_RANK[current]
          ? interval.status
          : current,
      'operational',
    );
    const segmentIncidents = new Map<string, { id: string; title: string }>();
    active.forEach((interval) => {
      if (interval.incident) {
        segmentIncidents.set(interval.incident.id, interval.incident);
      }
    });
    return {
      startAt,
      endAt,
      status,
      hasData: active.length > 0,
      incidents: [...segmentIncidents.values()],
    };
  });
}

function availabilityPercentage(segments: StatusSegment[]): number | null {
  let eligibleDuration = 0;
  let unavailableDuration = 0;

  segments.forEach((segment) => {
    if (!segment.hasData || segment.status === 'maintenance') return;
    const duration = Math.max(0, segment.endAt - segment.startAt);
    eligibleDuration += duration;
    if (segment.status !== 'operational') unavailableDuration += duration;
  });

  if (eligibleDuration === 0) return null;
  return Math.max(0, 100 - (unavailableDuration / eligibleDuration) * 100);
}

function incidentComponentStatus(
  incident: PublicStatusPageIncident,
): ComponentStatus {
  if (incident.kind === 'maintenance' || incident.impact === 'maintenance') {
    return 'maintenance';
  }
  if (incident.impact === 'critical') return 'major_outage';
  if (incident.impact === 'major') return 'partial_outage';
  return 'degraded_performance';
}

function zonedDateParts(
  timestamp: number,
  timezone: string,
): { year: number; month: number; day: number } {
  const values = dateTimeParts(timestamp, timezone);
  return { year: values.year, month: values.month, day: values.day };
}

function zonedMidnightMicros(
  date: { year: number; month: number; day: number },
  timezone: string,
): number {
  const desiredAsUtc = Date.UTC(date.year, date.month - 1, date.day);
  let candidate = desiredAsUtc;

  for (let iteration = 0; iteration < 3; iteration += 1) {
    const shown = dateTimeParts(candidate * 1_000, timezone);
    const shownAsUtc = Date.UTC(
      shown.year,
      shown.month - 1,
      shown.day,
      shown.hour === 24 ? 0 : shown.hour,
      shown.minute,
      shown.second,
    );
    const correction = desiredAsUtc - shownAsUtc;
    candidate += correction;
    if (correction === 0) break;
  }

  return candidate * 1_000;
}

function dateTimeParts(
  timestamp: number,
  timezone: string,
): {
  year: number;
  month: number;
  day: number;
  hour: number;
  minute: number;
  second: number;
} {
  const values = Object.fromEntries(
    new Intl.DateTimeFormat('en-US', {
      year: 'numeric',
      month: 'numeric',
      day: 'numeric',
      hour: 'numeric',
      minute: 'numeric',
      second: 'numeric',
      hourCycle: 'h23',
      timeZone: timezone,
    })
      .formatToParts(new Date(Math.floor(timestamp / 1_000)))
      .map(({ type, value }) => [type, value]),
  );
  return {
    year: Number(values.year),
    month: Number(values.month),
    day: Number(values.day),
    hour: Number(values.hour),
    minute: Number(values.minute),
    second: Number(values.second),
  };
}
