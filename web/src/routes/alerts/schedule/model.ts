import type {
  CoverageSegment,
  Rotation,
  Schedule,
} from '@/types/alerting';

export const MICROS_PER_MINUTE = 60_000_000;
export const MICROS_PER_HOUR = 60 * MICROS_PER_MINUTE;
export const MICROS_PER_DAY = 24 * MICROS_PER_HOUR;

export type ScheduleStatus =
  | 'active'
  | 'switching'
  | 'not_started'
  | 'paused'
  | 'gap';

export interface OnCallResolution {
  userId: string;
  source: 'rotation' | 'override';
  rotationId?: string;
  overrideId?: string;
}

export interface ScheduleTimelineSegment {
  id: string;
  startAt: number;
  endAt: number;
  userId: string | null;
  source: 'rotation' | 'override' | 'gap';
  rotationId?: string;
  overrideId?: string;
}

export function safeTimeZone(timeZone: string): string {
  try {
    new Intl.DateTimeFormat('en-US', { timeZone }).format();
    return timeZone || 'UTC';
  } catch {
    return 'UTC';
  }
}

/* Who is on call when comes from the server (`schedule.coverage`), worked out in the
 * schedule's timezone with its daylight-saving changes and minute-precise windows.
 * These helpers only read it: outside the covered stretch nobody is known to be on call. */

/** The server's segments, with neighbours that only differ in whom an override replaces joined up. */
function segmentsOf(schedule: Schedule): CoverageSegment[] {
  const joined: CoverageSegment[] = [];
  for (const segment of schedule.coverage?.segments ?? []) {
    const last = joined[joined.length - 1];
    if (last && last.endAt === segment.startAt && sameHolder(last, segment)) {
      joined[joined.length - 1] = { ...last, endAt: segment.endAt };
    } else {
      joined.push(segment);
    }
  }
  return joined;
}

function sameHolder(left: CoverageSegment, right: CoverageSegment): boolean {
  return (
    left.source === right.source
    && left.userId === right.userId
    && left.rotationId === right.rotationId
    && left.overrideId === right.overrideId
  );
}

function segmentAt(
  schedule: Schedule,
  atMicros: number,
): CoverageSegment | undefined {
  return segmentsOf(schedule).find(
    (segment) => segment.startAt <= atMicros && atMicros < segment.endAt,
  );
}

export function resolveScheduleAt(
  schedule: Schedule,
  atMicros: number,
): OnCallResolution | null {
  if (!schedule.enabled) return null;
  const segment = segmentAt(schedule, atMicros);
  if (!segment?.userId || segment.source === 'gap') return null;
  return {
    userId: segment.userId,
    source: segment.source,
    ...(segment.rotationId ? { rotationId: segment.rotationId } : {}),
    ...(segment.overrideId ? { overrideId: segment.overrideId } : {}),
  };
}

/** Under an override at `atMicros`: whom the rotation would have had on call. */
export function replacedUserAt(schedule: Schedule, atMicros: number): string | null {
  const segment = (schedule.coverage?.segments ?? []).find(
    (item) => item.startAt <= atMicros && atMicros < item.endAt,
  );
  return segment?.source === 'override' ? segment.replacedUserId ?? null : null;
}

export function scheduleMemberIds(schedule: Schedule): string[] {
  return Array.from(
    new Set(schedule.rotations.flatMap((rotation) => rotation.members)),
  );
}

/** When the stretch on call at `atMicros` began: a handoff, a window opening or an override. */
export function resolutionStartedAt(
  schedule: Schedule,
  resolution: OnCallResolution | null,
  atMicros: number,
): number | null {
  if (!resolution) return null;
  return segmentAt(schedule, atMicros)?.startAt ?? null;
}

/** The next time someone else (or nobody) takes over, within `horizonMicros`. */
export function nextScheduleBoundary(
  schedule: Schedule,
  afterMicros: number,
  horizonMicros = 14 * MICROS_PER_DAY,
): number | null {
  if (!schedule.enabled) return null;
  const limit = afterMicros + horizonMicros;
  const next = segmentsOf(schedule).find(
    (segment) => segment.startAt > afterMicros && segment.startAt <= limit,
  );
  return next?.startAt ?? null;
}

export function scheduleStatus(
  schedule: Schedule,
  nowMicros: number,
): ScheduleStatus {
  if (!schedule.enabled) return 'paused';
  const members = scheduleMemberIds(schedule);
  if (members.length === 0) return 'gap';
  const firstStart = Math.min(
    ...schedule.rotations
      .filter((rotation) => rotation.members.length > 0)
      .map((rotation) => rotation.startAt),
  );
  if (Number.isFinite(firstStart) && nowMicros < firstStart) {
    return 'not_started';
  }
  if (!resolveScheduleAt(schedule, nowMicros)) return 'gap';
  const next = nextScheduleBoundary(schedule, nowMicros);
  if (next && next - nowMicros <= 2 * MICROS_PER_HOUR) {
    return 'switching';
  }
  return 'active';
}

export function liveOrFutureOverrides(
  schedule: Schedule,
  nowMicros: number,
): Schedule['overrides'] {
  return schedule.overrides
    .filter((override) => override.endAt > nowMicros)
    .sort((a, b) => a.startAt - b.startAt);
}

export function buildScheduleTimeline(
  schedule: Schedule,
  fromMicros: number,
  days = 7,
): ScheduleTimelineSegment[] {
  const endAt = fromMicros + Math.max(1, days) * MICROS_PER_DAY;
  return segmentsOf(schedule)
    .filter((segment) => segment.endAt > fromMicros && segment.startAt < endAt)
    .slice(0, 80)
    .map((segment) => {
      const startAt = Math.max(segment.startAt, fromMicros);
      const userId = schedule.enabled ? segment.userId ?? null : null;
      return {
        id: `${startAt}-${segment.overrideId ?? segment.rotationId ?? 'gap'}`,
        startAt,
        endAt: Math.min(segment.endAt, endAt),
        userId,
        source: userId ? segment.source : 'gap',
        ...(segment.rotationId ? { rotationId: segment.rotationId } : {}),
        ...(segment.overrideId ? { overrideId: segment.overrideId } : {}),
      };
    });
}

export function rotationRole(schedule: Schedule): 'primary' | 'secondary' {
  const source = `${schedule.name} ${schedule.rotations[0]?.name ?? ''}`
    .toLowerCase();
  return /(secondary|backup|备用)/.test(source) ? 'secondary' : 'primary';
}

export function rotationKindKey(
  rotation: Rotation,
): 'daily' | 'weekly' | 'workdays' | 'custom' {
  const [only, ...more] = rotation.activeWindows ?? [];
  if (
    rotation.kind === 'daily'
    && only
    && more.length === 0
    && only.weekdayMask === 62
    && only.start === '00:00'
    && only.end === '24:00'
  ) {
    return 'workdays';
  }
  if (typeof rotation.kind === 'string') return rotation.kind;
  return 'custom';
}

export function timezoneDisplay(timeZone: string): {
  technical: string;
  label: string;
} {
  const normalized = safeTimeZone(timeZone);
  const labels: Record<string, string> = {
    UTC: '协调世界时',
    'Etc/UTC': '协调世界时',
    'Asia/Shanghai': '北京时间',
    'Asia/Hong_Kong': '香港时间',
    'Asia/Tokyo': '日本标准时间',
    'Europe/London': '伦敦时间',
    'America/New_York': '纽约时间',
    'America/Los_Angeles': '洛杉矶时间',
  };
  const offset = new Intl.DateTimeFormat('en-US', {
    timeZone: normalized,
    timeZoneName: 'shortOffset',
  })
    .formatToParts(new Date())
    .find((part) => part.type === 'timeZoneName')?.value;
  return {
    technical:
      normalized === 'UTC' || normalized === 'Etc/UTC'
        ? 'UTC'
        : offset?.replace('GMT', 'UTC') ?? normalized,
    label: labels[normalized] ?? normalized,
  };
}
